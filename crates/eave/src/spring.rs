//! Springs: how shapes and fades move toward where they're going (docs/design.md, Motion). Each
//! feel pairs with a motion there; their numbers live in `theme`.

use std::time::Duration;

use crate::theme;

/// How a spring moves: how hard it pulls toward its target, and its damping ratio (below 1 it
/// overshoots a little before settling).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Feel {
    pub stiffness: f32,
    pub damping: f32,
}

/// The press squeeze: springy, so letting go overshoots a little.
pub(crate) const PLAYFUL: Feel = Feel {
    stiffness: theme::SPRING_STIFFNESS,
    damping: theme::SPRING_DAMPING,
};
/// The drag: soft, so the stretch eases after the pointer and back.
pub(crate) const DRAG: Feel = Feel {
    stiffness: theme::DRAG_STIFFNESS,
    damping: theme::DRAG_DAMPING,
};
/// Nudges: back into place without swinging past.
pub(crate) const NUDGE: Feel = Feel {
    stiffness: theme::NUDGE_STIFFNESS,
    damping: theme::NUDGE_DAMPING,
};
/// Shape and content changes: settle without wobbling.
pub(crate) const MORPH: Feel = Feel {
    stiffness: theme::MORPH_STIFFNESS,
    damping: theme::MORPH_DAMPING,
};
/// The handover between an island's items and its popover's content: quicker than the shape, so
/// one side is gone in time for the other to come in while the shape still moves.
pub(crate) const FADE: Feel = Feel {
    stiffness: theme::FADE_STIFFNESS,
    damping: theme::FADE_DAMPING,
};
/// A popover's overshoot: kicked once, it swings past rest and back.
pub(crate) const SQUISH: Feel = Feel {
    stiffness: theme::SQUISH_STIFFNESS,
    damping: theme::SQUISH_DAMPING,
};

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
