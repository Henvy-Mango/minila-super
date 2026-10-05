//! Startup qualification only: this cannot identify a stable finger or foreign
//! object that has already been absorbed into the sensor's baseline.

pub const STABLE_MS: u64 = 1000;
pub const CHECK_INTERVAL_MS: u64 = 20;
pub const MAX_SAMPLE_GAP_MS: u64 = 100;
pub const MAX_FILTERED_SPAN: u16 = 8;
pub const MAX_BASELINE_SPAN: u16 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitReason {
    Settling,
    Touch { mask: u16 },
    SignalAboveRelease { pad: u8, delta: i32 },
    FilteredUnstable { pad: u8, span: u16 },
    BaselineUnstable { pad: u8, span: u16 },
    SampleGap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qualification {
    Waiting(WaitReason),
    Ready,
}

struct Window {
    since: u64,
    last: u64,
    filtered_min: [u16; 12],
    filtered_max: [u16; 12],
    baseline_min: [u16; 12],
    baseline_max: [u16; 12],
}

impl Window {
    fn new(now: u64, filtered: [u16; 12], baseline: [u16; 12]) -> Self {
        Self {
            since: now,
            last: now,
            filtered_min: filtered,
            filtered_max: filtered,
            baseline_min: baseline,
            baseline_max: baseline,
        }
    }
}

pub struct EnvironmentGate {
    release_threshold: u16,
    window: Option<Window>,
}

impl EnvironmentGate {
    pub fn new(release_threshold: u16) -> Self {
        assert!(release_threshold > 0);
        Self {
            release_threshold,
            window: None,
        }
    }

    pub fn reset(&mut self) {
        self.window = None;
    }

    pub fn stable_for(&self, now: u64) -> u64 {
        self.window
            .as_ref()
            .map_or(0, |w| now.saturating_sub(w.since))
    }

    // Call only with successful, fault-free samples. Faults must reset the gate.
    // Min/max across the whole window also catches slow cumulative drift.
    pub fn observe(
        &mut self,
        now: u64,
        touched: u16,
        filtered: [u16; 12],
        baseline: [u16; 12],
    ) -> Qualification {
        if touched != 0 {
            self.reset();
            return Qualification::Waiting(WaitReason::Touch { mask: touched });
        }
        for pad in 0..12 {
            let delta = i32::from(baseline[pad]) - i32::from(filtered[pad]);
            if delta >= i32::from(self.release_threshold) {
                self.reset();
                return Qualification::Waiting(WaitReason::SignalAboveRelease {
                    pad: pad as u8,
                    delta,
                });
            }
        }
        let Some(w) = self.window.as_mut() else {
            self.window = Some(Window::new(now, filtered, baseline));
            return Qualification::Waiting(WaitReason::Settling);
        };
        let mut reason = None;
        if now < w.last || now - w.last > MAX_SAMPLE_GAP_MS {
            reason = Some(WaitReason::SampleGap);
        } else {
            for pad in 0..12 {
                w.filtered_min[pad] = w.filtered_min[pad].min(filtered[pad]);
                w.filtered_max[pad] = w.filtered_max[pad].max(filtered[pad]);
                w.baseline_min[pad] = w.baseline_min[pad].min(baseline[pad]);
                w.baseline_max[pad] = w.baseline_max[pad].max(baseline[pad]);
                let span = w.filtered_max[pad] - w.filtered_min[pad];
                if span > MAX_FILTERED_SPAN {
                    reason = Some(WaitReason::FilteredUnstable {
                        pad: pad as u8,
                        span,
                    });
                    break;
                }
                let span = w.baseline_max[pad] - w.baseline_min[pad];
                if span > MAX_BASELINE_SPAN {
                    reason = Some(WaitReason::BaselineUnstable {
                        pad: pad as u8,
                        span,
                    });
                    break;
                }
            }
        }
        if let Some(reason) = reason {
            self.window = Some(Window::new(now, filtered, baseline));
            return Qualification::Waiting(reason);
        }
        w.last = now;
        if now - w.since >= STABLE_MS {
            Qualification::Ready
        } else {
            Qualification::Waiting(WaitReason::Settling)
        }
    }
}
