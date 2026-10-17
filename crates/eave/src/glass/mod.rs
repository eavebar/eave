//! Glass: the islands as liquid glass, drawn by the hypr-eave plugin in Hyprland (`hypr-eave/`),
//! which eave talks to over a private Wayland protocol (`protocol/eave-glass-v1.xml`).
//!
//! Glass is a theme, `theme = "glass"`. eave draws its islands as the dark theme would, opaque.
//! With each frame it tells the plugin which islands may turn to glass, in which fill color and
//! which way their ink goes, and which pictures never do ([`frame`]); the plugin lets the frosted,
//! refracted backdrop through where an island is its fill, and turns grey ink into a readable
//! opposite of the glass behind it. The plugin tells eave whether there's glass, and how bright it
//! is behind the bar ([`tone`]), so the ink over something bright can turn dark. Without the
//! plugin there's no glass to tell, and the theme looks dark.

pub(crate) mod client;
pub mod frame;
mod protocol;
pub mod tone;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub use client::{Event as GlassEvent, events};

use crate::config;

/// Last in the bar: sends what the frame said about glass before the frame is committed. From
/// an overlay of its own, since islands with popovers draw theirs in overlays, which come after
/// the rest of the bar; it's the last of them.
pub struct Present {
    /// `None` without the glass theme.
    look: Option<config::Glass>,
    /// What's drawn outside the islands that's never glass: the notch and the grown edge.
    solid: Vec<Rectangle>,
    /// The notch and the grown edge, exactly: the pointer goes under them, in every theme.
    bezel: Vec<Rectangle>,
    /// Whether this is the first bar's frame: the one with glass. Records are gathered for every
    /// bar's frame alike, and the others' are dropped, so they can't land on its glass.
    primary: bool,
}

impl Present {
    pub fn new(
        look: Option<config::Glass>,
        solid: Vec<Rectangle>,
        bezel: Vec<Rectangle>,
        primary: bool,
    ) -> Self {
        Self {
            look,
            solid,
            bezel,
            primary,
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
            bezel: &self.bezel,
            primary: self.primary,
        })))
    }
}

struct Sender<'a> {
    look: Option<config::Glass>,
    solid: &'a [Rectangle],
    bezel: &'a [Rectangle],
    primary: bool,
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
        for rect in self.bezel {
            frame::bezel(*rect);
        }
        let pictures = frame::take_pictures();
        let mut records = frame::take();
        // A compositor from before pictures (version 4) keeps them out of the glass as solids.
        if !client::pictures() {
            records.extend(frame::solids(&pictures));
        }
        let records = frame::fit(records);
        let reliefs = frame::fit_reliefs(frame::take_reliefs(), &records);
        let pictures = if client::pictures() {
            frame::fit_pictures(pictures, &records)
        } else {
            Vec::new()
        };
        if self.primary {
            client::present(self.look.as_ref(), records, reliefs, pictures);
        }
    }
}

impl<'a, M: 'a> From<Present> for Element<'a, M> {
    fn from(present: Present) -> Self {
        Element::new(present)
    }
}

/// A scrollable, so the pictures in it know how far it's scrolled ([`frame::picture`]).
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
