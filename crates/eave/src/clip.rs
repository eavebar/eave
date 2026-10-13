//! A line of text that may be too long for its room. iced can't put an ellipsis on text, so it's
//! cut at the edge of its room, and its last few pixels fade into the background behind it. Short
//! text is left as it is.

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Widget, layout, mouse, renderer};
use iced::gradient::Linear;
use iced::{Background, Color, Element, Length, Point, Radians, Rectangle, Size};

use crate::theme;

pub struct Clip<'a, M> {
    content: Element<'a, M>,
    /// What's behind the text, to fade into.
    background: Color,
}

impl<'a, M> Clip<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, background: Color) -> Self {
        Self {
            content: content.into(),
            background,
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Clip<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Laid out as wide as it wants to be, then cut to the room there is.
        let max = limits.max();
        let natural = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, max.height));
        let content =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &natural);
        let width = max.width.min(content.size().width);
        layout::Node::with_children(Size::new(width, content.size().height), vec![content])
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

        let bounds = layout.bounds();
        let Some(content) = layout.children().next() else {
            return;
        };
        renderer.with_layer(bounds, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                content,
                cursor,
                &bounds.intersection(viewport).unwrap_or(bounds),
            );
        });
        if content.bounds().width > bounds.width + 0.5 {
            let fade = Rectangle::new(
                Point::new(bounds.x + bounds.width - theme::TEXT_FADE, bounds.y),
                Size::new(theme::TEXT_FADE, bounds.height),
            );
            let gradient = Linear::new(Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(
                    0.0,
                    Color {
                        a: 0.0,
                        ..self.background
                    },
                )
                .add_stop(1.0, self.background);
            // A layer of its own, so it lands over the text.
            renderer.with_layer(fade, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: fade,
                        ..renderer::Quad::default()
                    },
                    Background::Gradient(gradient.into()),
                );
            });
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }
}

/// Text that may run longer than `max` high: laid out in full, cut at `max`, and its last lines
/// fading into the background behind it, so a cut list or paragraph doesn't leave a hole.
pub struct Fold<'a, M> {
    content: Element<'a, M>,
    background: Color,
    max: f32,
}

impl<'a, M> Fold<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, background: Color, max: f32) -> Self {
        Self {
            content: content.into(),
            background,
            max,
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Fold<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let natural = layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
        let content =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &natural);
        let height = content.size().height.min(self.max);
        layout::Node::with_children(Size::new(width, height), vec![content])
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

        let bounds = layout.bounds();
        let Some(content) = layout.children().next() else {
            return;
        };
        renderer.with_layer(bounds, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                content,
                cursor,
                &bounds.intersection(viewport).unwrap_or(bounds),
            );
        });
        if content.bounds().height > bounds.height + 0.5 {
            let height = theme::FOLD_FADE.min(bounds.height);
            let fade = Rectangle::new(
                Point::new(bounds.x, bounds.y + bounds.height - height),
                Size::new(bounds.width, height),
            );
            let gradient = Linear::new(Radians(std::f32::consts::PI))
                .add_stop(
                    0.0,
                    Color {
                        a: 0.0,
                        ..self.background
                    },
                )
                .add_stop(1.0, self.background);
            renderer.with_layer(fade, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: fade,
                        ..renderer::Quad::default()
                    },
                    Background::Gradient(gradient.into()),
                );
            });
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }
}

impl<'a, M: 'a> From<Fold<'a, M>> for Element<'a, M> {
    fn from(fold: Fold<'a, M>) -> Self {
        Element::new(fold)
    }
}

impl<'a, M: 'a> From<Clip<'a, M>> for Element<'a, M> {
    fn from(clip: Clip<'a, M>) -> Self {
        Element::new(clip)
    }
}
