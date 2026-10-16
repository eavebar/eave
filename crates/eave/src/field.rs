//! The host side of [`eave_api::ui::text_input`] and [`eave_api::ui::text_area`]: iced's text
//! input or the [`TextArea`](crate::text_area::TextArea), in a box of our own that shows whether
//! it has the focus. In a popover it takes the focus when it first appears, so a password page is
//! ready to type into. On a notification it waits for a click, and says when it gets or loses the
//! focus, so the bar takes the keyboard only then.
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
    /// As tall as its content, at least a line: a text area.
    grows: bool,
    /// Takes the focus as it appears.
    eager: bool,
    /// Told when it gets the focus, or loses it.
    on_focus: Option<Box<dyn Fn(bool) -> M + 'a>>,
    /// Its corner, nested in what it's in (D-125).
    radius: f32,
}

impl<'a, M> Field<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, alert: Option<Color>) -> Self {
        Self {
            content: content.into(),
            alert,
            grows: false,
            eager: true,
            on_focus: None,
            radius: theme::BUTTON_RADIUS,
        }
    }

    /// Rounds its corners `radius`.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// As tall as what it holds, for a text area.
    pub fn grows(mut self) -> Self {
        self.grows = true;
        self
    }

    /// Waits for a click instead of taking the focus as it appears, and sends `on_focus` when
    /// it gets the focus or loses it.
    pub fn on_focus(mut self, on_focus: impl Fn(bool) -> M + 'a) -> Self {
        self.eager = false;
        self.on_focus = Some(Box::new(on_focus));
        self
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
        let height = if self.grows {
            Length::Shrink
        } else {
            Length::Fixed(theme::FIELD_HEIGHT)
        };
        Size::new(Length::Fill, height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if self.grows {
            let width = limits
                .resolve(Length::Fill, Length::Shrink, Size::ZERO)
                .width;
            let inner = layout::Limits::new(Size::ZERO, Size::new(width, limits.max().height));
            let content =
                self.content
                    .as_widget_mut()
                    .layout(&mut tree.children[0], renderer, &inner);
            let height = content.size().height.max(theme::FIELD_HEIGHT);
            let offset = Vector::new(0.0, ((height - content.size().height) / 2.0).round());
            return layout::Node::with_children(
                Size::new(width, height),
                vec![content.translate(offset)],
            );
        }
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
        let p = crate::theme::Palette::of(theme);
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let edge = match (state.focused, self.alert) {
            (_, Some(alert)) => alert,
            (true, None) => p.field_focus,
            (false, None) => p.field_border,
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: self.radius.into(),
                },
                ..renderer::Quad::default()
            },
            p.field_bg,
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
        if self.eager && !state.focused_once {
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
            if let Some(on_focus) = &self.on_focus {
                shell.publish(on_focus(focused.found));
            }
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
