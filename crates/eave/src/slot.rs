//! The wrapper around every bar item. It turns clicks the content didn't handle into messages,
//! and reports where the item is, so a popover can open under it.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Item bounds plus the width of the bar it sits in, both in logical pixels.
pub type OnPress<'a, M> = Box<dyn Fn(Rectangle, f32) -> M + 'a>;

pub struct Slot<'a, M> {
    content: Element<'a, M>,
    on_press: Option<OnPress<'a, M>>,
    on_secondary: Option<M>,
}

impl<'a, M> Slot<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>) -> Self {
        Self {
            content: content.into(),
            on_press: None,
            on_secondary: None,
        }
    }

    pub fn on_press(mut self, f: impl Fn(Rectangle, f32) -> M + 'a) -> Self {
        self.on_press = Some(Box::new(f));
        self
    }

    pub fn on_secondary(mut self, message: Option<M>) -> Self {
        self.on_secondary = message;
        self
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Slot<'_, M> {
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
        if shell.is_event_captured() || !cursor.is_over(layout.bounds()) {
            return;
        }

        let message = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => self
                .on_press
                .as_ref()
                .map(|f| f(layout.bounds(), viewport.width)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                self.on_secondary.clone()
            }
            _ => None,
        };
        if let Some(message) = message {
            shell.publish(message);
            shell.capture_event();
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
