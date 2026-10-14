//! The host side of [`eave_api::ui::counted`]: content with a count in a red badge on its top
//! right corner, like unread notifications on a bell. The badge grows in from nothing and shrinks
//! back on the morph spring, counts through the numbers in between like a counter, and springs
//! from a circle to a pill when the number gets a second digit. Like the counter, it steps on the
//! frames the compositor asks for, and stops asking once it's still. It's drawn in a layer of its
//! own, over its content.

use std::time::Instant;

use iced::advanced::text::{Alignment, Paragraph as _};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer, text};
use iced::widget::canvas::{self, Frame, Path};
use iced::{
    Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Vector, alignment, window,
};

use eave_api::ui::Tone;

use crate::counter::{Count, shape};
use crate::spring::{self, MAX_FRAME, REST_DISTANCE, Spring};
use crate::{island, squish, theme};

/// The most the badge shows: two digits keep it a short pill.
const MOST: u32 = 99;

pub struct Pip<'a, M> {
    content: Element<'a, M>,
    count: u32,
}

impl<'a, M> Pip<'a, M> {
    pub fn new(content: Element<'a, M>, count: u32) -> Self {
        Self {
            content,
            count: count.min(MOST),
        }
    }
}

struct State {
    count: Count,
    /// How grown in the badge is: 0 gone, 1 all there.
    grow: Spring,
    width: Spring,
    /// The last frame stepped, while it moves.
    last: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            count: Count::default(),
            grow: Spring::new(0.0, spring::MORPH),
            width: Spring::new(theme::PIP_HEIGHT, spring::MORPH),
            last: None,
        }
    }
}

impl State {
    /// Takes in the count, and the badge's width for it.
    fn set(&mut self, count: u32, width: f32) {
        if count == 0 {
            // It shrinks away showing the last number it had.
            self.grow.target = 0.0;
            return;
        }
        if self.gone() {
            // Coming back: with the new number at once, not counting from the old one.
            self.count = Count::default();
            self.width = Spring::new(width, spring::MORPH);
        }
        self.count.set(i64::from(count));
        self.width.target = width;
        self.grow.target = 1.0;
    }

    fn gone(&self) -> bool {
        self.grow.target == 0.0 && self.grow.value < REST_DISTANCE
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        let seconds = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last)
                .min(MAX_FRAME)
                .as_secs_f32()
        });
        self.grow.advance(seconds);
        self.width.advance(seconds);
        let counting = self.count.step(now);
        let mut moving = counting;
        for spring in [&mut self.grow, &mut self.width] {
            if spring.at_rest() {
                spring.settle();
            } else {
                moving = true;
            }
        }
        self.last = moving.then_some(now);
        moving
    }
}

/// How wide the badge is with `count` in it: a circle for one digit, a pill for more.
fn width(count: u32, renderer: &iced::Renderer) -> f32 {
    let text = shape(
        &count.to_string(),
        renderer,
        theme::PIP_TEXT_SIZE,
        Some(theme::STRONG_FONT),
    );
    let digits = text.min_bounds().width;
    if count < 10 {
        theme::PIP_HEIGHT
    } else {
        (digits + 2.0 * theme::PIP_PADDING).max(theme::PIP_HEIGHT)
    }
}

/// How much room the badge takes past the content's right edge.
fn room(width: f32) -> f32 {
    (width - theme::PIP_OVERLAP).max(0.0).ceil()
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Pip<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    /// The content, with room on its right for what of the badge sticks out, while there's a
    /// count. The island springs to the new width; the badge's own size is only drawn.
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let width = width(self.count, renderer);
        tree.state.downcast_mut::<State>().set(self.count, width);
        let room = if self.count == 0 { 0.0 } else { room(width) };
        let size = content.size();
        layout::Node::with_children(Size::new(size.width + room, size.height), vec![content])
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let p = crate::theme::Palette::of(theme);
        use iced::advanced::Renderer as _;
        use iced::advanced::graphics::geometry::Renderer as _;

        let content = layout.children().next().expect("the content");
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            content,
            cursor,
            viewport,
        );
        let state = tree.state.downcast_ref::<State>();
        let grow = state.grow.value;
        if grow < REST_DISTANCE {
            return;
        }
        let bounds = content.bounds();
        let size = Size::new(state.width.value.max(theme::PIP_HEIGHT), theme::PIP_HEIGHT);
        let left = bounds.x + bounds.width - theme::PIP_OVERLAP;
        let center = Point::new(left + size.width / 2.0, bounds.y + theme::PIP_DROP);
        // It grows and squishes about its own middle, like the island's other things do.
        let squish = squish::current();
        let area = layout.bounds().expand(theme::PIP_HEIGHT);
        let mut frame = Frame::with_bounds(renderer, area);
        frame.translate(Vector::new(center.x, center.y));
        frame.scale_nonuniform(Vector::new(squish.x * grow, squish.y * grow));
        frame.translate(Vector::new(-center.x, -center.y));
        let corner = Point::new(center.x - size.width / 2.0, center.y - size.height / 2.0);
        let pill = Path::rounded_rectangle(corner, size, (size.height / 2.0).into());
        frame.fill(&pill, p.tone(Tone::Critical));
        frame.fill_text(canvas::Text {
            content: state.count.shown.to_string(),
            position: center,
            color: Color::WHITE,
            size: Pixels(theme::PIP_TEXT_SIZE),
            font: theme::STRONG_FONT,
            align_x: Alignment::Center,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            ..canvas::Text::default()
        });
        // A layer of its own, so it lands over the content whatever that's drawn with: within a
        // layer, the renderer draws by kind, not in order. Kept to what the island shows.
        let Some(layer) = area.intersection(&island::clip().unwrap_or(*viewport)) else {
            return;
        };
        renderer.with_layer(layer, |renderer| {
            renderer.draw_geometry(frame.into_geometry());
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && tree.state.downcast_mut::<State>().step(*now)
        {
            shell.request_redraw();
        }
        let content = layout.children().next().expect("the content");
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let content = layout.children().next().expect("the content");
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], content, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let content = layout.children().next().expect("the content");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            content,
            cursor,
            viewport,
            renderer,
        )
    }
}

impl<'a, M: 'a> From<Pip<'a, M>> for Element<'a, M> {
    fn from(pip: Pip<'a, M>) -> Self {
        Element::new(pip)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Steps `state` a frame at a time from `start` until it's still. Returns when that was.
    fn settle(state: &mut State, start: Instant) -> Instant {
        let mut now = start;
        let mut frames = 0;
        while state.step(now) {
            now += Duration::from_millis(16);
            frames += 1;
            assert!(frames < 200, "settles");
        }
        now
    }

    #[test]
    fn no_count_no_badge() {
        let mut state = State::default();
        state.set(0, theme::PIP_HEIGHT);
        assert!(!state.step(Instant::now()), "asks for no frames");
        assert_eq!(state.grow.value, 0.0);
    }

    #[test]
    fn grows_in_with_its_number_and_out_with_it() {
        let mut state = State::default();
        state.set(3, theme::PIP_HEIGHT);
        assert_eq!(state.count.shown, 3, "the first number shows at once");
        let now = settle(&mut state, Instant::now());
        assert_eq!(state.grow.value, 1.0);
        state.set(0, theme::PIP_HEIGHT);
        settle(&mut state, now);
        assert_eq!(state.grow.value, 0.0);
        assert_eq!(state.count.shown, 3, "shrinks away showing what it had");
    }

    #[test]
    fn counts_up_and_widens_for_two_digits() {
        let mut state = State::default();
        state.set(8, theme::PIP_HEIGHT);
        let now = settle(&mut state, Instant::now());
        state.set(12, 20.0);
        let now = now + Duration::from_millis(16);
        assert!(state.step(now));
        assert!(state.count.shown < 12, "counts through the numbers between");
        settle(&mut state, now);
        assert_eq!(state.count.shown, 12);
        assert_eq!(state.width.value, 20.0);
    }

    #[test]
    fn coming_back_starts_from_the_new_number() {
        let mut state = State::default();
        state.set(40, 20.0);
        let now = settle(&mut state, Instant::now());
        state.set(0, 20.0);
        let now = settle(&mut state, now);
        state.set(1, theme::PIP_HEIGHT);
        assert_eq!(state.count.shown, 1);
        assert_eq!(state.width.value, theme::PIP_HEIGHT);
        settle(&mut state, now);
    }

    #[test]
    fn past_ninety_nine_shows_ninety_nine() {
        let pip = Pip::<()>::new(iced::widget::text("").into(), 250);
        assert_eq!(pip.count, MOST);
    }

    #[test]
    fn room_is_what_sticks_out() {
        assert_eq!(room(theme::PIP_OVERLAP - 1.0), 0.0);
        assert_eq!(
            room(theme::PIP_HEIGHT),
            (theme::PIP_HEIGHT - theme::PIP_OVERLAP).ceil()
        );
    }
}
