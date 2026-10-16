//! A control under the hand ([D-127](../../../docs/decisions.md)). Wrapped around a button, a row
//! or a toggle, it hears the press without taking it, so the control under it still gets it:
//!
//! - it squeezes in a few pixels on `HAND`, most on the side the finger is on, and overshoots
//!   past its rest as it's let go;
//! - it keeps where the finger is while it's held, and a light that comes up there and fades
//!   once it's let go, for the glass to draw.
//!
//! iced's button keeps only whether it's pressed, not where, so this can't live in its style.

use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::touch;
use iced::{Element, Event, Length, Point, Rectangle, Size, Transformation, Vector, window};

use crate::spring::{FADE, HAND, MAX_FRAME, Spring};
use crate::theme;

pub struct Press<'a, M> {
    content: Element<'a, M>,
    /// Whether it squeezes: not in the bar, where its island squeezes as a whole.
    squeezes: bool,
}

impl<'a, M> Press<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>) -> Self {
        Self {
            content: content.into(),
            squeezes: true,
        }
    }

    /// Whether it squeezes under the hand.
    pub fn squeezes(mut self, squeezes: bool) -> Self {
        self.squeezes = squeezes;
        self
    }
}

#[derive(Debug)]
pub(crate) struct State {
    /// How far it's pressed in: 1 held, 0 at rest, a little under as it springs back past.
    pub press: Spring,
    /// How bright the light under the finger is.
    pub light: Spring,
    /// Where the finger is, from its top left, once it's been pressed.
    pub grab: Option<Vector>,
    held: bool,
    last: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            press: Spring::new(0.0, HAND),
            light: Spring::new(0.0, FADE),
            grab: None,
            held: false,
            last: None,
        }
    }
}

impl State {
    fn down(&mut self, at: Vector) {
        self.held = true;
        self.grab = Some(at);
        self.press.target = 1.0;
        self.light.target = 1.0;
    }

    fn up(&mut self) {
        self.held = false;
        self.press.target = 0.0;
        self.light.target = 0.0;
    }

    /// Advances to `now`. Returns whether another frame is needed.
    pub(crate) fn step(&mut self, now: Instant) -> bool {
        let seconds = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last)
                .min(MAX_FRAME)
                .as_secs_f32()
        });
        let mut moving = false;
        for spring in [&mut self.press, &mut self.light] {
            spring.advance(seconds);
            if spring.at_rest() {
                spring.settle();
            } else {
                moving = true;
            }
        }
        self.last = moving.then_some(now);
        moving
    }

    /// How much it's scaled, drawn `size`: in by `CONTROL_PRESS_INSET` on its sides at the
    /// press's height, the same few pixels whatever its size, and never by more than a tenth.
    pub(crate) fn scale(&self, size: Size) -> f32 {
        let side = (size.width * size.height).sqrt().max(1.0);
        (1.0 - self.press.value * 2.0 * theme::CONTROL_PRESS_INSET / side).max(0.9)
    }

    /// What it's drawn through, laid out at `bounds`: scaled about a point across from the
    /// finger, so the side the finger is on gives most.
    pub(crate) fn transformation(&self, bounds: Rectangle) -> Transformation {
        let scale = self.scale(bounds.size());
        let center = bounds.center();
        let pivot = match self.grab {
            Some(grab) => {
                let finger = bounds.position() + grab;
                center + (center - finger) * theme::CONTROL_PRESS_LEAN
            }
            None => center,
        };
        Transformation::translate(pivot.x, pivot.y)
            * Transformation::scale(scale)
            * Transformation::translate(-pivot.x, -pivot.y)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Press<'_, M> {
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
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let draw = |renderer: &mut iced::Renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        };
        if self.squeezes && state.press.value != 0.0 {
            renderer.with_transformation(state.transformation(layout.bounds()), draw);
        } else {
            draw(renderer);
        }
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        let pressed_at = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                cursor.position_over(bounds)
            }
            Event::Touch(touch::Event::FingerPressed { position, .. })
                if bounds.contains(*position) =>
            {
                Some(*position)
            }
            _ => None,
        };
        let moved_to = match event {
            Event::Mouse(mouse::Event::CursorMoved { position })
            | Event::Touch(touch::Event::FingerMoved { position, .. }) => Some(*position),
            _ => None,
        };
        let released = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
        );
        if let Some(at) = pressed_at {
            state.down(at - bounds.position());
            shell.request_redraw();
        } else if let Some(at) = moved_to.filter(|_| state.held) {
            // The finger, kept on the control: the light stays on it.
            let at = Point::new(
                at.x.clamp(bounds.x, bounds.x + bounds.width),
                at.y.clamp(bounds.y, bounds.y + bounds.height),
            );
            state.grab = Some(at - bounds.position());
            shell.request_redraw();
        } else if released && state.held {
            state.up();
            shell.request_redraw();
        } else if let Event::Window(window::Event::RedrawRequested(now)) = event
            && state.step(*now)
        {
            shell.request_redraw();
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Press<'a, M>> for Element<'a, M> {
    fn from(press: Press<'a, M>) -> Self {
        Element::new(press)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const FRAME: Duration = Duration::from_millis(8);

    /// Steps `state` frame by frame until it rests, calling `each` after every frame.
    fn run(state: &mut State, mut now: Instant, mut each: impl FnMut(&State)) -> Instant {
        state.step(now);
        for _ in 0..1000 {
            now += FRAME;
            let moving = state.step(now);
            each(state);
            if !moving {
                return now;
            }
        }
        panic!("it never came to rest");
    }

    #[test]
    fn a_press_squeezes_then_holds_still() {
        let mut state = State::default();
        let size = Size::new(80.0, 30.0);
        state.down(Vector::new(10.0, 15.0));
        run(&mut state, Instant::now(), |_| {});
        assert_eq!(state.press.value, 1.0);
        let scale = state.scale(size);
        assert!((0.9..0.97).contains(&scale), "a few pixels in: {scale}");
        assert!(
            !state.step(Instant::now() + Duration::from_secs(1)),
            "held, it asks for nothing"
        );
    }

    #[test]
    fn letting_go_overshoots_once_then_settles_in_place() {
        let mut state = State::default();
        state.down(Vector::ZERO);
        let now = run(&mut state, Instant::now(), |_| {});
        state.up();
        let mut past: f32 = 0.0;
        run(&mut state, now, |state| past = past.min(state.press.value));
        assert!(past < -0.01, "it springs back past rest: {past}");
        assert_eq!(state.press.value, 0.0);
        assert_eq!(state.light.value, 0.0, "the light goes with it");
        assert_eq!(state.scale(Size::new(80.0, 30.0)), 1.0);
    }

    #[test]
    fn the_side_under_the_finger_gives_most() {
        let mut state = State::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(100.0, 30.0));
        state.down(Vector::new(90.0, 15.0));
        run(&mut state, Instant::now(), |_| {});
        let transformation = state.transformation(bounds);
        let left = Point::new(0.0, 15.0) * transformation;
        let right = Point::new(100.0, 15.0) * transformation;
        assert!(
            100.0 - right.x > left.x,
            "the right side, under the finger, moves in more: {} vs {}",
            100.0 - right.x,
            left.x
        );
    }

    #[test]
    fn at_rest_asks_for_no_frames() {
        let mut state = State::default();
        assert!(!state.step(Instant::now()));
    }
}
