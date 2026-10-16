//! The host side of [`eave_api::ui::slider`]: a thin line, filled up to the value, with no knob
//! (D-61). Under the pointer, or while dragged, the line eases thicker, and back when it's left
//! alone. A click eases the fill to where it landed; a drag follows the hand exactly. In a
//! popover it's a groove instead, a capsule filled up to the value (D-127). A drag past either
//! end stretches that end, less and less, and it springs back when let go.
//!
//! Like iced's slider it sends the change on press and while dragging, the release message once
//! it's let go, and steps with the arrow keys or Ctrl and the wheel while the pointer is over it.

use std::ops::RangeInclusive;

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::keyboard::{self, Key, key};
use iced::{Border, Color, Element, Event, Length, Point, Rectangle, Size, touch, window};

use std::time::Instant;

use crate::ease::Ease;
use crate::glass::frame::{self, Relief, Shape};
use crate::hand;
use crate::spring::{HAND, MAX_FRAME, Spring};
use crate::theme;

pub struct Slider<'a, M> {
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_change: Box<dyn Fn(f32) -> M + 'a>,
    on_release: Option<M>,
    width: Length,
    color: Color,
    /// A groove's corner, in a popover; a line in the bar.
    groove: Option<f32>,
    /// Whether its fill is the ink's, which turns the glass milky (D-126).
    milky: bool,
}

impl<'a, M> Slider<'a, M> {
    pub fn new(
        value: f32,
        range: RangeInclusive<f32>,
        step: f32,
        on_change: impl Fn(f32) -> M + 'a,
        on_release: Option<M>,
        color: Color,
    ) -> Self {
        Self {
            value: value.clamp(*range.start(), *range.end()),
            range,
            step,
            on_change: Box::new(on_change),
            on_release,
            width: Length::Fill,
            color,
            groove: None,
            milky: false,
        }
    }

    /// A groove with corners `radius`, instead of a line: a well in the glass, milky up to the
    /// value if `milky`.
    pub fn groove(mut self, radius: f32, milky: bool) -> Self {
        self.groove = Some(radius);
        self.milky = milky;
        self
    }

    fn height(&self) -> f32 {
        if self.groove.is_some() {
            theme::SLIDER_GROOVE
        } else {
            theme::SLIDER_HEIGHT
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// How far along the range `value` is, 0 to 1.
    fn fraction(&self, value: f32) -> f32 {
        let (start, end) = (*self.range.start(), *self.range.end());
        if end > start {
            ((value - start) / (end - start)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// How far past its ends `x` is: negative before the start, positive after the end.
    fn past(bounds: Rectangle, x: f32) -> f32 {
        if x < bounds.x {
            x - bounds.x
        } else {
            (x - (bounds.x + bounds.width)).max(0.0)
        }
    }

    /// The value at `x`, snapped to the step.
    fn locate(&self, bounds: Rectangle, x: f32) -> f32 {
        let (start, end) = (*self.range.start(), *self.range.end());
        let fraction = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
        self.snap(start + fraction * (end - start))
    }

    fn snap(&self, value: f32) -> f32 {
        let (start, end) = (*self.range.start(), *self.range.end());
        let stepped = if self.step > 0.0 {
            start + ((value - start) / self.step).round() * self.step
        } else {
            value
        };
        stepped.clamp(start, end)
    }
}

struct State {
    /// How thick the line is, from at rest (0) to under the pointer (1).
    grow: Ease,
    /// How far the fill reaches, 0 to 1.
    fill: Ease,
    /// How far an end is stretched by a drag past it: the end, right if positive.
    stretch: Spring,
    last: Option<Instant>,
    dragging: bool,
    /// The pointer has moved since the press: the fill follows it instead of easing.
    moved: bool,
    modifiers: keyboard::Modifiers,
}

impl Default for State {
    fn default() -> Self {
        Self {
            grow: Ease::default(),
            fill: Ease::default(),
            stretch: Spring::new(0.0, HAND),
            last: None,
            dragging: false,
            moved: false,
            modifiers: keyboard::Modifiers::default(),
        }
    }
}

impl State {
    /// Stretches toward a drag `past` the ends, by that much in pixels.
    fn pull(&mut self, past: f32) {
        self.stretch.target = hand::rubber_band(past, theme::SLIDER_STRETCH);
    }

    /// Advances the stretch to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        let seconds = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last)
                .min(MAX_FRAME)
                .as_secs_f32()
        });
        self.stretch.advance(seconds);
        let moving = if self.stretch.at_rest() {
            self.stretch.settle();
            false
        } else {
            true
        };
        self.last = moving.then_some(now);
        moving
    }

    /// `bounds`, stretched at the end that's pulled.
    fn stretched(&self, bounds: Rectangle) -> Rectangle {
        let stretch = self.stretch.value;
        if stretch >= 0.0 {
            Rectangle {
                width: bounds.width + stretch,
                ..bounds
            }
        } else {
            Rectangle {
                x: bounds.x + stretch,
                width: bounds.width - stretch,
                ..bounds
            }
        }
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Slider<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.height()))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let fraction = self.fraction(self.value);
        if state.moved {
            state.fill.snap(fraction);
        } else {
            state.fill.set(fraction);
        }
        layout::atomic(limits, self.width, self.height())
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let p = crate::theme::Palette::of(theme);
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = state.stretched(layout.bounds());
        if let Some(radius) = self.groove {
            // A groove: brighter under the pointer, filled up to the value with the fill at
            // least as wide as it's tall, so it stays round at the start.
            let radius = radius.min(bounds.height / 2.0);
            let rounded = Border::default().rounded(radius);
            let filled = bounds.width * state.fill.value();
            if crate::press::on_glass() {
                // A well in the glass, milky up to the value (D-126).
                frame::relief(
                    Relief {
                        rect: bounds,
                        radius,
                        shape: Shape::Well,
                        depth: theme::RELIEF_WELL,
                        // Deeper under the pointer, as the line grows thicker without glass.
                        press: 0.5 * state.grow.value(),
                        milk: if self.milky { 1.0 } else { 0.0 },
                        fill_x: bounds.x + filled.max(bounds.height),
                        light: bounds.position(),
                        lit: 0.0,
                    },
                    viewport,
                );
            } else {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: rounded,
                        ..renderer::Quad::default()
                    },
                    p.track.scale_alpha(1.0 + 0.5 * state.grow.value()),
                );
            }
            if filled >= 0.5 {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            bounds.position(),
                            Size::new(filled.max(bounds.height), bounds.height),
                        ),
                        border: rounded,
                        ..renderer::Quad::default()
                    },
                    self.color,
                );
            }
            return;
        }
        let height = theme::SLIDER_RAIL
            + (theme::SLIDER_RAIL_HOVER - theme::SLIDER_RAIL) * state.grow.value();
        let rail = Rectangle::new(
            Point::new(bounds.x, bounds.center_y() - height / 2.0),
            Size::new(bounds.width, height),
        );
        let rounded = Border::default().rounded(height / 2.0);
        renderer.fill_quad(
            renderer::Quad {
                bounds: rail,
                border: rounded,
                ..renderer::Quad::default()
            },
            p.track,
        );
        let filled = rail.width * state.fill.value();
        if filled >= 0.5 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(rail.position(), Size::new(filled.max(height), height)),
                    border: rounded,
                    ..renderer::Quad::default()
                },
                self.color,
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let set = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                cursor.position_over(bounds).map(|position| {
                    state.dragging = true;
                    state.moved = false;
                    shell.capture_event();
                    self.locate(bounds, position.x)
                })
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. }) => {
                if state.dragging {
                    if let Some(message) = self.on_release.clone() {
                        shell.publish(message);
                    }
                    state.dragging = false;
                    state.moved = false;
                    state.pull(0.0);
                    shell.request_redraw();
                }
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                let position = cursor.land().position().filter(|_| state.dragging);
                position.map(|position| {
                    state.moved = true;
                    shell.capture_event();
                    state.pull(Self::past(bounds, position.x));
                    shell.request_redraw();
                    self.locate(bounds, position.x)
                })
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if state.modifiers.control() && cursor.is_over(bounds) =>
            {
                let (mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. }) =
                    delta;
                shell.capture_event();
                Some(self.snap(self.value + self.step.copysign(*y)))
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) if cursor.is_over(bounds) => {
                let step = match key {
                    Key::Named(key::Named::ArrowUp | key::Named::ArrowRight) => self.step,
                    Key::Named(key::Named::ArrowDown | key::Named::ArrowLeft) => -self.step,
                    _ => return,
                };
                shell.capture_event();
                Some(self.snap(self.value + step))
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                let grow = state.grow.step(*now, theme::SLIDER_GROW);
                let fill = state.fill.step(*now, theme::SLIDER_EASE);
                let stretch = state.step(*now);
                if grow || fill || stretch {
                    shell.request_redraw();
                }
                return;
            }
            _ => None,
        };
        if let Some(value) = set
            && (value - self.value).abs() > f32::EPSILON
        {
            shell.publish((self.on_change)(value));
            self.value = value;
        }
        // Thicker under the pointer, or while held.
        let grown = state.dragging || cursor.is_over(bounds);
        let target = if grown { 1.0 } else { 0.0 };
        if state.grow.target() != target {
            state.grow.set(target);
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<State>().dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, M: Clone + 'a> From<Slider<'a, M>> for Element<'a, M> {
    fn from(slider: Slider<'a, M>) -> Self {
        Element::new(slider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(value: f32, step: f32) -> Slider<'static, ()> {
        Slider::new(value, 0.0..=150.0, step, |_| (), None, Color::WHITE)
    }

    #[test]
    fn a_point_on_the_line_is_a_value_snapped_to_the_step() {
        let bounds = Rectangle::new(Point::new(10.0, 0.0), Size::new(300.0, 16.0));
        let slider = slider(0.0, 5.0);
        assert_eq!(slider.locate(bounds, 10.0), 0.0);
        assert_eq!(slider.locate(bounds, 161.0), 75.0);
        assert_eq!(slider.locate(bounds, 164.0), 75.0, "snapped to 5");
        assert_eq!(slider.locate(bounds, 500.0), 150.0, "past the end");
        assert_eq!(slider.locate(bounds, -50.0), 0.0, "before the start");
    }

    #[test]
    fn a_drag_past_an_end_stretches_it_less_and_less_then_springs_back() {
        let bounds = Rectangle::new(Point::new(10.0, 0.0), Size::new(300.0, 28.0));
        assert_eq!(Slider::<()>::past(bounds, 200.0), 0.0, "on the line");
        assert_eq!(Slider::<()>::past(bounds, 330.0), 20.0);
        assert_eq!(Slider::<()>::past(bounds, 0.0), -10.0);

        let mut state = State::default();
        let (mut now, frame) = (Instant::now(), std::time::Duration::from_millis(8));
        let settle = |state: &mut State, now: &mut Instant| {
            state.step(*now);
            for _ in 0..1000 {
                *now += frame;
                if !state.step(*now) {
                    return;
                }
            }
            panic!("it never came to rest");
        };
        state.pull(40.0);
        settle(&mut state, &mut now);
        let little = state.stretch.value;
        state.pull(400.0);
        settle(&mut state, &mut now);
        let far = state.stretch.value;
        assert!(little > 0.0 && far > little, "{little} then {far}");
        assert!(far < theme::SLIDER_STRETCH, "never past the most: {far}");
        assert!(far - little < 400.0 - 40.0, "it gives less and less");
        let stretched = state.stretched(bounds);
        assert_eq!(stretched.x, bounds.x, "the start stays");
        assert_eq!(stretched.width, bounds.width + far);

        state.pull(0.0);
        settle(&mut state, &mut now);
        assert_eq!(state.stretch.value, 0.0, "let go, it springs back");
        state.pull(-30.0);
        settle(&mut state, &mut now);
        assert!(
            state.stretched(bounds).x < bounds.x,
            "a drag before the start stretches it"
        );
    }

    #[test]
    fn the_fill_is_the_share_of_the_range() {
        assert_eq!(slider(75.0, 1.0).fraction(75.0), 0.5);
        assert_eq!(slider(500.0, 1.0).value, 150.0, "clamped to the range");
        let empty = Slider::new(3.0, 3.0..=3.0, 1.0, |_| (), None, Color::WHITE);
        assert_eq!(empty.fraction(3.0), 0.0);
    }
}
