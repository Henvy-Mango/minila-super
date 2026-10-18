use super::{
    gesture::{Gesture, Recognizer, KEYPAD_PATHS, PCB_PAD_TO_ELE},
    TouchEvent,
};
use embassy_time::Instant;
use rmk::{
    event::{publish_event_async, KeyboardEvent},
    macros::processor,
};

// Both wrappers receive the raw ELE-indexed touch mask.
struct PcbPads(u16);

impl core::fmt::Debug for PcbPads {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(
                PCB_PAD_TO_ELE
                    .iter()
                    .enumerate()
                    .filter_map(|(pad, &ele)| (self.0 & (1 << ele) != 0).then_some(pad)),
            )
            .finish()
    }
}

struct Electrodes(u16);

impl core::fmt::Debug for Electrodes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries((0u8..12).filter(|ele| self.0 & (1 << ele) != 0))
            .finish()
    }
}

#[cfg(feature = "mpr121")]
#[processor(subscribe = [TouchEvent], poll_interval = 5)]
pub struct TouchProcessor {
    gestures: Recognizer,
    held: Option<((u8, u8), u64)>,
    last_touched: Option<u16>,
    accepting_input: bool,
}

impl TouchProcessor {
    pub fn new() -> Self {
        Self {
            gestures: Recognizer::new(KEYPAD_PATHS),
            held: None,
            last_touched: None,
            accepting_input: false,
        }
    }
    async fn on_touch_event(&mut self, event: TouchEvent) {
        if self.accepting_input != event.ready {
            // Both requalification and faults discard old taps/repeats/held keys.
            self.gestures.reset();
            self.release().await;
            self.accepting_input = event.ready;
        }
        let gesture = match event.sample {
            Some(Ok(sample)) => {
                if self.last_touched != Some(sample.touched) {
                    defmt::info!(
                        "MPR121 ele_mask=0x{:03X}, pcb_pad={}, ele={}, filtered_by_ele={:?}, baseline_by_ele={:?}, delta_by_ele={:?}",
                        sample.touched,
                        defmt::Debug2Format(&PcbPads(sample.touched)),
                        defmt::Debug2Format(&Electrodes(sample.touched)),
                        sample.filtered,
                        sample.baseline,
                        sample.delta()
                    );
                    self.last_touched = Some(sample.touched);
                }
                if event.ready {
                    self.gestures.sample(event.ms, sample.touched)
                } else {
                    None
                }
            }
            Some(Err(fault)) => {
                defmt::warn!(
                    "MPR121 fault={}; retrying initialization",
                    defmt::Debug2Format(&fault)
                );
                self.gestures.reset();
                self.last_touched = None;
                self.release().await;
                None
            }
            None if event.ready => self.gestures.idle_timer(event.ms),
            None => None,
        };
        if let Some(g) = gesture {
            match g {
                Gesture::LongPress { mask } => {
                    defmt::info!(
                        "MPR121 gesture=LongPress pcb_pad={}, ele={}",
                        defmt::Debug2Format(&PcbPads(mask)),
                        defmt::Debug2Format(&Electrodes(mask))
                    );
                }
                Gesture::RepeatLongPress { mask } => {
                    defmt::info!(
                        "MPR121 gesture=RepeatLongPress pcb_pad={}, ele={}",
                        defmt::Debug2Format(&PcbPads(mask)),
                        defmt::Debug2Format(&Electrodes(mask))
                    );
                }
                _ => defmt::info!("MPR121 gesture={}", defmt::Debug2Format(&g)),
            }
            self.release().await;
            if let Some((row, col)) = g.key() {
                publish_event_async(KeyboardEvent::key(row, col, true)).await;
                self.held = Some(((row, col), Instant::now().as_millis() + 20));
            }
        }
    }
    async fn poll(&mut self) {
        if self
            .held
            .is_some_and(|(_, until)| Instant::now().as_millis() >= until)
        {
            self.release().await;
        }
    }
    async fn release(&mut self) {
        if let Some(((row, col), _)) = self.held.take() {
            publish_event_async(KeyboardEvent::key(row, col, false)).await;
        }
    }
}
