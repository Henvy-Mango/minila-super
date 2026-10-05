//! XW06A GPIO reader -> TouchSampleEvent -> gesture processor -> RMK KeyboardEvent.

use embassy_rp::gpio::Input;
use embassy_time::{Instant, Timer};
use rmk::event::{publish_event_async, KeyboardEvent};
use rmk::macros::{event, input_device, processor};

pub mod gesture;
use gesture::{Config, Gesture, Recognizer};

const SAMPLE_MS: u64 = 5;
const KEY_HOLD_MS: u64 = 20;

#[event(channel_size = 16, pubs = 1, subs = 1)]
#[derive(Clone, Copy, Debug)]
pub struct TouchSampleEvent {
    pub timestamp_ms: u64,
    pub touched: u8,
}

#[input_device(publish = TouchSampleEvent)]
pub struct Xw06a {
    inputs: [Input<'static>; 6],
    initialized: bool,
}

impl Xw06a {
    pub fn new(inputs: [Input<'static>; 6]) -> Self {
        Self {
            inputs,
            initialized: false,
        }
    }

    async fn read_touch_sample_event(&mut self) -> TouchSampleEvent {
        if !self.initialized {
            // Datasheet initialization is approximately 400 ms. Keep fingers off at power-on.
            Timer::after_millis(500).await;
            self.initialized = true;
        }
        Timer::after_millis(SAMPLE_MS).await;
        let touched = self
            .inputs
            .iter()
            .enumerate()
            .fold(0, |mask, (index, pin)| {
                mask | ((pin.is_low() as u8) << index)
            });
        TouchSampleEvent {
            timestamp_ms: Instant::now().as_millis(),
            touched,
        }
    }
}

#[processor(subscribe = [TouchSampleEvent])]
pub struct TouchStripProcessor {
    recognizer: Recognizer,
    held: Option<(Gesture, u64)>,
}

impl TouchStripProcessor {
    pub fn new() -> Self {
        Self {
            recognizer: Recognizer::new(Config::default()),
            held: None,
        }
    }

    async fn on_touch_sample_event(&mut self, event: TouchSampleEvent) {
        if self
            .held
            .is_some_and(|(_, deadline)| event.timestamp_ms >= deadline)
        {
            self.release().await;
        }
        if let Some(gesture) = self.recognizer.sample(event.timestamp_ms, event.touched) {
            self.release().await;
            let (row, col) = gesture.key_position();
            publish_event_async(KeyboardEvent::key(row, col, true)).await;
            self.held = Some((gesture, event.timestamp_ms + KEY_HOLD_MS));
            match gesture {
                Gesture::SwipeUp => defmt::debug!("Touch strip: up"),
                Gesture::SwipeDown => defmt::debug!("Touch strip: down"),
                Gesture::DoubleTap => defmt::debug!("Touch strip: double tap"),
            }
        }
    }

    async fn release(&mut self) {
        if let Some((gesture, _)) = self.held.take() {
            let (row, col) = gesture.key_position();
            publish_event_async(KeyboardEvent::key(row, col, false)).await;
        }
    }
}
