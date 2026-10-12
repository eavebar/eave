//! A label that can squish. At rest it's plain iced text. While its island squishes it
//! ([`squish`]), it's drawn as glyph outlines in a canvas frame instead, which can stretch one way
//! at a time. Outlines aren't hinted, so they're a touch softer, but only for the moment a squish
//! lasts.

use iced::advanced::text::Shaping;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::widget::canvas::{Frame, Text};
use iced::widget::text;
use iced::{Color, Element, Event, Length, Pixels, Rectangle, Size};

use crate::squish::{self, Squish};

pub struct Label<'a, M> {
    content: String,
    size: f32,
    color: Color,
    text: Element<'a, M>,
}

impl<'a, M: 'a> Label<'a, M> {
    pub fn new(content: String, size: f32, color: Color) -> Self {
        let text = text(content.clone()).size(size).color(color).into();
        Self {
            content,
            size,
            color,
            text,
        }
    }
}

/// Draws `text`, squished about the center of `bounds`, as outlines.
pub(crate) fn draw_squished(
    renderer: &mut iced::Renderer,
    squish: Squish,
    text: Text,
    bounds: Rectangle,
) {
    use iced::advanced::graphics::geometry::Renderer as _;

    // Room for the stretch to spill out of the bounds a little.
    let mut frame = Frame::with_bounds(renderer, bounds.expand(bounds.width.max(bounds.height)));
    squish.apply(&mut frame, bounds.center());
    frame.fill_text(text);
    renderer.draw_geometry(frame.into_geometry());
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Label<'_, M> {
    fn tag(&self) -> tree::Tag {
        self.text.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.text.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.text.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.text.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.text.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.text.as_widget_mut().layout(tree, renderer, limits)
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
        let squish = squish::current();
        if squish.is_none() {
            self.text
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
            return;
        }
        let bounds = layout.bounds();
        let text = Text {
            content: self.content.clone(),
            position: bounds.position(),
            color: self.color,
            size: Pixels(self.size),
            shaping: Shaping::default(),
            ..Text::default()
        };
        draw_squished(renderer, squish, text, bounds);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.text
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
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
        self.text.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }
}

impl<'a, M: 'a> From<Label<'a, M>> for Element<'a, M> {
    fn from(label: Label<'a, M>) -> Self {
        Element::new(label)
    }
}
