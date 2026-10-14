//! The host side of [`eave_api::ui::text_area`]: iced's text editor, keeping its own content.
//!
//! iced's editor borrows a `Content` from the app, but plugins hand over a plain string with every
//! view. So the content lives in this widget's tree state: it follows what the editor does, sends
//! the plugin the new text after each edit, and takes the plugin's text again when it differs
//! (cleared after sending, say). Enter submits and Shift+Enter breaks the line.

use iced::advanced::text::highlighter::PlainText;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::keyboard::{self, key};
use iced::widget::text_editor::{self, Action, Binding, Content, Motion, TextEditor};
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Size, Theme};

use crate::theme;

/// Lines it grows to before scrolling.
const MAX_LINES: f32 = 6.0;

pub struct TextArea<'a, M> {
    placeholder: String,
    value: String,
    on_input: Box<dyn Fn(String) -> M + 'a>,
    on_submit: Option<M>,
}

impl<'a, M> TextArea<'a, M> {
    pub fn new(
        placeholder: String,
        value: String,
        on_input: impl Fn(String) -> M + 'a,
        on_submit: Option<M>,
    ) -> Self {
        Self {
            placeholder,
            value,
            on_input: Box::new(on_input),
            on_submit,
        }
    }

    fn editor<'b>(&'b self, content: &'b Content) -> TextEditor<'b, PlainText, Inner> {
        let line = (theme::TEXT_SIZE * 1.3).ceil();
        let submits = self.on_submit.is_some();
        TextEditor::new(content)
            .placeholder(self.placeholder.as_str())
            .size(theme::TEXT_SIZE)
            .padding([7, 10])
            .max_height(line * MAX_LINES + 14.0)
            .style(style)
            .key_binding(move |press| {
                // Esc is the bar's: it closes the popover, or cancels a notification's answer,
                // as with a one-line field (D-41).
                if matches!(press.key, keyboard::Key::Named(key::Named::Escape)) {
                    return None;
                }
                let enter = matches!(press.key, keyboard::Key::Named(key::Named::Enter));
                if enter && submits && !press.modifiers.shift() {
                    return Some(Binding::Custom(Inner::Submit));
                }
                if enter {
                    return Some(Binding::Enter);
                }
                Binding::from_key_press(press)
            })
            .on_action(Inner::Action)
    }
}

#[derive(Debug, Clone)]
enum Inner {
    Action(Action),
    Submit,
}

struct State {
    content: Content,
}

impl State {
    fn new(text: &str) -> Self {
        let mut content = Content::with_text(text);
        content.perform(Action::Move(Motion::DocumentEnd));
        Self { content }
    }
}

impl<M: Clone> Widget<M, Theme, iced::Renderer> for TextArea<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(&self.value))
    }

    fn children(&self) -> Vec<Tree> {
        let content = Content::new();
        let editor: Element<'_, Inner> = self.editor(&content).into();
        vec![Tree::new(&editor)]
    }

    fn diff(&self, tree: &mut Tree) {
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_mut::<State>();
        // The plugin changed the text itself: take it, cursor at the end.
        if state.content.text() != self.value {
            *state = State::new(&self.value);
        }
        let editor: Element<'_, Inner> = self.editor(&state.content).into();
        children[0].diff(&editor);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_ref::<State>();
        self.editor(&state.content)
            .layout(&mut children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        self.editor(&state.content).draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
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
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_ref::<State>();
        self.editor(&state.content)
            .operate(&mut children[0], layout, renderer, operation);
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
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_mut::<State>();
        let mut inner = Vec::new();
        {
            let mut local = Shell::new(&mut inner);
            self.editor(&state.content).update(
                &mut children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                &mut local,
                viewport,
            );
            if local.is_event_captured() {
                shell.capture_event();
            }
            shell.request_redraw_at(local.redraw_request());
            if local.is_layout_invalid() {
                shell.invalidate_layout();
            }
            shell.request_input_method(local.input_method());
        }
        let mut edited = false;
        for message in inner {
            match message {
                Inner::Action(action) => {
                    edited |= action.is_edit();
                    state.content.perform(action);
                    shell.request_redraw();
                }
                Inner::Submit => {
                    if let Some(message) = &self.on_submit {
                        shell.publish(message.clone());
                    }
                }
            }
        }
        if edited {
            let text = state.content.text();
            self.value.clone_from(&text);
            shell.publish((self.on_input)(text));
            // It may have grown or shrunk a line.
            shell.invalidate_layout();
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
        let state = tree.state.downcast_ref::<State>();
        self.editor(&state.content).mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
}

impl<'a, M: Clone + 'a> From<TextArea<'a, M>> for Element<'a, M> {
    fn from(area: TextArea<'a, M>) -> Self {
        Element::new(area)
    }
}

/// Plain inside the [`Field`](crate::field::Field) that draws the box around it.
fn style(theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    let p = crate::theme::Palette::of(theme);
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: p.placeholder,
        value: p.normal,
        selection: p.selection,
    }
}
