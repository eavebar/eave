//! Lays out a popover panel at its natural size and reports that size, so the host can size the
//! popover's surface to fit. Until the host says the surface fits, the panel is laid out but not
//! drawn, so the user never sees it clipped or in the wrong place.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Size, window};

pub struct Measure<'a, M> {
    content: Element<'a, M>,
    on_measure: fn(Size) -> M,
    visible: bool,
}

#[derive(Default)]
struct State {
    size: Size,
    reported: Option<Size>,
}

impl<'a, M> Measure<'a, M> {
    pub fn new(
        content: impl Into<Element<'a, M>>,
        on_measure: fn(Size) -> M,
        visible: bool,
    ) -> Self {
        Self {
            content: content.into(),
            on_measure,
            visible,
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Measure<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Unbounded and compressed: the content takes its natural size, whatever the surface size
        // is right now, and `Fill` children (a separator) stretch to their siblings, not to infinity.
        let natural =
            layout::Limits::with_compression(Size::ZERO, Size::INFINITE, Size::new(true, true));
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &natural);
        tree.state.downcast_mut::<State>().size = child.size();
        layout::Node::with_children(limits.max(), vec![child])
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
        if !self.visible {
            return;
        }
        let child = layout.children().next().expect("measure has one child");
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            child,
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
        let child = layout.children().next().expect("measure has one child");
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], child, renderer, operation);
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
        let state = tree.state.downcast_mut::<State>();
        if let Event::Window(window::Event::RedrawRequested(_)) = event
            && state.reported != Some(state.size)
        {
            state.reported = Some(state.size);
            shell.publish((self.on_measure)(state.size));
        }

        if self.visible {
            let child = layout.children().next().expect("measure has one child");
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                child,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
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
        if !self.visible {
            return mouse::Interaction::None;
        }
        let child = layout.children().next().expect("measure has one child");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
            cursor,
            viewport,
            renderer,
        )
    }
}

impl<'a, M: 'a> From<Measure<'a, M>> for Element<'a, M> {
    fn from(measure: Measure<'a, M>) -> Self {
        Element::new(measure)
    }
}
