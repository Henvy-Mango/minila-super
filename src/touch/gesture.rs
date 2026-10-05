//! Pure state machine, independent of MCU, I2C and RMK.

// Derived from STAR/SEVEN/FOUR/ONE etc. in the supplied .pde example.
// Columns share vertical row positions for swipes; taps use neighboring row/column regions.
pub const KEYPAD_PATHS: &[&[u8]] = &[&[0, 1, 2, 3], &[4, 5, 6, 7], &[8, 9, 10, 11]];
const DEBOUNCE_MS: u64 = 10;
const RELEASE_GRACE_MS: u64 = 50;
/// Maximum time from the first release to the second press (not press-to-press).
pub const DOUBLE_TAP_INTERVAL_MS: u64 = 300;
pub const LONG_PRESS_MS: u64 = 600;
pub const LONG_PRESS_REPEAT_INTERVAL_MS: u64 = 350;
pub const SWIPE_HOLD_DELAY_MS: u64 = 600;
pub const SWIPE_REPEAT_INTERVAL_MS: u64 = 350;
// Swap swipe direction and its hold-repeat direction;
pub const SWIPE_REVERSE_DIRECTION: bool = true;
// One-shot software timers after a zero-touch sample: debounce/re-arm,
// release confirmation, then tap expiry. They do not request an I2C read.
pub const IDLE_TIMER_OFFSETS_MS: [u64; 3] =
    [DEBOUNCE_MS, RELEASE_GRACE_MS, DOUBLE_TAP_INTERVAL_MS + 1];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    SlideUp,
    SlideDown,
    RepeatSlideUp,
    RepeatSlideDown,
    DoubleTap,
    LongPress { mask: u16 },
    RepeatLongPress { mask: u16 },
}

impl Gesture {
    pub const fn key(self) -> Option<(u8, u8)> {
        match self {
            // trigger some keys on matrix index
            Self::DoubleTap => Some((4, 4)),
            Self::SlideUp | Self::RepeatSlideUp => Some((2, 12)),
            Self::SlideDown | Self::RepeatSlideDown => Some((3, 11)),
            Self::LongPress { .. } | Self::RepeatLongPress { .. } => None,
        }
    }
}

struct Contact {
    tap_mask: u16,
    long_position: Option<u8>,
    start_ms: u64,
    min: u8,
    max: u8,
    anchor: u8,
    swiping: bool,
    long_pressed: bool,
    last_volume: u64,
    direction: Option<Gesture>,
    repeat_at: Option<u64>,
}

pub struct Recognizer {
    paths: &'static [&'static [u8]],
    candidate: Option<(u16, u64)>,
    armed: bool,
    blocked: bool,
    contact: Option<Contact>,
    release_ms: Option<u64>,
    tap: Option<(u64, u16)>,
}

impl Recognizer {
    pub fn new(paths: &'static [&'static [u8]]) -> Self {
        assert!(!paths.is_empty());
        let mut used = 0u16;
        for path in paths {
            assert!(path.len() >= 3 && path.len() <= 12);
            for &pad in *path {
                assert!(pad < 12 && used & (1 << pad) == 0);
                used |= 1 << pad;
            }
        }
        Self {
            paths,
            candidate: None,
            armed: false,
            blocked: false,
            contact: None,
            release_ms: None,
            tap: None,
        }
    }

    /// On a bus/sensor fault, discard pending taps and require release to re-arm.
    pub fn reset(&mut self) {
        *self = Self::new(self.paths);
    }

    pub fn idle_timer(&mut self, now: u64) -> Option<Gesture> {
        if self.candidate.is_some_and(|(mask, _)| mask == 0) {
            self.sample(now, 0)
        } else {
            None
        }
    }

    pub fn sample(&mut self, now: u64, touched: u16) -> Option<Gesture> {
        let mask = touched & 0x0fff;
        if self.candidate.is_none_or(|(m, _)| m != mask) {
            self.candidate = Some((mask, now));
            // Stop repeating on the first raw release/movement, before debounce
            // or the swipe gap grace. A resumed contact must dwell again.
            if let Some(c) = self.contact.as_mut() {
                c.repeat_at = None;
            }
        }
        let changed_ms = self.candidate.unwrap().1;
        if now.saturating_sub(changed_ms) < DEBOUNCE_MS {
            return None;
        }
        if !self.armed {
            self.armed = mask == 0;
            return None;
        }
        if mask == 0 {
            // Count the grace period from the observed edge, not from debounce
            // confirmation. Debounce and polling must not extend the gap.
            let released = *self.release_ms.get_or_insert(changed_ms);
            if now.saturating_sub(released) < RELEASE_GRACE_MS {
                return None;
            }
            return self.finish_release(now, released);
        }
        // A new touch may arrive before an idle poll confirms the full gap.
        // Resolve the old contact using the actual gap before starting the new
        // one; short gaps still preserve a swipe across electrode dead zones.
        let completed = self.release_ms.take().and_then(|released| {
            if changed_ms.saturating_sub(released) >= RELEASE_GRACE_MS {
                self.finish_release(changed_ms, released)
            } else {
                None
            }
        });
        if self.blocked {
            return completed;
        }
        let Some(pos) = self.position(mask) else {
            self.blocked = true;
            self.contact = None;
            self.tap = None;
            return completed;
        };
        let c = self.contact.get_or_insert(Contact {
            tap_mask: mask,
            long_position: None,
            start_ms: changed_ms,
            min: pos,
            max: pos,
            anchor: pos,
            swiping: false,
            long_pressed: false,
            last_volume: 0,
            direction: None,
            repeat_at: None,
        });
        c.min = c.min.min(pos);
        c.max = c.max.max(pos);
        // Keep the entire debounced contact footprint: moving away and back
        // must not turn a wide horizontal sweep into a tap.
        c.tap_mask |= mask;
        // Long presses accept the same row groups as swipes. Once consumed,
        // confirmed vertical movement stops repeats until full release.
        if c.long_pressed {
            if c.long_position != Some(pos) {
                c.long_position = None;
            }
            if c.long_position.is_some() {
                // A raw interruption clears repeat_at; dwell again before
                // resuming. Confirmed movement ends repeats until full release.
                let deadline = *c.repeat_at.get_or_insert(changed_ms + LONG_PRESS_MS);
                if now >= deadline {
                    c.repeat_at = Some(now + LONG_PRESS_REPEAT_INTERVAL_MS);
                    return Some(Gesture::RepeatLongPress { mask });
                }
            }
            return completed;
        }
        // Start the dwell at the latest raw mask change, so a finger can settle
        // onto overlapping pads without losing long-press eligibility forever.
        if !c.swiping && now.saturating_sub(changed_ms) >= LONG_PRESS_MS {
            c.long_pressed = true;
            c.long_position = Some(pos);
            c.repeat_at = Some(now + LONG_PRESS_REPEAT_INTERVAL_MS);
            self.tap = None;
            return Some(Gesture::LongPress { mask });
        }
        if c.max - c.min > 2 {
            self.tap = None;
        }
        // Half-row units: one row of vertical travel produces a volume step.
        if pos.abs_diff(c.anchor) < 2 {
            if c.swiping && pos == c.anchor {
                let deadline = *c.repeat_at.get_or_insert(changed_ms + SWIPE_HOLD_DELAY_MS);
                if now >= deadline {
                    // Emit at most one step per sample, never catch up with a
                    // burst after a scheduling delay.
                    c.repeat_at = Some(now + SWIPE_REPEAT_INTERVAL_MS);
                    return match c.direction {
                        Some(Gesture::SlideUp) => Some(Gesture::RepeatSlideUp),
                        Some(Gesture::SlideDown) => Some(Gesture::RepeatSlideDown),
                        _ => None,
                    };
                }
            }
            return completed;
        }
        let gesture = if (pos > c.anchor) != SWIPE_REVERSE_DIRECTION {
            Gesture::SlideUp
        } else {
            Gesture::SlideDown
        };
        if c.swiping && c.direction == Some(gesture) && now.saturating_sub(c.last_volume) < 80 {
            return completed;
        }
        c.swiping = true;
        c.anchor = pos;
        c.last_volume = now;
        c.direction = Some(gesture);
        c.repeat_at = Some(now + SWIPE_HOLD_DELAY_MS);
        self.tap = None;
        Some(gesture)
    }

    fn finish_release(&mut self, now: u64, released: u64) -> Option<Gesture> {
        self.blocked = false;
        if let Some(c) = self.contact.take() {
            let duration = released.saturating_sub(c.start_ms);
            if self.adjacent_region(c.tap_mask)
                && !c.swiping
                && !c.long_pressed
                && c.max - c.min <= 2
                && (30..=250).contains(&duration)
            {
                if let Some((first_release, first_mask)) = self.tap.take() {
                    if c.start_ms.saturating_sub(first_release) <= DOUBLE_TAP_INTERVAL_MS
                        && self.adjacent_region(first_mask | c.tap_mask)
                    {
                        return Some(Gesture::DoubleTap);
                    }
                }
                self.tap = Some((released, c.tap_mask));
            } else {
                self.tap = None;
            }
        }
        if self
            .tap
            .is_some_and(|(t, _)| now.saturating_sub(t) > DOUBLE_TAP_INTERVAL_MS)
        {
            self.tap = None;
        }
        None
    }

    fn position(&self, mask: u16) -> Option<u8> {
        let mut remaining = mask;
        let mut first = u8::MAX;
        let mut last = 0;
        for path in self.paths {
            for (row, pad) in path.iter().enumerate() {
                if mask & (1 << pad) != 0 {
                    remaining &= !(1 << pad);
                    first = first.min(row as u8);
                    last = last.max(row as u8);
                }
            }
        }
        // Merge columns without weighting rows by how many pads are touched.
        // Same-row overlap stays still; adjacent rows use their midpoint.
        (remaining == 0 && first != u8::MAX && last - first <= 1).then(|| first + last)
    }

    // Both clicks must fit in one 2x2 neighborhood, including diagonal corners.
    // Use physical coordinates from KEYPAD_PATHS, not numeric electrode distance.
    fn adjacent_region(&self, mask: u16) -> bool {
        let mut remaining = mask;
        let (mut min_row, mut min_col) = (usize::MAX, usize::MAX);
        let (mut max_row, mut max_col) = (0, 0);
        for (col, path) in self.paths.iter().enumerate() {
            for (row, pad) in path.iter().enumerate() {
                if mask & (1 << pad) != 0 {
                    remaining &= !(1 << pad);
                    min_row = min_row.min(row);
                    max_row = max_row.max(row);
                    min_col = min_col.min(col);
                    max_col = max_col.max(col);
                }
            }
        }
        mask != 0 && remaining == 0 && max_row - min_row <= 1 && max_col - min_col <= 1
    }
}
