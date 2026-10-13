//! A number that eases to each new value it's given, over a fixed time, fast at first. The
//! toggle's knob and the meter's fill move on it. Like the switcher, it moves on the frames the
//! compositor asks for, and stops asking once it's there.

use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct Ease {
    /// The value on screen, once it has one at all.
    shown: Option<f32>,
    from: f32,
    to: f32,
    /// Changed, and waiting for a frame to start moving on.
    pending: bool,
    start: Option<Instant>,
}

impl Ease {
    /// Takes in the value. The first one shows at once; a new one is eased to from where it is.
    pub fn set(&mut self, value: f32) {
        match self.shown {
            None => {
                self.shown = Some(value);
                self.to = value;
            }
            Some(_) if self.to == value => {}
            Some(shown) => {
                self.from = shown;
                self.to = value;
                self.pending = true;
                self.start = None;
            }
        }
    }

    /// Shows `value` at once, stopping any easing: for a value that follows the hand.
    pub fn snap(&mut self, value: f32) {
        self.shown = Some(value);
        self.to = value;
        self.pending = false;
        self.start = None;
    }

    /// Where it's going, or where it is once it's there.
    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn value(&self) -> f32 {
        self.shown.unwrap_or(self.to)
    }

    /// Advances to `now`, taking `over` from start to end. Returns whether another frame is
    /// needed.
    pub fn step(&mut self, now: Instant, over: Duration) -> bool {
        if self.pending {
            self.pending = false;
            self.start = Some(now);
        }
        let Some(start) = self.start else {
            return false;
        };
        let t = now.saturating_duration_since(start).as_secs_f32() / over.as_secs_f32();
        if t >= 1.0 {
            self.shown = Some(self.to);
            self.start = None;
            return false;
        }
        let eased = 1.0 - (1.0 - t).powi(3);
        self.shown = Some(self.from + (self.to - self.from) * eased);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVER: Duration = Duration::from_millis(100);

    #[test]
    fn first_value_shows_at_once_then_eases() {
        let start = Instant::now();
        let mut ease = Ease::default();
        ease.set(0.0);
        assert!(!ease.step(start, OVER));
        assert_eq!(ease.value(), 0.0);

        ease.set(1.0);
        assert!(ease.step(start, OVER));
        assert_eq!(ease.value(), 0.0);
        assert!(ease.step(start + OVER / 2, OVER));
        let middle = ease.value();
        assert!(
            middle > 0.5 && middle < 1.0,
            "eased, ahead of linear: {middle}"
        );
        assert!(!ease.step(start + OVER, OVER));
        assert_eq!(ease.value(), 1.0);
    }

    #[test]
    fn the_same_value_asks_for_nothing() {
        let mut ease = Ease::default();
        ease.set(0.4);
        ease.set(0.4);
        assert!(!ease.step(Instant::now(), OVER));
    }
}
