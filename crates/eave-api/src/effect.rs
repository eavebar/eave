use std::time::Duration;

use crate::Task;

/// What [`Widget::update`](crate::Widget::update) asks for: async work, plus requests to the host.
pub struct Effect<M> {
    task: Task<M>,
    popover: Option<PopoverRequest>,
    nudge: Option<NudgeRequest>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverRequest {
    Open,
    Close,
}

/// Which way something just went, so the host can give the widget's island a nudge that way.
///
/// It says what happened, not how to move: the host picks the motion. A value going up or down
/// (a volume, a brightness) bumps the island's top or bottom edge out at the widget's item; moving
/// to the next or previous thing (a workspace, a track) pushes the whole island left or right,
/// and it springs back into place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    Up,
    Down,
    Left,
    Right,
}

/// A nudge as the host gets it: which way, and how long what it follows takes on screen, if the
/// widget knows.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NudgeRequest {
    pub nudge: Nudge,
    pub lasting: Option<Duration>,
}

impl From<Nudge> for NudgeRequest {
    fn from(nudge: Nudge) -> Self {
        Self {
            nudge,
            lasting: None,
        }
    }
}

impl<M> Effect<M> {
    pub fn none() -> Self {
        Self {
            task: Task::none(),
            popover: None,
            nudge: None,
        }
    }

    pub fn task(task: Task<M>) -> Self {
        Self {
            task,
            popover: None,
            nudge: None,
        }
    }

    /// Opens this widget's popover, under its item. Ignored if the widget has no popover.
    pub fn open_popover(mut self) -> Self {
        self.popover = Some(PopoverRequest::Open);
        self
    }

    pub fn close_popover(mut self) -> Self {
        self.popover = Some(PopoverRequest::Close);
        self
    }

    /// Nudges this widget's island. Ignored while the island is held or has a popover.
    pub fn nudge(mut self, nudge: Nudge) -> Self {
        self.nudge = Some(NudgeRequest {
            nudge,
            lasting: None,
        });
        self
    }

    /// Nudges this widget's island in step with something else on screen that takes `lasting`,
    /// like the compositor's own workspace animation: the host plays the same motion, slowed down
    /// or sped up to settle over about that long.
    pub fn nudge_lasting(mut self, nudge: Nudge, lasting: Duration) -> Self {
        self.nudge = Some(NudgeRequest {
            nudge,
            lasting: Some(lasting),
        });
        self
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> (Task<M>, Option<PopoverRequest>, Option<NudgeRequest>) {
        (self.task, self.popover, self.nudge)
    }
}

impl<M> From<Task<M>> for Effect<M> {
    fn from(task: Task<M>) -> Self {
        Self::task(task)
    }
}
