//! Hardware-independent six-electrode gesture recognition. Times are monotonic milliseconds.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    SwipeUp,
    SwipeDown,
    DoubleTap,
}

impl Gesture {
    /// Unused matrix positions reserved for gestures in keyboard.toml.
    pub const fn key_position(self) -> (u8, u8) {
        match self {
            Self::SwipeUp => (2, 12),
            Self::SwipeDown => (3, 11),
            Self::DoubleTap => (4, 4),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub debounce_ms: u64,
    pub release_grace_ms: u64,
    pub tap_min_ms: u64,
    pub tap_max_ms: u64,
    pub double_tap_ms: u64,
    pub volume_interval_ms: u64,
    // Positions use half-electrode units: electrode 0 = 0, electrode 5 = 10.
    pub swipe_start: u8,
    pub swipe_step: u8,
    pub tap_travel: u8,
    pub double_tap_distance: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debounce_ms: 10,
            release_grace_ms: 35,
            tap_min_ms: 30,
            tap_max_ms: 250,
            double_tap_ms: 300,
            volume_interval_ms: 80,
            swipe_start: 4,
            swipe_step: 2,
            tap_travel: 2,
            double_tap_distance: 2,
        }
    }
}

struct Contact {
    started: u64,
    start: u8,
    min: u8,
    max: u8,
    anchor: u8,
    swiping: bool,
    last_volume: u64,
}

struct Tap {
    released: u64,
    position: u8,
}

pub struct Recognizer {
    config: Config,
    candidate: u8,
    candidate_since: u64,
    stable: u8,
    initialized: bool,
    armed: bool,
    blocked: bool,
    contact: Option<Contact>,
    released_since: Option<u64>,
    previous_tap: Option<Tap>,
}

impl Recognizer {
    pub fn new(config: Config) -> Self {
        assert!(config.swipe_start > config.tap_travel);
        assert!(config.swipe_step > 0);
        assert!(config.tap_max_ms >= config.tap_min_ms);
        Self {
            config,
            candidate: 0,
            candidate_since: 0,
            stable: 0,
            initialized: false,
            armed: false,
            blocked: false,
            contact: None,
            released_since: None,
            previous_tap: None,
        }
    }

    /// Call periodically (5 ms in the GPIO reader), including when all pads are released.
    /// Bit 0 is the bottom electrode; a set bit means touched, independent of GPIO polarity.
    pub fn sample(&mut self, now: u64, touched: u8) -> Option<Gesture> {
        let touched = touched & 0x3f;
        if !self.initialized || touched != self.candidate {
            self.initialized = true;
            self.candidate = touched;
            self.candidate_since = now;
        }
        if now.saturating_sub(self.candidate_since) < self.config.debounce_ms {
            return None;
        }
        self.stable = self.candidate;

        // Ignore a finger already present at startup until a clean release.
        if !self.armed {
            self.armed = self.stable == 0;
            return None;
        }

        if self.stable == 0 {
            let released = *self.released_since.get_or_insert(now);
            if now.saturating_sub(released) < self.config.release_grace_ms {
                return None;
            }
            self.blocked = false;
            if let Some(contact) = self.contact.take() {
                let duration = released.saturating_sub(contact.started);
                let is_tap = !contact.swiping
                    && contact.max - contact.min <= self.config.tap_travel
                    && (self.config.tap_min_ms..=self.config.tap_max_ms).contains(&duration);
                if is_tap {
                    if let Some(first) = self.previous_tap.take() {
                        if contact.started.saturating_sub(first.released)
                            <= self.config.double_tap_ms
                            && contact.start.abs_diff(first.position)
                                <= self.config.double_tap_distance
                        {
                            return Some(Gesture::DoubleTap);
                        }
                    }
                    self.previous_tap = Some(Tap {
                        released,
                        position: contact.start,
                    });
                } else {
                    self.previous_tap = None;
                }
            }
            if self
                .previous_tap
                .as_ref()
                .is_some_and(|tap| now.saturating_sub(tap.released) > self.config.double_tap_ms)
            {
                self.previous_tap = None;
            }
            return None;
        }

        self.released_since = None;
        if self.blocked {
            return None;
        }
        let Some(position) = position(self.stable) else {
            // Nonadjacent pads or a broad contact are ambiguous. Require a full release.
            self.blocked = true;
            self.contact = None;
            self.previous_tap = None;
            return None;
        };

        let contact = self.contact.get_or_insert(Contact {
            started: now,
            start: position,
            min: position,
            max: position,
            anchor: position,
            swiping: false,
            last_volume: 0,
        });
        contact.min = contact.min.min(position);
        contact.max = contact.max.max(position);
        if contact.max - contact.min > self.config.tap_travel {
            self.previous_tap = None;
        }
        let threshold = if contact.swiping {
            self.config.swipe_step
        } else {
            self.config.swipe_start
        };
        if position.abs_diff(contact.anchor) < threshold {
            return None;
        }
        if contact.swiping
            && now.saturating_sub(contact.last_volume) < self.config.volume_interval_ms
        {
            return None;
        }
        let gesture = if position > contact.anchor {
            Gesture::SwipeUp
        } else {
            Gesture::SwipeDown
        };
        contact.swiping = true;
        contact.anchor = position;
        contact.last_volume = now;
        self.previous_tap = None;
        Some(gesture)
    }
}

/// One pad or two adjacent pads; their midpoint avoids choosing an arbitrary winner.
fn position(mask: u8) -> Option<u8> {
    let first = mask.trailing_zeros() as u8;
    let last = 7 - mask.leading_zeros() as u8;
    (mask.count_ones() <= 2 && last - first <= 1).then_some(first + last)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
