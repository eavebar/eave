//! Glass: the islands as liquid glass, drawn by the hypr-eave plugin in Hyprland (`hypr-eave/`),
//! which eave talks to over a private Wayland protocol (`protocol/eave-glass-v1.xml`).
//!
//! eave draws its islands as it would anyway, opaque. With each frame it tells the plugin which
//! islands may turn to glass, in which fill color and tone, and which pictures never do ([`frame`]);
//! the plugin lets the frosted, refracted backdrop through where an island is its fill. The plugin
//! tells eave whether there's glass, and how bright it is behind the bar ([`tone`]), so an island
//! over something bright can turn light and keep its ink readable. Without the plugin there's no
//! glass to tell, and nothing changes.

mod client;
pub mod frame;
mod protocol;
pub mod tone;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub use client::{Event as GlassEvent, active, events};
pub use frame::Tone;

use crate::config;

/// Last in the bar: sends what the frame said about glass before the frame is committed. From
/// an overlay of its own, since islands with popovers draw theirs in overlays, which come after
/// the rest of the bar; it's the last of them.
pub struct Present {
    /// `None` while the config turns glass off.
    look: Option<config::Glass>,
    /// What's drawn outside the islands that's never glass: the notch and the grown edge.
    solid: Vec<Rectangle>,
}

impl Present {
    pub fn new(glass: &config::Glass, solid: Vec<Rectangle>) -> Self {
        Self {
            look: glass.enabled.then_some(*glass),
            solid,
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Present {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::ZERO)
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        Some(overlay::Element::new(Box::new(Sender {
            look: self.look,
            solid: &self.solid,
        })))
    }
}

struct Sender<'a> {
    look: Option<config::Glass>,
    solid: &'a [Rectangle],
}

impl<M> overlay::Overlay<M, iced::Theme, iced::Renderer> for Sender<'_> {
    fn layout(&mut self, _renderer: &iced::Renderer, _bounds: Size) -> layout::Node {
        layout::Node::new(Size::ZERO)
    }

    fn draw(
        &self,
        _renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        for rect in self.solid {
            frame::opaque(*rect);
        }
        client::present(self.look.as_ref(), frame::take());
        // For the next frame: a frame that turns glass on is drawn again once it's there.
        frame::gather(self.look.is_some() && active());
    }
}

impl<'a, M: 'a> From<Present> for Element<'a, M> {
    fn from(present: Present) -> Self {
        Element::new(present)
    }
}

/// A scrollable, so the pictures in it know how far it's scrolled ([`frame::solid`]).
pub struct Scrolled<'a, M> {
    content: Element<'a, M>,
}

impl<'a, M> Scrolled<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

impl<'a, M: 'a> From<Scrolled<'a, M>> for Element<'a, M> {
    fn from(scrolled: Scrolled<'a, M>) -> Self {
        Element::new(scrolled)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Scrolled<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
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
        frame::scrolled(layout.bounds().position(), || {
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
