//! Springs: how shapes and fades move toward where they're going (docs/design.md, Motion). Each
//! feel pairs with a kind of motion there; their numbers live in `theme` (D-83).

use std::f32::consts::TAU;
use std::time::Duration;

use crate::theme;

/// How a spring moves: how hard it pulls toward its target, and its damping ratio (below 1 it
/// overshoots a little before settling). Made with [`Feel::new`], from how long it takes to swing
/// once, so the feels in `theme` read as tempos rather than stiffnesses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Feel {
    pub stiffness: f32,
    pub damping: f32,
}

impl Feel {
    /// A spring that would swing once every `response` seconds if nothing damped it, damped by
    /// `damping` (1 is critical).
    pub const fn new(response: f32, damping: f32) -> Self {
        let omega = TAU / response;
        Self {
            stiffness: omega * omega,
            damping,
        }
    }

    #[cfg(test)]
    fn omega(self) -> f32 {
        self.stiffness.sqrt()
    }

    /// How fast to set a spring at rest off so it goes out `peak` at its furthest, and comes back.
    /// Motions say how far they go; this says how hard that is to kick, on this feel. Found by
    /// stepping a kick the way [`Spring::advance`] does, not from the exact solution, which the
    /// steps fall a few percent short of, more the more damped.
    pub fn kick(self, peak: f32) -> f32 {
        peak / self.turn().0
    }

    /// How long a kick takes to go out as far as it goes, before it turns back.
    pub fn peak_time(self) -> f32 {
        self.turn().1
    }

    /// Where a unit kick turns back, and when.
    fn turn(self) -> (f32, f32) {
        let (mut spring, mut time) = (self.kicked(), 0.0);
        while spring.velocity > 0.0 {
            spring.advance(SUBSTEP);
            time += SUBSTEP;
        }
        (spring.value, time)
    }

    /// How far past rest it swings back after a kick, as a share of how far it went out: 0 when
    /// it's damped to critical or more.
    #[cfg(test)]
    pub fn overshoot(self) -> f32 {
        let (mut spring, mut out, mut back) = (self.kicked(), 0.0f32, 0.0f32);
        for _ in 0..(5.0 / SUBSTEP) as usize {
            spring.advance(SUBSTEP);
            out = out.max(spring.value);
            back = back.min(spring.value);
        }
        -back / out
    }

    /// About how long a kick takes to play: until it stays within 5 % of how far it went out.
    pub fn settle_time(self) -> Duration {
        let (mut spring, mut out) = (self.kicked(), 0.0f32);
        let (mut time, mut last_out) = (0.0, 0.0);
        while time < 5.0 {
            spring.advance(SUBSTEP);
            time += SUBSTEP;
            out = out.max(spring.value.abs());
            if spring.value.abs() >= 0.05 * out {
                last_out = time;
            }
        }
        Duration::from_secs_f32(last_out)
    }

    /// At rest, just kicked outward at 1.
    fn kicked(self) -> Spring {
        Spring {
            velocity: 1.0,
            ..Spring::new(0.0, self)
        }
    }
}

/// What the hand moves: a press, and a drag. Letting go overshoots a little.
pub(crate) const HAND: Feel = Feel::new(theme::HAND_RESPONSE, theme::HAND_DAMPING);
/// What an event kicks, with nothing to go to: a nudge's push, an item's kick, the jelly's pokes,
/// a popover's squish and splat. All of them out and back on one feel, so they read as one thing.
pub(crate) const REACT: Feel = Feel::new(theme::REACT_RESPONSE, theme::REACT_DAMPING);
/// Shapes and content going somewhere: settle without wobbling.
pub(crate) const MORPH: Feel = Feel::new(theme::MORPH_RESPONSE, theme::MORPH_DAMPING);
/// The handover between an island's items and its popover's content: quicker than the shape, so
/// one side is gone in time for the other to come in while the shape still moves.
pub(crate) const FADE: Feel = Feel::new(theme::FADE_RESPONSE, theme::FADE_DAMPING);

/// The longest step a single frame may take.
pub(crate) const MAX_FRAME: Duration = Duration::from_millis(50);
/// Springs are integrated in steps no longer than this, so they stay stable at any frame rate.
pub(crate) const SUBSTEP: f32 = 1.0 / 480.0;
/// Close enough to the target, and slow enough, to stop: well under a physical pixel, or a
/// hundredth of a fade.
pub(crate) const REST_DISTANCE: f32 = 0.01;
pub(crate) const REST_SPEED: f32 = 0.5;

/// A damped spring along one axis.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub feel: Feel,
}

impl Spring {
    /// At rest at `value`.
    pub fn new(value: f32, feel: Feel) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            feel,
        }
    }

    pub fn advance(&mut self, seconds: f32) {
        let Feel { stiffness, damping } = self.feel;
        let damping = 2.0 * damping * stiffness.sqrt();
        let mut left = seconds;
        while left > 0.0 {
            let dt = left.min(SUBSTEP);
            let force = -stiffness * (self.value - self.target) - damping * self.velocity;
            self.velocity += force * dt;
            self.value += self.velocity * dt;
            left -= dt;
        }
    }

    pub fn at_rest(&self) -> bool {
        (self.value - self.target).abs() < REST_DISTANCE && self.velocity.abs() < REST_SPEED
    }

    pub fn settle(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far a spring at rest goes out when kicked at `speed`, and how far back past rest.
    fn swing(feel: Feel, speed: f32) -> (f32, f32) {
        let mut spring = Spring {
            velocity: speed,
            ..Spring::new(0.0, feel)
        };
        let (mut out, mut back): (f32, f32) = (0.0, 0.0);
        for _ in 0..5000 {
            spring.advance(SUBSTEP);
            out = out.max(spring.value);
            back = back.min(spring.value);
        }
        (out, -back)
    }

    #[test]
    fn a_feel_swings_at_its_response() {
        let feel = Feel::new(0.25, 0.0);
        assert!((feel.omega() - TAU / 0.25).abs() < 0.01);
        assert!((feel.stiffness - (TAU / 0.25).powi(2)).abs() < 0.1);
    }

    #[test]
    fn a_kick_goes_as_far_as_it_says_and_swings_back_as_predicted() {
        for damping in [0.4, 0.6, 0.85, 1.0, 1.4] {
            let feel = Feel::new(0.3, damping);
            let (out, back) = swing(feel, feel.kick(2.0));
            assert!((out - 2.0).abs() < 0.01, "{damping}: {out}");
            assert!(
                (back - 2.0 * feel.overshoot()).abs() < 0.01,
                "{damping}: {back}"
            );
            // Close to the exact solution's e^(−πζ/√(1−ζ²)).
            let exact = if damping < 1.0 {
                (-std::f32::consts::PI * damping / (1.0 - damping * damping).sqrt()).exp()
            } else {
                0.0
            };
            assert!(
                (feel.overshoot() - exact).abs() < 0.03,
                "{damping}: {}",
                feel.overshoot()
            );
        }
    }

    #[test]
    fn slower_and_bouncier_feels_take_longer() {
        let quick = Feel::new(0.2, 0.9).settle_time();
        let slow = Feel::new(0.4, 0.9).settle_time();
        let bouncy = Feel::new(0.2, 0.4).settle_time();
        assert!(
            quick < slow && quick < bouncy,
            "{quick:?} {slow:?} {bouncy:?}"
        );
        assert!(quick > Duration::from_millis(50), "{quick:?}");
    }
}
