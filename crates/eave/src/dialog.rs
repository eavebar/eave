//! A widget's dialog (D-90): its panel in the middle of the screen, for what needs more room than
//! a popover, like a conversation. It comes out of what was just acted on: the shape of the
//! popover, the panel of notifications or the item's island springs on to it while the island
//! stays on the bar, and the dormer turns into it. Asked for out of the blue, it drops out of the
//! top, as the dormer does. It goes back where it came from.

use std::time::{Duration, Instant};

use iced::Rectangle;

use crate::app::IslandId;

/// How long after something is acted on a dialog asked for still comes out of it: long enough for
/// a notification's click to go round the session bus to the widget that sent it, and back.
pub const ORIGIN_WINDOW: Duration = Duration::from_secs(1);

/// What happens to the dialog.
#[derive(Debug, Clone)]
pub enum Event {
    /// The one asked for is ready to open, with this much of the screen below the surface's top.
    Opened(f32),
    /// Its island placed it.
    Placed(Rectangle),
    /// Its neck let go of it as it came out (false), or caught it as it went back (true).
    Neck(bool),
    /// Its island finished turning back.
    Closed,
}

/// Where a dialog comes out of, and goes back into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The notch, or the top edge, like the dormer.
    Top,
    /// An island on the bar: the one whose item, popover or notification was acted on. The dialog
    /// comes out of its shape, and goes back onto it.
    Island(IslandId),
}

/// What was last acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    /// An item, its popover, or the notifications its island shows.
    Island(IslandId),
    /// A row in the dormer.
    Dormer,
}

/// The last thing acted on, and when: a dialog asked for soon after comes out of it.
#[derive(Debug, Clone, Copy)]
pub struct Touched {
    pub touch: Touch,
    pub at: Instant,
}

impl Touched {
    /// What was acted on, if that was recent enough at `now` to come out of.
    pub fn fresh(touched: Option<Self>, now: Instant) -> Option<Touch> {
        touched
            .filter(|touched| now.saturating_duration_since(touched.at) <= ORIGIN_WINDOW)
            .map(|touched| touched.touch)
    }
}

/// The dialog on screen, from when it's opened until it's back where it came from.
pub struct Dialog {
    /// The id of the instance it belongs to.
    pub instance: u64,
    /// False while it goes back, until its island says it's done.
    pub open: bool,
    /// How far the screen reaches down from the surface's top: it's centered in that.
    pub room: f32,
    /// Where its island put it, in surface coordinates. The surface grows to fit it.
    pub rect: Option<Rectangle>,
    pub origin: Origin,
}

impl Dialog {
    pub fn new(instance: u64, room: f32, origin: Origin) -> Self {
        Self {
            instance,
            open: true,
            room,
            rect: None,
            origin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_touch_counts_only_for_a_moment() {
        let at = Instant::now();
        let touched = Some(Touched {
            touch: Touch::Dormer,
            at,
        });
        assert_eq!(Touched::fresh(touched, at), Some(Touch::Dormer));
        assert_eq!(
            Touched::fresh(touched, at + ORIGIN_WINDOW / 2),
            Some(Touch::Dormer)
        );
        assert_eq!(Touched::fresh(touched, at + 2 * ORIGIN_WINDOW), None);
        assert_eq!(Touched::fresh(None, at), None);
    }
}
