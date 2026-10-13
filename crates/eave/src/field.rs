//! The host side of [`eave_api::ui::text_input`]: iced's text input, in a box of our own that
//! shows whether it has the focus. It takes the focus when it first appears, so a password page
//! is ready to type into.
//!
//! Also the two widget operations the popover needs around it: focusing the first field, and
//! asking whether any field has the focus, so the popover keeps the keys while one does
//! ([D-41](../../../docs/decisions.md)).

use iced::advanced::widget::{Id, Operation, Tree, operation::Focusable, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Border, Color, Element, Event, Length, Rectangle, Size, Vector};

use crate::theme;

pub struct Field<'a, M> {
    /// A row holding the text input, and whatever trails it.
    content: Element<'a, M>,
    /// The edge's color when it isn't focused, if it should stand out (a wrong password).
    alert: Option<Color>,
}

impl<'a, M> Field<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, alert: Option<Color>) -> Self {
        Self {
            content: content.into(),
            alert,
        }
    }
}

#[derive(Default)]
struct State {
    /// Whether it has taken the focus it takes on appearing.
    focused_once: bool,
    focused: bool,
}

/// Focuses the first focusable widget it meets.
struct FocusFirst {
    done: bool,
}

impl Operation for FocusFirst {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        if !self.done {
            operate(self);
        }
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if !self.done {
            state.focus();
            self.done = true;
        }
    }
}

/// Finds out whether any focusable widget has the focus.
#[derive(Default)]
pub struct AnyFocused {
    pub found: bool,
}

impl Operation for AnyFocused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        if !self.found {
            operate(self);
        }
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        self.found |= state.is_focused();
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Field<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(theme::FIELD_HEIGHT))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fixed(theme::FIELD_HEIGHT), Size::ZERO);
        let inner = layout::Limits::new(Size::ZERO, size);
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &inner);
        let offset = Vector::new(0.0, ((size.height - content.size().height) / 2.0).round());
        layout::Node::with_children(size, vec![content.translate(offset)])
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
        let edge = match (state.focused, self.alert) {
            (_, Some(alert)) => alert,
            (true, None) => theme::FIELD_FOCUS,
            (false, None) => theme::FIELD_BORDER,
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: theme::BUTTON_RADIUS.into(),
                },
                ..renderer::Quad::default()
            },
            theme::FIELD_BG,
        );
        if let Some(content) = layout.children().next() {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                content,
                cursor,
                viewport,
            );
        }
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
        if let Some(content) = layout.children().next() {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                content,
                renderer,
                operation,
            );
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
        let Some(content) = layout.children().next() else {
            return;
        };
        let (state, children) = (tree.state.downcast_mut::<State>(), &mut tree.children);
        if !state.focused_once {
            state.focused_once = true;
            let mut focus = FocusFirst { done: false };
            self.content
                .as_widget_mut()
                .operate(&mut children[0], content, renderer, &mut focus);
            shell.request_redraw();
        }
        self.content.as_widget_mut().update(
            &mut children[0],
            event,
            content,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let mut focused = AnyFocused::default();
        self.content
            .as_widget_mut()
            .operate(&mut children[0], content, renderer, &mut focused);
        if state.focused != focused.found {
            state.focused = focused.found;
            shell.request_redraw();
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
        let Some(content) = layout.children().next() else {
            return mouse::Interaction::None;
        };
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            content,
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
        let content = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            content,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Field<'a, M>> for Element<'a, M> {
    fn from(field: Field<'a, M>) -> Self {
        Element::new(field)
    }
}
