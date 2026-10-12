//! The wrapper around every bar item. It turns clicks and scrolls the content didn't handle into
//! messages. A primary click counts on release, and only if it was short: a long press is for
//! playing with the island, not for opening things.

use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub struct Slot<'a, M> {
    content: Element<'a, M>,
    on_press: Option<M>,
    /// Sent when the content itself handles a primary click, e.g. a button inside the item.
    on_inner_press: Option<M>,
    on_secondary: Option<M>,
    /// Up, then down.
    on_scroll: Option<(M, M)>,
}

#[derive(Debug, Default)]
struct State {
    scrolled: Scrolled,
    /// When the primary button went down on this item, until it comes back up.
    pressed: Option<Instant>,
}

/// Touchpad scrolling that hasn't added up to a whole step yet. Positive is up.
#[derive(Debug, Default)]
struct Scrolled(f32);

impl Scrolled {
    /// Adds `delta` and takes out the whole steps it completes.
    fn steps(&mut self, delta: mouse::ScrollDelta) -> i32 {
        self.0 += match delta {
            mouse::ScrollDelta::Lines { y, .. } => y,
            mouse::ScrollDelta::Pixels { y, .. } => y / crate::theme::SCROLL_STEP,
        };
        let steps = self.0.trunc();
        self.0 -= steps;
        steps as i32
    }
}

impl<'a, M> Slot<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>) -> Self {
        Self {
            content: content.into(),
            on_press: None,
            on_inner_press: None,
            on_secondary: None,
            on_scroll: None,
        }
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_inner_press(mut self, message: M) -> Self {
        self.on_inner_press = Some(message);
        self
    }

    pub fn on_secondary(mut self, message: Option<M>) -> Self {
        self.on_secondary = message;
        self
    }

    pub fn on_scroll(mut self, messages: Option<(M, M)>) -> Self {
        self.on_scroll = messages;
        self
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Slot<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
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
        // A release anywhere ends the press; it only counts as a click over the item, and if short.
        if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event
            && let Some(pressed) = tree.state.downcast_mut::<State>().pressed.take()
            && pressed.elapsed() < crate::theme::LONG_PRESS
            && cursor.is_over(layout.bounds())
            && let Some(on_press) = &self.on_press
        {
            shell.publish(on_press.clone());
            shell.capture_event();
            return;
        }
        if !cursor.is_over(layout.bounds()) {
            return;
        }
        if shell.is_event_captured() {
            if let (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(message)) =
                (event, &self.on_inner_press)
            {
                shell.publish(message.clone());
            }
            return;
        }

        if let (Event::Mouse(mouse::Event::WheelScrolled { delta }), Some((up, down))) =
            (event, &self.on_scroll)
        {
            let steps = tree.state.downcast_mut::<State>().scrolled.steps(*delta);
            // A flung wheel can report many lines at once; a handful of steps is plenty.
            let message = if steps > 0 { up } else { down };
            for _ in 0..steps.unsigned_abs().min(5) {
                shell.publish(message.clone());
            }
            shell.capture_event();
            return;
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if self.on_press.is_some() =>
            {
                tree.state.downcast_mut::<State>().pressed = Some(Instant::now());
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(message) = self.on_secondary.clone() {
                    shell.publish(message);
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let inner = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        let clickable = self.on_press.is_some() || self.on_secondary.is_some();
        if inner == mouse::Interaction::None && clickable && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            inner
        }
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

impl<'a, M: Clone + 'a> From<Slot<'a, M>> for Element<'a, M> {
    fn from(slot: Slot<'a, M>) -> Self {
        Element::new(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touchpad_scrolling_adds_up_to_steps() {
        let mut scrolled = Scrolled::default();
        let pixels = |y| mouse::ScrollDelta::Pixels { x: 0.0, y };
        assert_eq!(scrolled.steps(pixels(20.0)), 0);
        assert_eq!(scrolled.steps(pixels(20.0)), 1);
        assert_eq!(scrolled.steps(pixels(-50.0)), -1);
        assert_eq!(
            scrolled.steps(mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 }),
            1
        );
    }
}
