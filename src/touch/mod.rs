pub mod driver;
pub mod environment;
pub mod gesture;
#[cfg(feature = "mpr121")]
pub mod processor;

use driver::{Fault, Mpr121, Snapshot, RELEASE_THRESHOLD};
use embassy_rp::{
    gpio::Input,
    i2c::{Async, I2c},
    peripherals::I2C0,
};
use embassy_time::{with_timeout, Delay, Duration, Instant, Timer};
use environment::{EnvironmentGate, Qualification, CHECK_INTERVAL_MS, STABLE_MS};
use gesture::IDLE_TIMER_OFFSETS_MS;
use rmk::embassy_futures::select::{select, Either};
use rmk::macros::{event, input_device};

embassy_rp::bind_interrupts!(pub struct MprIrqs {
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<I2C0>;
});

#[event(channel_size = 8, pubs = 1, subs = 1)]
#[derive(Clone, Copy, Debug)]
pub struct TouchEvent {
    pub ms: u64,
    // Only qualified samples/timers may reach the gesture recognizer.
    pub ready: bool,
    // None is a software timer notification, not a fresh sensor reading.
    pub sample: Option<Result<Snapshot, Fault>>,
}

#[input_device(publish = TouchEvent)]
pub struct MprInput {
    chip: Mpr121<I2c<'static, I2C0, Async>>,
    irq: Input<'static>,
    configured: bool,
    input_ready: bool,
    environment: EnvironmentGate,
    last_check_log: Option<u64>,
    touched: u16,
    idle_since: u64,
    idle_tick: usize,
    next_read: Instant,
}

impl MprInput {
    pub fn new(bus: I2c<'static, I2C0, Async>, irq: Input<'static>) -> Self {
        Self {
            chip: Mpr121::new(bus, driver::ADDRESS),
            irq,
            configured: false,
            input_ready: false,
            environment: EnvironmentGate::new(u16::from(RELEASE_THRESHOLD)),
            last_check_log: None,
            touched: 0,
            idle_since: 0,
            idle_tick: IDLE_TIMER_OFFSETS_MS.len(),
            next_read: Instant::now(),
        }
    }

    async fn read_touch_event(&mut self) -> TouchEvent {
        if self.input_ready && self.touched == 0 {
            if let Some(offset) = IDLE_TIMER_OFFSETS_MS.get(self.idle_tick) {
                let deadline = Instant::from_millis(self.idle_since + offset);
                if let Either::Second(()) =
                    select(self.irq.wait_for_low(), Timer::at(deadline)).await
                {
                    self.idle_tick += 1;
                    return TouchEvent {
                        ms: Instant::now().as_millis(),
                        ready: true,
                        sample: None,
                    };
                }
            } else {
                // Once gesture deadlines have elapsed, idle has no timer wakeups.
                self.irq.wait_for_low().await;
            }
        }
        // Bound active sampling and a stuck-low IRQ without delaying an IRQ
        // arriving after a long idle. Never cancel an in-progress I2C operation.
        Timer::at(self.next_read).await;
        let sample = self.read().await;
        let now = Instant::now();
        match &sample {
            Err(_) => {
                self.configured = false;
                self.input_ready = false;
                self.environment.reset();
                self.last_check_log = None;
                self.touched = 0;
            }
            Ok(snapshot) if !self.input_ready => {
                match self.environment.observe(
                    now.as_millis(),
                    snapshot.touched,
                    snapshot.filtered,
                    snapshot.baseline,
                ) {
                    Qualification::Ready => {
                        self.input_ready = true;
                        defmt::info!(
                            "MPR121 ready: no touch and stable for {} ms; gestures enabled",
                            STABLE_MS
                        );
                    }
                    Qualification::Waiting(reason) => {
                        if self
                            .last_check_log
                            .is_none_or(|t| now.as_millis().saturating_sub(t) >= 1000)
                        {
                            defmt::info!(
                                "MPR121 checking: {}, stable={}/{} ms; gestures disabled",
                                defmt::Debug2Format(&reason),
                                self.environment.stable_for(now.as_millis()),
                                STABLE_MS
                            );
                            self.last_check_log = Some(now.as_millis());
                        }
                    }
                }
            }
            Ok(_) => {}
        }
        self.next_read = now
            + Duration::from_millis(if self.configured && !self.input_ready {
                CHECK_INTERVAL_MS
            } else {
                8
            });
        if self.input_ready && self.touched == 0 {
            self.idle_since = now.as_millis();
            self.idle_tick = 0;
        } else {
            self.idle_tick = IDLE_TIMER_OFFSETS_MS.len();
        }
        TouchEvent {
            ms: now.as_millis(),
            ready: self.input_ready,
            sample: Some(sample),
        }
    }

    async fn read(&mut self) -> Result<Snapshot, Fault> {
        if !self.configured {
            // Back off on a missing/disconnected board, without blocking the keyboard.
            Timer::after_millis(1000).await;
            defmt::info!("MPR121 initializing: addr=0x{:02X}", driver::ADDRESS);
            with_timeout(Duration::from_millis(250), self.chip.init(&mut Delay))
                .await
                .map_err(|_| Fault::Timeout)??;
            Timer::after_millis(100).await;
            self.configured = true;
            defmt::info!("MPR121 configured; checking environment, keep pads untouched");
        }
        let sample = with_timeout(Duration::from_millis(50), self.chip.sample())
            .await
            .map_err(|_| Fault::Timeout)??;
        self.touched = sample.touched;
        Ok(sample)
    }
}
