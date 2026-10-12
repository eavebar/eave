//! The host side of [`eave_api::ui::gulp`]: content that keeps gulping, like a bottle being
//! poured. It swells from its middle and settles back, one gulp after another, for as long as it's
//! on screen. Like the switcher, it steps on the frames the compositor asks for. Hidden (in an
//! alternate showing something else), it gets no frames and asks for none.

use std::f32::consts::PI;
use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Transformation, window};

use crate::theme;

pub struct Gulp<'a, M> {
    content: Element<'a, M>,
}

impl<'a, M> Gulp<'a, M> {
    pub fn new(content: Element<'a, M>) -> Self {
        Self { content }
    }
}

#[derive(Default)]
struct State {
    /// The first frame it was on screen, when the first gulp starts.
    start: Option<Instant>,
    /// From 0 (still) to 1, as of the last frame.
    swell: f32,
}

impl State {
    fn step(&mut self, now: Instant) {
        let start = *self.start.get_or_insert(now);
        self.swell = swell(now.saturating_duration_since(start).as_secs_f32());
    }
}

/// How swollen it is `t` seconds after the first gulp: rising and falling smoothly, one gulp after
/// another.
fn swell(t: f32) -> f32 {
    (PI * t / theme::BATTERY_GULP.as_secs_f32()).sin().powi(2)
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Gulp<'_, M> {
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

    /// The content's own size: the swell is only drawn, so nothing around it moves.
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

        let swell = tree.state.downcast_ref::<State>().swell;
        let center = layout.bounds().center();
        let transformation = Transformation::translate(center.x, center.y)
            * Transformation::scale(1.0 + theme::GULP_SWELL * swell)
            * Transformation::translate(-center.x, -center.y);
        renderer.with_transformation(transformation, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
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
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            tree.state.downcast_mut::<State>().step(*now);
            shell.request_redraw();
        }
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
}

impl<'a, M: 'a> From<Gulp<'a, M>> for Element<'a, M> {
    fn from(gulp: Gulp<'a, M>) -> Self {
        Element::new(gulp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gulps_over_and_over() {
        let gulp = theme::BATTERY_GULP.as_secs_f32();
        assert_eq!(swell(0.0), 0.0, "starts still");
        for index in 0..5 {
            let index = index as f32;
            assert!(
                swell((index + 0.5) * gulp) > 0.99,
                "full at each gulp's height"
            );
            assert!(swell((index + 1.0) * gulp) < 0.01, "back between gulps");
        }
    }

    #[test]
    fn starts_gulping_on_its_first_frame() {
        let mut state = State::default();
        let start = Instant::now();
        state.step(start);
        assert_eq!(state.swell, 0.0);
        state.step(start + theme::BATTERY_GULP / 2);
        assert!(state.swell > 0.99);
    }
}
