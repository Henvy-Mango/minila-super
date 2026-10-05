use super::{Config, Gesture, Recognizer};

struct Trace {
    recognizer: Recognizer,
    now: u64,
    events: Vec<(u64, Gesture)>,
}

impl Trace {
    fn new() -> Self {
        let mut trace = Self {
            recognizer: Recognizer::new(Config::default()),
            now: 0,
            events: Vec::new(),
        };
        trace.hold(0, 60);
        trace
    }

    fn hold(&mut self, mask: u8, duration: u64) {
        for _ in 0..duration / 5 {
            self.now += 5;
            if let Some(gesture) = self.recognizer.sample(self.now, mask) {
                self.events.push((self.now, gesture));
            }
        }
    }

    fn tap(&mut self, mask: u8) {
        self.hold(mask, 70);
        self.hold(0, 80);
    }

    fn gestures(&self) -> Vec<Gesture> {
        self.events.iter().map(|&(_, g)| g).collect()
    }
}

#[test]
fn swipe_up_and_down_with_adjacent_overlap() {
    let mut t = Trace::new();
    for mask in [1, 3, 2, 6, 4, 12, 8, 24, 16, 48, 32] {
        t.hold(mask, 50);
    }
    t.hold(0, 100);
    assert!(!t.events.is_empty());
    assert!(t.gestures().iter().all(|g| *g == Gesture::SwipeUp));
    t.events.clear();
    for mask in [32, 48, 16, 24, 8, 12, 4, 6, 2, 3, 1] {
        t.hold(mask, 50);
    }
    t.hold(0, 100);
    assert!(!t.events.is_empty());
    assert!(t.gestures().iter().all(|g| *g == Gesture::SwipeDown));
}

#[test]
fn fast_swipe_can_skip_an_electrode() {
    let mut t = Trace::new();
    t.hold(1, 20);
    t.hold(8, 20);
    t.hold(0, 100);
    assert_eq!(t.gestures(), [Gesture::SwipeUp]);
}

#[test]
fn single_tap_is_silent_and_double_tap_fires_on_second_release() {
    let mut t = Trace::new();
    t.tap(4);
    assert!(t.events.is_empty());
    t.hold(4, 70);
    assert!(t.events.is_empty());
    t.hold(0, 80);
    assert_eq!(t.gestures(), [Gesture::DoubleTap]);
    t.hold(0, 500);
    assert_eq!(t.events.len(), 1);
}

#[test]
fn overlapping_pads_can_be_double_tapped() {
    let mut t = Trace::new();
    t.tap(0b001100);
    t.tap(0b001000);
    assert_eq!(t.gestures(), [Gesture::DoubleTap]);
}

#[test]
fn slow_or_distant_taps_do_not_form_a_double_tap() {
    let mut t = Trace::new();
    t.tap(1);
    t.hold(0, 350);
    t.tap(1);
    t.tap(32);
    assert!(t.events.is_empty());
}

#[test]
fn second_tap_may_end_after_window_if_it_started_in_time() {
    let mut t = Trace::new();
    t.tap(4);
    t.hold(0, 180);
    t.hold(4, 150);
    t.hold(0, 80);
    assert_eq!(t.gestures(), [Gesture::DoubleTap]);
}

#[test]
fn long_hold_and_noise_are_not_taps() {
    let mut t = Trace::new();
    for _ in 0..10 {
        t.hold(1, 5);
        t.hold(0, 5);
    }
    t.hold(0, 80);
    t.hold(1, 500);
    t.hold(0, 80);
    t.tap(1);
    assert!(t.events.is_empty());
}

#[test]
fn short_release_gaps_do_not_create_two_taps() {
    let mut t = Trace::new();
    t.hold(4, 60);
    t.hold(0, 20);
    t.hold(4, 60);
    t.hold(0, 100);
    assert!(t.events.is_empty());
}

#[test]
fn crossing_a_small_dead_zone_preserves_swipe() {
    let mut t = Trace::new();
    t.hold(1, 40);
    t.hold(0, 20);
    t.hold(4, 40);
    assert_eq!(t.gestures(), [Gesture::SwipeUp]);
}

#[test]
fn volume_is_rate_limited_and_stationary_finger_does_not_repeat() {
    let mut t = Trace::new();
    for mask in [1, 4, 8, 16, 32] {
        t.hold(mask, 20);
    }
    t.hold(32, 200);
    assert_eq!(t.events.len(), 2);
    assert!(t.events[1].0 - t.events[0].0 >= 80);
    t.hold(32, 1000);
    assert_eq!(t.events.len(), 2);
}

#[test]
fn reversing_direction_produces_down_without_tap() {
    let mut t = Trace::new();
    for mask in [1, 4, 2] {
        t.hold(mask, 100);
    }
    t.hold(0, 80);
    t.tap(2);
    assert_eq!(t.gestures(), [Gesture::SwipeUp, Gesture::SwipeDown]);
}

#[test]
fn swipe_cancels_pending_double_tap() {
    let mut t = Trace::new();
    t.tap(1);
    t.hold(1, 40);
    t.hold(4, 40);
    t.hold(0, 80);
    t.tap(1);
    assert_eq!(t.gestures(), [Gesture::SwipeUp]);
}

#[test]
fn movement_out_and_back_is_not_a_tap() {
    let mut t = Trace::new();
    for mask in [1, 6, 1] {
        t.hold(mask, 40);
    }
    t.hold(0, 80);
    t.tap(1);
    assert!(t.events.is_empty());
}

#[test]
fn invalid_multitouch_blocks_until_full_release() {
    for mask in [0b100001, 0b000111, 0b111111] {
        let mut t = Trace::new();
        t.tap(1);
        t.hold(mask, 40);
        t.hold(32, 80);
        t.hold(0, 80);
        t.tap(1);
        assert!(t.events.is_empty());
        t.tap(1);
        assert_eq!(t.gestures(), [Gesture::DoubleTap]);
    }
}

#[test]
fn touch_at_startup_requires_release_before_arming() {
    let mut t = Trace::new();
    t.recognizer = Recognizer::new(Config::default());
    t.hold(1, 100);
    t.hold(32, 100);
    assert!(t.events.is_empty());
    t.hold(0, 100);
    t.tap(4);
    t.tap(4);
    assert_eq!(t.gestures(), [Gesture::DoubleTap]);
}

#[test]
fn three_taps_produce_only_one_double_tap() {
    let mut t = Trace::new();
    for _ in 0..3 {
        t.tap(4);
    }
    assert_eq!(t.gestures(), [Gesture::DoubleTap]);
}
