//! A plugin's little animation ([`eave_api::ui::sprite`]): frames it ships, shown one after
//! another in its order, a frame every [`theme::SPRITE_FRAME`]. It plays its loop
//! [`theme::SPRITE_PLAYS`] times when it shows up or changes, then rests on its first frame, so the
//! bar isn't redrawn all day; a looping one keeps going while it shows. It asks for a frame only
//! when the picture changes, not at the compositor's rate ([D-64](../../../docs/decisions.md)).

use std::time::{Duration, Instant};

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::widget::image;
use iced::{Element, Event, Length, Rectangle, Size, window};

use crate::{squish, theme};

pub struct Sprite {
    /// Tells this sprite from another, so a change starts it over.
    pub key: usize,
    pub frames: Vec<image::Handle>,
    /// Indices into `frames`, in the order they play.
    pub order: &'static [u8],
    pub looping: bool,
    pub opacity: f32,
}

#[derive(Debug, Default)]
struct State {
    key: usize,
    /// When it started playing: the first frame drawn.
    start: Option<Instant>,
    /// Where it is in `order`.
    at: usize,
}

/// Where a sprite with `order` is `elapsed` into playing, and how long until its picture changes
/// (`None`: it's at rest for good).
fn step(order: &[u8], looping: bool, elapsed: Duration) -> (usize, Option<Duration>) {
    let len = order.len().max(1);
    let frame = theme::SPRITE_FRAME;
    let played = (elapsed.as_nanos() / frame.as_nanos()) as usize;
    let end = theme::SPRITE_PLAYS * len;
    if !looping && played >= end {
        return (0, None);
    }
    let at = played % len;
    let shown = order.get(at);
    // Frames that repeat need no redraw: wait for the next one that differs.
    let mut next = played + 1;
    loop {
        if !looping && next >= end {
            if order.first() == shown {
                return (at, None);
            }
            break;
        }
        if order.get(next % len) != shown || next - played > len {
            break;
        }
        next += 1;
    }
    let until = frame * next as u32;
    (at, Some(until.saturating_sub(elapsed)))
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Sprite {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            key: self.key,
            ..State::default()
        })
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.key != self.key {
            *state = State {
                key: self.key,
                ..State::default()
            };
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(theme::ICON_SIZE),
            Length::Fixed(theme::ICON_SIZE),
        )
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(theme::ICON_SIZE, theme::ICON_SIZE))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::image::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let index = self.order.get(state.at).copied().unwrap_or(0) as usize;
        let Some(handle) = self.frames.get(index).or(self.frames.first()) else {
            return;
        };
        let squish = squish::current();
        let bounds = squish.rect(layout.bounds());
        renderer.draw_image(
            iced::advanced::image::Image {
                opacity: self.opacity,
                snap: squish.is_none(),
                ..iced::advanced::image::Image::new(handle.clone())
            },
            bounds,
            bounds,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };
        let state = tree.state.downcast_mut::<State>();
        let start = *state.start.get_or_insert(*now);
        let (at, next) = step(
            self.order,
            self.looping,
            now.saturating_duration_since(start),
        );
        state.at = at;
        if let Some(next) = next {
            shell.request_redraw_at(*now + next);
        }
    }
}

impl<'a, M: 'a> From<Sprite> for Element<'a, M> {
    fn from(sprite: Sprite) -> Self {
        Element::new(sprite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: u32) -> Duration {
        theme::SPRITE_FRAME * n
    }

    #[test]
    fn it_steps_through_its_order_and_waits_out_repeats() {
        let order = [0, 0, 1, 2];
        assert_eq!(step(&order, true, Duration::ZERO), (0, Some(frames(2))));
        assert_eq!(step(&order, true, frames(1)), (1, Some(frames(1))));
        assert_eq!(step(&order, true, frames(2)), (2, Some(frames(1))));
        // The last frame wraps round to the first.
        assert_eq!(step(&order, true, frames(3)), (3, Some(frames(1))));
        assert_eq!(step(&order, true, frames(4)), (0, Some(frames(2))));
    }

    #[test]
    fn it_rests_on_the_first_frame_after_playing_unless_it_loops() {
        let order = [0, 1, 1, 2];
        let end = frames(4 * theme::SPRITE_PLAYS as u32);
        assert_eq!(step(&order, false, end), (0, None));
        assert_eq!(step(&order, false, end + frames(9)), (0, None));
        // The last frame of the last loop changes to the resting one.
        assert_eq!(step(&order, false, end - frames(1)), (3, Some(frames(1))));
        assert_eq!(step(&order, true, end), (0, Some(frames(1))));
    }

    #[test]
    fn a_play_ending_on_the_resting_frame_stops_early() {
        let order = [0, 1, 0, 0];
        let end = frames(4 * theme::SPRITE_PLAYS as u32);
        assert_eq!(step(&order, false, end - frames(2)), (2, None));
    }

    #[test]
    fn a_still_sprite_never_asks_for_frames_when_resting() {
        assert_eq!(step(&[], false, frames(99)), (0, None));
        let (_, next) = step(&[0, 0], true, Duration::ZERO);
        assert!(next.is_some(), "a looping one still wakes once a loop");
    }
}
