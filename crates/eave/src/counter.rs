//! The host side of [`eave_api::ui::counter`]: a number and its unit, shown like a label. When the
//! number changes, it counts there through every number in between, fast at first and easing
//! into the new one. Like the switcher, it steps on the frames the compositor asks for, and stops
//! asking once it's there.

use std::ops::RangeInclusive;
use std::time::Instant;

use iced::advanced::text::{self, Paragraph as _};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::widget::canvas;
use iced::{Color, Element, Event, Length, Pixels, Point, Rectangle, Size, alignment, window};

use crate::{label, squish, theme};

type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;

pub struct Counter {
    value: i64,
    range: RangeInclusive<i64>,
    unit: String,
    color: Color,
}

impl Counter {
    pub fn new(value: i64, range: RangeInclusive<i64>, unit: String, color: Color) -> Self {
        Self {
            value,
            range,
            unit,
            color,
        }
    }
}

/// How many characters the widest number in `range` takes, its minus sign included.
fn places(range: &RangeInclusive<i64>) -> usize {
    let length = |n: i64| n.to_string().len();
    length(*range.start()).max(length(*range.end()))
}

#[derive(Default)]
struct Count {
    /// Where it's counting to, once it has a number at all.
    to: Option<i64>,
    from: i64,
    /// The number on screen.
    shown: i64,
    /// Changed, and waiting for a frame to start counting on. A counter that isn't on screen (in
    /// an alternate) waits until it is.
    pending: bool,
    start: Option<Instant>,
}

impl Count {
    /// Takes in the number. A new one is counted to from the one on screen.
    fn set(&mut self, value: i64) {
        match self.to {
            None => self.shown = value,
            Some(to) if to == value => return,
            Some(_) => {
                self.from = self.shown;
                self.pending = true;
                self.start = None;
            }
        }
        self.to = Some(value);
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        if self.pending {
            self.pending = false;
            self.start = Some(now);
        }
        let (Some(start), Some(to)) = (self.start, self.to) else {
            return false;
        };
        let t =
            now.saturating_duration_since(start).as_secs_f32() / theme::COUNTER_COUNT.as_secs_f32();
        if t >= 1.0 {
            self.shown = to;
            self.start = None;
            return false;
        }
        let eased = 1.0 - (1.0 - t).powi(3);
        self.shown = self.from + ((to - self.from) as f32 * eased).round() as i64;
        true
    }
}

struct State {
    count: Count,
    /// The text on screen, shaped.
    text: String,
    paragraph: Paragraph,
    /// The widest digit, so room can be kept for any number.
    digit: f32,
}

fn shape(content: &str, renderer: &iced::Renderer) -> Paragraph {
    use iced::advanced::text::Renderer as _;

    Paragraph::with_text(text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(theme::TEXT_SIZE),
        line_height: text::LineHeight::default(),
        font: renderer.default_font(),
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    })
}

impl State {
    /// Shapes the number on screen again, if it changed.
    fn reshape(&mut self, unit: &str, renderer: &iced::Renderer) {
        let text = format!("{}{unit}", self.count.shown);
        if text != self.text {
            self.paragraph = shape(&text, renderer);
            self.text = text;
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Counter {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        // Shaped on first layout, which has the renderer.
        tree::State::new(None::<State>)
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    /// Room for the widest number in its range (or the number itself, if it's outside), so it
    /// keeps its width as it counts.
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<Option<State>>();
        let state = state.get_or_insert_with(|| State {
            count: Count::default(),
            text: String::new(),
            paragraph: shape("", renderer),
            digit: ('0'..='9')
                .map(|digit| shape(&digit.to_string(), renderer).min_bounds().width)
                .fold(0.0, f32::max),
        });
        state.count.set(self.value);
        state.reshape(&self.unit, renderer);
        let unit = shape(&self.unit, renderer).min_bounds().width;
        let room = places(&self.range) as f32 * state.digit + unit;
        let target = shape(&format!("{}{}", self.value, self.unit), renderer).min_bounds();
        let height = text::LineHeight::default()
            .to_absolute(Pixels(theme::TEXT_SIZE))
            .0;
        layout::Node::new(Size::new(room.max(target.width).ceil(), height))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::text::Renderer as _;

        let Some(state) = tree.state.downcast_ref::<Option<State>>() else {
            return;
        };
        // Centered in the room it keeps.
        let bounds = layout.bounds();
        let width = state.paragraph.min_bounds().width;
        let at = Point::new(bounds.center_x() - width / 2.0, bounds.y);
        let squish = squish::current();
        if squish.is_none() {
            renderer.fill_paragraph(&state.paragraph, at, self.color, *viewport);
        } else {
            let text = canvas::Text {
                content: state.text.clone(),
                position: at,
                color: self.color,
                size: Pixels(theme::TEXT_SIZE),
                shaping: text::Shaping::Basic,
                ..canvas::Text::default()
            };
            label::draw_squished(renderer, squish, text, bounds);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && let Some(state) = tree.state.downcast_mut::<Option<State>>()
        {
            let more = state.count.step(*now);
            state.reshape(&self.unit, renderer);
            if more {
                shell.request_redraw();
            }
        }
    }
}

impl<'a, M: 'a> From<Counter> for Element<'a, M> {
    fn from(counter: Counter) -> Self {
        Element::new(counter)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn room_for_the_widest_number() {
        assert_eq!(places(&(0..=100)), 3);
        assert_eq!(places(&(0..=9)), 1);
        assert_eq!(places(&(-40..=50)), 3);
    }

    #[test]
    fn the_first_number_shows_at_once() {
        let mut count = Count::default();
        count.set(40);
        assert_eq!(count.shown, 40);
        assert!(!count.step(Instant::now()));
    }

    #[test]
    fn counts_through_every_number_in_between() {
        let mut count = Count::default();
        count.set(40);
        count.set(45);
        assert_eq!(count.shown, 40, "until a frame comes");
        let start = Instant::now();
        let mut seen = vec![count.shown];
        let mut now = start;
        while count.step(now) {
            if seen.last() != Some(&count.shown) {
                seen.push(count.shown);
            }
            now += Duration::from_millis(4);
        }
        seen.push(count.shown);
        seen.dedup();
        assert_eq!(seen, [40, 41, 42, 43, 44, 45]);
    }

    #[test]
    fn eases_in_and_counts_down_too() {
        let mut count = Count::default();
        count.set(100);
        count.set(0);
        let start = Instant::now();
        count.step(start);
        count.step(start + theme::COUNTER_COUNT / 2);
        assert!(
            count.shown < 50,
            "past halfway at half time: {}",
            count.shown
        );
        assert!(!count.step(start + theme::COUNTER_COUNT));
        assert_eq!(count.shown, 0);
    }

    #[test]
    fn a_new_number_mid_count_carries_on_from_the_screen() {
        let mut count = Count::default();
        count.set(40);
        count.set(50);
        let start = Instant::now();
        count.step(start);
        count.step(start + theme::COUNTER_COUNT / 2);
        let there = count.shown;
        count.set(60);
        assert_eq!(count.from, there);
    }
}
