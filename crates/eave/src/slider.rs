//! The host side of [`eave_api::ui::slider`]: a thin line, filled up to the value, with no knob
//! (D-61). Under the pointer, or while dragged, the line eases thicker, and back when it's left
//! alone. A click eases the fill to where it landed; a drag follows the hand exactly.
//!
//! Like iced's slider it sends the change on press and while dragging, the release message once
//! it's let go, and steps with the arrow keys or Ctrl and the wheel while the pointer is over it.

use std::ops::RangeInclusive;

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::keyboard::{self, Key, key};
use iced::{Border, Color, Element, Event, Length, Point, Rectangle, Size, touch, window};

use crate::ease::Ease;
use crate::theme;

pub struct Slider<'a, M> {
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_change: Box<dyn Fn(f32) -> M + 'a>,
    on_release: Option<M>,
    width: Length,
    color: Color,
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

#[derive(Default)]
struct State {
    /// How thick the line is, from at rest (0) to under the pointer (1).
    grow: Ease,
    /// How far the fill reaches, 0 to 1.
    fill: Ease,
    dragging: bool,
    /// The pointer has moved since the press: the fill follows it instead of easing.
    moved: bool,
    modifiers: keyboard::Modifiers,
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Slider<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(theme::SLIDER_HEIGHT))
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
        layout::atomic(limits, self.width, theme::SLIDER_HEIGHT)
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
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
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
            theme::TRACK,
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
                }
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                let position = cursor.land().position().filter(|_| state.dragging);
                position.map(|position| {
                    state.moved = true;
                    shell.capture_event();
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
                if grow || fill {
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
    fn the_fill_is_the_share_of_the_range() {
        assert_eq!(slider(75.0, 1.0).fraction(75.0), 0.5);
        assert_eq!(slider(500.0, 1.0).value, 150.0, "clamped to the range");
        let empty = Slider::new(3.0, 3.0..=3.0, 1.0, |_| (), None, Color::WHITE);
        assert_eq!(empty.fraction(3.0), 0.0);
    }
}
