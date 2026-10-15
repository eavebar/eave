//! The host side of [`eave_api::ui::page`]: one page of a sequence the plugin moves through, like
//! a list and one of its rows' details, or a calendar's months. When the page's order changes, it
//! hands over the way tabs do: the old page slides away and fades, then the new one comes in from
//! the side it's on. Higher orders are to the right.
//!
//! The plugin only ever sends the page it's on, so the old one has to be kept to be drawn while it
//! goes. Each view's content moves into the widget state as it's diffed, along with its own tree.
//! A new order keeps the old content there until the hand-over is done. The same order replaces
//! it, diffed against its tree, so a page keeps its state (a scrolled list, a field) while it
//! changes.

use std::cell::RefCell;

use eave_api::erased::AnyMessage;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Padding, Rectangle, Size, Vector, window};

use crate::handover::Handover;
use crate::island;
use crate::theme;

type Content = Element<'static, AnyMessage>;

pub struct Page {
    order: i64,
    /// This view's content, until it moves into the widget state.
    content: RefCell<Option<Content>>,
    size: Size<Length>,
}

impl Page {
    pub fn new(order: i64, content: Content) -> Self {
        let size = content.as_widget().size();
        Self {
            order,
            content: RefCell::new(Some(content)),
            size: Size::new(size.width, Length::Shrink),
        }
    }
}

/// A page on screen, or going.
struct Shown {
    order: i64,
    /// Which one it is to the hand-over: every page that comes gets the next.
    index: usize,
    content: Content,
    tree: Tree,
}

impl Shown {
    fn new(order: i64, index: usize, content: Content) -> Self {
        let tree = Tree::new(content.as_widget());
        Self {
            order,
            index,
            content,
            tree,
        }
    }
}

struct State {
    current: Shown,
    /// The page going out, while it does.
    previous: Option<Shown>,
    pages: Handover,
}

impl State {
    fn new(order: i64, content: Content) -> Self {
        Self {
            current: Shown::new(order, 0, content),
            previous: None,
            pages: Handover::new(Some(0), theme::TAB_FADE),
        }
    }

    /// Takes in this view's content: the same page changes in place; another hands over to it.
    fn take(&mut self, order: i64, content: Content) {
        if order == self.current.order {
            self.current.tree.diff(content.as_widget());
            self.current.content = content;
            return;
        }
        let toward = away(self.current.order, order);
        match self.previous.take() {
            // Back to the one still on its way: the two trade places, and it turns around.
            Some(mut previous) if previous.order == order => {
                previous.tree.diff(content.as_widget());
                previous.content = content;
                self.previous = Some(std::mem::replace(&mut self.current, previous));
            }
            previous => {
                let index = self.current.index.max(previous.as_ref().map_or(0, |p| p.index)) + 1;
                let new = Shown::new(order, index, content);
                // One still going out stays going; the one that hadn't come in yet never does.
                let leaving = previous.filter(|previous| self.leaving() == Some(previous.index));
                let current = std::mem::replace(&mut self.current, new);
                self.previous = Some(leaving.unwrap_or(current));
            }
        }
        self.pages.set(self.current.index, toward);
    }

    /// The index of the page on screen going out, if one is.
    fn leaving(&self) -> Option<usize> {
        self.pages
            .leaving()
            .filter(|_| self.pages.shown(self.current.index).0 != self.current.index)
    }

    /// The page going out, while it's still there to draw.
    fn going(&self) -> Option<&Shown> {
        let index = self.pages.leaving()?;
        self.previous.as_ref().filter(|previous| previous.index == index)
    }
}

/// The way the old page leaves going from `from` to `to`: away from the new one, which comes in
/// from its own side.
fn away(from: i64, to: i64) -> Vector {
    Vector::new(if to > from { -1.0 } else { 1.0 }, 0.0)
}

impl Widget<AnyMessage, iced::Theme, iced::Renderer> for Page {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let content = self
            .content
            .borrow_mut()
            .take()
            .unwrap_or_else(|| iced::widget::Space::new().into());
        tree::State::new(State::new(self.order, content))
    }

    fn size(&self) -> Size<Length> {
        self.size
    }

    fn diff(&self, tree: &mut Tree) {
        if let Some(content) = self.content.borrow_mut().take() {
            tree.state.downcast_mut::<State>().take(self.order, content);
        }
    }

    /// The new page's size at once, so the panel's shape eases to it while the old page goes. The
    /// old one is laid out too, after it, until it's gone.
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let current = &mut state.current;
        let node = current
            .content
            .as_widget_mut()
            .layout(&mut current.tree, renderer, limits);
        let size = node.size();
        let mut children = vec![node];
        let going = state.pages.leaving();
        if let Some(previous) = state
            .previous
            .as_mut()
            .filter(|previous| Some(previous.index) == going)
        {
            children.push(
                previous
                    .content
                    .as_widget_mut()
                    .layout(&mut previous.tree, renderer, limits),
            );
        }
        layout::Node::with_children(size, children)
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
        let (index, shown, slide) = state.pages.shown(state.current.index);
        let (page, place) = match state.going() {
            Some(going) if going.index == index => (going, layout.children().nth(1)),
            _ => (&state.current, layout.children().next()),
        };
        let Some(place) = place else {
            return;
        };
        let draw = |renderer: &mut iced::Renderer| {
            page.content
                .as_widget()
                .draw(&page.tree, renderer, theme, style, place, cursor, viewport);
        };
        if shown >= 1.0 && slide == Vector::ZERO {
            draw(renderer);
            return;
        }
        // Clipped to where the slide can reach, within what the shape shows: a layer isn't
        // clipped by the ones around it.
        let slide = slide * theme::TAB_SLIDE;
        let cover = place.bounds().expand(2.0);
        let Some(reach) = cover
            .expand(Padding::from([0.0, theme::TAB_SLIDE]))
            .intersection(viewport)
        else {
            return;
        };
        renderer.with_layer(reach, |renderer| {
            renderer.with_translation(slide, |renderer| {
                draw(renderer);
                island::fade(renderer, theme, cover, reach - slide, shown);
            });
        });
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let current = &mut tree.state.downcast_mut::<State>().current;
        let Some(place) = layout.children().next() else {
            return;
        };
        current
            .content
            .as_widget_mut()
            .operate(&mut current.tree, place, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, AnyMessage>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let going = state.pages.leaving().is_some();
            if state.pages.step(*now) {
                shell.request_redraw();
            } else if going {
                // Gone: it's laid out no more.
                state.previous = None;
                shell.invalidate_layout();
            }
        }
        // Only the page coming in takes the pointer and keys.
        let Some(place) = layout.children().next() else {
            return;
        };
        let current = &mut state.current;
        current.content.as_widget_mut().update(
            &mut current.tree,
            event,
            place,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let current = &tree.state.downcast_ref::<State>().current;
        layout
            .children()
            .next()
            .map(|place| {
                current.content.as_widget().mouse_interaction(
                    &current.tree,
                    place,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .unwrap_or_default()
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, AnyMessage, iced::Theme, iced::Renderer>> {
        let current = &mut tree.state.downcast_mut::<State>().current;
        let place = layout.children().next()?;
        current.content.as_widget_mut().overlay(
            &mut current.tree,
            place,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Page> for Element<'a, AnyMessage> {
    fn from(page: Page) -> Self {
        Element::new(page)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn tall(height: f32) -> Content {
        iced::widget::Space::new().width(10).height(height).into()
    }

    fn lay_out(page: &mut Page, tree: &mut Tree) -> layout::Node {
        let renderer = crate::test_renderer();
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 1000.0));
        page.layout(tree, &renderer, &limits)
    }

    #[test]
    fn a_higher_order_comes_in_from_the_right() {
        assert_eq!(away(0, 1), Vector::new(-1.0, 0.0), "the old one out left");
        assert_eq!(away(5, 2), Vector::new(1.0, 0.0), "back: out right");
    }

    #[test]
    fn another_page_keeps_the_old_one_until_it_is_gone() {
        let mut page = Page::new(0, tall(100.0));
        let mut tree = Tree::new(&page as &dyn Widget<_, _, _>);
        assert_eq!(lay_out(&mut page, &mut tree).children().len(), 1);

        // The same page: it changes in place.
        let mut page = Page::new(0, tall(120.0));
        page.diff(&mut tree);
        let node = lay_out(&mut page, &mut tree);
        assert_eq!((node.size().height, node.children().len()), (120.0, 1));

        // Another: the new one's height at once, the old one laid out while it goes.
        let mut page = Page::new(1, tall(40.0));
        page.diff(&mut tree);
        let node = lay_out(&mut page, &mut tree);
        assert_eq!(node.size().height, 40.0);
        assert_eq!(node.children().len(), 2);
        assert_eq!(node.children()[1].size().height, 120.0);

        let state = tree.state.downcast_mut::<State>();
        let start = Instant::now();
        state.pages.step(start);
        assert_eq!(state.pages.shown(state.current.index).0, 0, "the old one first");
        assert!(!state.pages.step(start + theme::TAB_FADE));
        state.previous = None;
        assert_eq!(lay_out(&mut page, &mut tree).children().len(), 1);
    }

    #[test]
    fn going_back_mid_way_turns_around() {
        let page = Page::new(0, tall(100.0));
        let mut tree = Tree::new(&page as &dyn Widget<_, _, _>);
        Page::new(1, tall(40.0)).diff(&mut tree);
        let state = tree.state.downcast_mut::<State>();
        let start = Instant::now();
        state.pages.step(start);
        state.pages.step(start + theme::TAB_FADE * 3 / 4);

        Page::new(0, tall(100.0)).diff(&mut tree);
        let state = tree.state.downcast_ref::<State>();
        assert_eq!(state.current.order, 0);
        assert_eq!(state.previous.as_ref().map(|p| p.order), Some(1));
        assert_eq!(state.going().map(|p| p.order), Some(1), "what's on screen goes back");
    }

    #[test]
    fn a_third_page_mid_way_lets_the_old_one_finish_going() {
        let page = Page::new(0, tall(100.0));
        let mut tree = Tree::new(&page as &dyn Widget<_, _, _>);
        Page::new(1, tall(40.0)).diff(&mut tree);
        let state = tree.state.downcast_mut::<State>();
        let start = Instant::now();
        state.pages.step(start);
        state.pages.step(start + theme::TAB_FADE / 4);

        Page::new(2, tall(60.0)).diff(&mut tree);
        let state = tree.state.downcast_ref::<State>();
        assert_eq!(state.current.order, 2);
        assert_eq!(state.going().map(|p| p.order), Some(0));
    }
}
