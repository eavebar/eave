//! One thing on screen at a time, handed over to the next: the old one fades out (and slides
//! away, if given a way), then the new one fades in from the other side. iced has no group
//! opacity, so the fades are the island's: covering with its own color. That's why one goes all
//! the way out before the other starts, instead of the two crossing. Changing back mid-way turns
//! around from what's on screen, so nothing jumps. The alternate's options and the tabs' pages
//! hand over on it.

use std::time::{Duration, Instant};

use iced::Vector;

#[derive(Debug)]
pub struct Handover {
    /// How long a whole handover takes, out and in.
    over: Duration,
    pub current: Option<usize>,
    /// The one going out, while it does.
    previous: Option<usize>,
    /// The way the old one slides out: a unit step, or zero for a fade in place.
    pub slide: Vector,
    /// Where it starts, from 0 to 1: past 0 when it turned around.
    from: f32,
    /// Changed, and waiting for a frame to start on.
    pending: bool,
    start: Option<Instant>,
    elapsed: Duration,
}

impl Handover {
    pub fn new(current: Option<usize>, over: Duration) -> Self {
        Self {
            over,
            current,
            previous: None,
            slide: Vector::ZERO,
            from: 0.0,
            pending: false,
            start: None,
            elapsed: Duration::ZERO,
        }
    }

    /// Takes in which one should be on screen, and the way the old one should leave if it
    /// changed.
    pub fn set(&mut self, current: usize, toward: Vector) {
        let Some(was) = self.current.filter(|&was| was != current) else {
            self.current = Some(current);
            return;
        };
        self.current = Some(current);
        if !self.running() {
            self.previous = Some(was);
            self.slide = toward;
            self.from = 0.0;
            self.pending = true;
            self.start = None;
            return;
        }
        let t = self.progress();
        let on_screen = self.shown(was).0;
        if t < 0.5 && on_screen != current {
            // Still on its way out: only what comes in next changes.
        } else if t <= 0.0 {
            // Back before anything moved.
            self.previous = None;
            self.pending = false;
            self.start = None;
        } else {
            // Turn around: what's on screen goes back the way it came, from where it is.
            self.previous = Some(if on_screen == current { was } else { on_screen });
            self.slide = -self.slide;
            self.from = 1.0 - t;
            self.pending = true;
            self.start = None;
        }
    }

    /// Whether a handover is on its way.
    pub fn running(&self) -> bool {
        self.previous.is_some() && (self.pending || self.start.is_some())
    }

    /// The one going out, while it does.
    pub fn leaving(&self) -> Option<usize> {
        self.previous.filter(|_| self.running())
    }

    /// How far it has come, from 0 to 1.
    fn progress(&self) -> f32 {
        let run = if self.pending {
            0.0
        } else {
            self.elapsed.as_secs_f32() / self.over.as_secs_f32()
        };
        (self.from + run).clamp(0.0, 1.0)
    }

    /// Which one is on screen, how much it shows, and how far it has slid, as a fraction of the
    /// whole slide: the old one in the first half, speeding away, the new one in the second,
    /// slowing into place.
    pub fn shown(&self, current: usize) -> (usize, f32, Vector) {
        match self.leaving() {
            Some(previous) => {
                let t = self.progress();
                if t < 0.5 {
                    let out = 2.0 * t;
                    (previous, 1.0 - out, self.slide * (out * out))
                } else {
                    let rest = 2.0 - 2.0 * t;
                    (current, 1.0 - rest, -self.slide * (rest * rest))
                }
            }
            None => (current, 1.0, Vector::ZERO),
        }
    }

    /// Advances to `now`. Returns whether another frame is needed.
    pub fn step(&mut self, now: Instant) -> bool {
        if self.pending {
            self.pending = false;
            self.start = Some(now);
        }
        let Some(start) = self.start else {
            return false;
        };
        self.elapsed = now.saturating_duration_since(start);
        if self.progress() >= 1.0 {
            self.start = None;
            self.previous = None;
            self.from = 0.0;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVER: Duration = Duration::from_millis(240);
    const RIGHT: Vector = Vector::new(1.0, 0.0);

    fn showing(handover: &Handover, current: usize) -> (usize, f32) {
        let (index, shown, _) = handover.shown(current);
        (index, shown)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn fades_out_then_in() {
        let mut handover = Handover::new(Some(0), OVER);
        handover.set(1, Vector::ZERO);
        assert_eq!(
            showing(&handover, 1),
            (0, 1.0),
            "the old one, until a frame comes"
        );
        let start = Instant::now();
        assert!(handover.step(start));
        let quarter = OVER / 4;
        handover.step(start + quarter);
        let (index, shown, slide) = handover.shown(1);
        assert_eq!(index, 0);
        assert!(close(shown, 0.5), "{shown}");
        assert_eq!(slide, Vector::ZERO, "no way given, no slide");
        handover.step(start + quarter * 3);
        let (index, shown) = showing(&handover, 1);
        assert_eq!(index, 1);
        assert!(close(shown, 0.5), "{shown}");
        assert!(!handover.step(start + OVER));
        assert_eq!(showing(&handover, 1), (1, 1.0));
        assert_eq!(handover.leaving(), None);
    }

    #[test]
    fn slides_out_the_way_given_and_in_from_the_other_side() {
        let mut handover = Handover::new(Some(0), OVER);
        handover.set(1, RIGHT);
        let start = Instant::now();
        handover.step(start);
        handover.step(start + OVER / 4);
        let (_, _, out) = handover.shown(1);
        assert!(
            out.x > 0.0 && out.y == 0.0,
            "the old one leaves to the right: {out:?}"
        );
        handover.step(start + OVER * 3 / 4);
        let (_, _, coming) = handover.shown(1);
        assert!(
            coming.x < 0.0,
            "the new one comes from the left: {coming:?}"
        );
    }

    #[test]
    fn going_back_before_the_old_one_is_gone_keeps_it() {
        let mut handover = Handover::new(Some(0), OVER);
        handover.set(1, Vector::ZERO);
        handover.step(Instant::now());
        handover.set(0, Vector::ZERO);
        assert!(!handover.running());
        assert_eq!(showing(&handover, 0), (0, 1.0));
    }

    /// Changing back mid-way carries on from what's on screen, in either half.
    #[test]
    fn turning_around_never_jumps() {
        for fraction in [4, 2] {
            let mut handover = Handover::new(Some(0), OVER);
            handover.set(1, RIGHT);
            let start = Instant::now();
            handover.step(start);
            let now = start + OVER * 3 / (fraction * 2);
            handover.step(now);
            let before = handover.shown(1);
            handover.set(0, RIGHT);
            let after = handover.shown(0);
            assert_eq!(before.0, after.0, "the same one on screen");
            assert!(close(before.1, after.1), "{before:?} {after:?}");
            assert!(close(before.2.x, after.2.x), "{before:?} {after:?}");
            handover.step(now);
            assert!(
                close(handover.shown(0).1, after.1),
                "the next frame starts there"
            );
            assert!(!handover.step(now + OVER));
            assert_eq!(showing(&handover, 0), (0, 1.0));
        }
    }

    #[test]
    fn the_same_one_again_changes_nothing() {
        let mut handover = Handover::new(Some(1), OVER);
        handover.set(1, RIGHT);
        assert!(!handover.running());
    }
}
