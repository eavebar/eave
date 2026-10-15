//! The bar sliding up off the screen and back (D-88): its content is drawn lifted by a spring,
//! on `MORPH`, since it's going somewhere. Only drawn: the layout stays where the bar rests, so
//! the islands keep their places and the surface its size. Lifted all the way and resting, the
//! content isn't drawn and gets no events, so a hidden bar asks for no frames.

use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, window};

use crate::spring::{self, Spring};

/// About how long it takes to slide all the way, out or away.
pub fn slide_time() -> std::time::Duration {
    let mut spring = Spring::new(0.0, spring::MORPH);
    spring.target = 1.0;
    let mut time = 0.0;
    while !spring.at_rest() && time < 5.0 {
        spring.advance(spring::SUBSTEP);
        time += spring::SUBSTEP;
    }
    std::time::Duration::from_secs_f32(time)
}

pub struct Lift<'a, M> {
    content: Element<'a, M>,
    /// Whether it's out, or lifted away.
    out: bool,
    /// How far up it goes when lifted: enough to clear the screen.
    by: f32,
}

impl<'a, M> Lift<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, out: bool, by: f32) -> Self {
        Self {
            content: content.into(),
            out,
            by,
        }
    }
}

/// How far out it is, 1 out and 0 lifted, and when it last moved. `None` before its first
/// layout, which puts it where it should be without moving.
#[derive(Default)]
struct State {
    spring: Option<Spring>,
    last: Option<Instant>,
}

impl State {
    fn aim(&mut self, out: bool) {
        let target = if out { 1.0 } else { 0.0 };
        let spring = self
            .spring
            .get_or_insert_with(|| Spring::new(target, spring::MORPH));
        if spring.target != target {
            spring.target = target;
            self.last = None;
        }
    }

    /// Moves it on to `now`. Whether it's still moving.
    fn step(&mut self, now: Instant) -> bool {
        let Some(spring) = &mut self.spring else {
            return false;
        };
        if spring.at_rest() {
            spring.settle();
            self.last = None;
            return false;
        }
        let last = self.last.replace(now).unwrap_or(now);
        let elapsed = now.saturating_duration_since(last).min(spring::MAX_FRAME);
        spring.advance(elapsed.as_secs_f32());
        true
    }

    fn out(&self) -> f32 {
        self.spring.map_or(1.0, |spring| spring.value)
    }

    /// Lifted all the way, and staying there.
    fn gone(&self) -> bool {
        self.spring
            .is_some_and(|spring| spring.target == 0.0 && spring.at_rest())
    }
}

impl<M> Lift<'_, M> {
    /// How far its content is drawn from where it's laid out.
    fn offset(&self, state: &State) -> Vector {
        Vector::new(0.0, -(1.0 - state.out()) * self.by)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Lift<'_, M> {
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

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        tree.state.downcast_mut::<State>().aim(self.out);
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
        if state.gone() {
            return;
        }
        let offset = self.offset(state);
        renderer.with_translation(offset, |renderer| {
            crate::glass::frame::translated(offset, || {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor - offset,
                    viewport,
                );
            });
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
        let state = tree.state.downcast_mut::<State>();
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && state.step(*now)
        {
            shell.request_redraw();
        }
        if state.gone() {
            return;
        }
        let offset = self.offset(state);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor - offset,
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
        let state = tree.state.downcast_ref::<State>();
        if state.gone() {
            return mouse::Interaction::None;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor - self.offset(state),
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
        let state = tree.state.downcast_ref::<State>();
        if state.gone() {
            return None;
        }
        let offset = self.offset(state);
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation + offset,
        )
    }
}

impl<'a, M: 'a> From<Lift<'a, M>> for Element<'a, M> {
    fn from(lift: Lift<'a, M>) -> Self {
        Element::new(lift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn play(state: &mut State, from: Instant) -> Instant {
        let mut now = from;
        while state.step(now) {
            now += Duration::from_millis(8);
            assert!(now < from + Duration::from_secs(3), "never settles");
        }
        now
    }

    #[test]
    fn slides_in_a_moment() {
        let time = slide_time();
        assert!(
            time > Duration::from_millis(150) && time < Duration::from_secs(1),
            "{time:?}"
        );
    }

    #[test]
    fn starts_where_it_should_be_without_moving() {
        let mut state = State::default();
        state.aim(false);
        assert!(state.gone());
        assert!(!state.step(Instant::now()));

        let mut state = State::default();
        state.aim(true);
        assert_eq!(state.out(), 1.0);
        assert!(!state.step(Instant::now()));
    }

    #[test]
    fn slides_out_and_back_without_wobbling_past_the_screen() {
        let mut state = State::default();
        state.aim(false);
        state.aim(true);
        assert!(!state.gone(), "drawn as soon as it starts coming out");
        let mut now = Instant::now();
        let mut furthest: f32 = 0.0;
        while state.step(now) {
            furthest = furthest.max(state.out());
            now += Duration::from_millis(8);
        }
        assert!((state.out() - 1.0).abs() < spring::REST_DISTANCE);
        assert!(furthest < 1.02, "{furthest}");

        state.aim(false);
        play(&mut state, now);
        assert!(state.gone());
    }
}
