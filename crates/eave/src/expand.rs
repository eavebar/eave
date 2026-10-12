//! The host side of [`eave_api::ui::expand`]: the collapsed child, or the expanded one once the
//! pointer has rested over it for [`theme::EXPAND_DELAY`]. It's as big as the child it shows, so
//! the item grows and the island springs to fit. The delay keeps a pointer passing over several
//! of them on its way somewhere from opening each one and shifting everything after it.
//!
//! Its whole width counts, from the top of the bar down to its bottom edge, so a pointer flung
//! against the screen edge still hovers it.

use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Size, window};

use crate::theme;

pub struct Expand<'a, M> {
    /// Collapsed, then expanded.
    children: [Element<'a, M>; 2],
}

impl<'a, M> Expand<'a, M> {
    pub fn new(collapsed: Element<'a, M>, expanded: Element<'a, M>) -> Self {
        Self {
            children: [collapsed, expanded],
        }
    }
}

#[derive(Debug, Default)]
struct State {
    /// Since when the pointer has been over it, while it is.
    hovered_since: Option<Instant>,
    /// Whether it should show the expanded child.
    expanded: bool,
    /// Which child the current layout is of. It trails `expanded` until the next layout.
    laid_out: usize,
}

impl State {
    /// Follows the pointer: `over` it or not at `now`. True when what it shows changes.
    fn hover(&mut self, over: bool, now: Instant) -> bool {
        match (over, self.hovered_since) {
            (true, None) => {
                self.hovered_since = Some(now);
                false
            }
            (true, Some(since)) if !self.expanded && now >= since + theme::EXPAND_DELAY => {
                self.expanded = true;
                true
            }
            (false, _) => {
                self.hovered_since = None;
                std::mem::take(&mut self.expanded)
            }
            _ => false,
        }
    }
}

fn is_over(bounds: Rectangle, cursor: mouse::Cursor) -> bool {
    cursor.position().is_some_and(|point| {
        point.x >= bounds.x
            && point.x < bounds.x + bounds.width
            && point.y < bounds.y + bounds.height
    })
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Expand<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let index = usize::from(state.expanded);
        state.laid_out = index;
        let child = self.children[index].as_widget_mut().layout(
            &mut tree.children[index],
            renderer,
            limits,
        );
        layout::Node::with_children(child.size(), vec![child])
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
        let index = tree.state.downcast_ref::<State>().laid_out;
        if let Some(child) = layout.children().next() {
            self.children[index].as_widget().draw(
                &tree.children[index],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
            );
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let index = tree.state.downcast_ref::<State>().laid_out;
        if let Some(child) = layout.children().next() {
            self.children[index].as_widget_mut().operate(
                &mut tree.children[index],
                child,
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
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => Some(*now),
            Event::Mouse(_) => Some(Instant::now()),
            _ => None,
        };
        let state = tree.state.downcast_mut::<State>();
        if let Some(now) = now {
            let waiting = state.hovered_since.is_none();
            if state.hover(is_over(layout.bounds(), cursor), now) {
                shell.invalidate_layout();
                shell.request_redraw();
            } else if waiting && let Some(since) = state.hovered_since {
                shell.request_redraw_at(since + theme::EXPAND_DELAY);
            }
        }
        // The layout is of the old child until the next one: let it have the event.
        let index = state.laid_out;
        if let Some(child) = layout.children().next() {
            self.children[index].as_widget_mut().update(
                &mut tree.children[index],
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
        let index = tree.state.downcast_ref::<State>().laid_out;
        layout
            .children()
            .next()
            .map(|child| {
                self.children[index].as_widget().mouse_interaction(
                    &tree.children[index],
                    child,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .unwrap_or_default()
    }
}

impl<'a, M: 'a> From<Expand<'a, M>> for Element<'a, M> {
    fn from(expand: Expand<'a, M>) -> Self {
        Element::new(expand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_after_resting_and_collapses_at_once() {
        let mut state = State::default();
        let start = Instant::now();
        assert!(!state.hover(true, start), "not at once");
        assert!(!state.hover(true, start + theme::EXPAND_DELAY / 2));
        assert!(state.hover(true, start + theme::EXPAND_DELAY));
        assert!(state.expanded);
        assert!(
            !state.hover(true, start + theme::EXPAND_DELAY * 2),
            "already"
        );
        assert!(state.hover(false, start + theme::EXPAND_DELAY * 2));
        assert!(!state.expanded);
        assert!(!state.hover(false, start + theme::EXPAND_DELAY * 3));
    }

    #[test]
    fn passing_over_never_expands() {
        let mut state = State::default();
        let start = Instant::now();
        state.hover(true, start);
        state.hover(false, start + theme::EXPAND_DELAY / 2);
        assert!(!state.hover(true, start + theme::EXPAND_DELAY));
        assert!(!state.expanded, "the wait starts over");
    }
}
