//! The host side of [`eave_api::ui::alternate`]: one of several options at a time. When the
//! current one changes, the old one fades out and then the new one fades in. iced has no group
//! opacity, so the fades are the island's: covering with its own color. That's why one fades all
//! the way out before the other starts, instead of the two crossing. Only the option on screen
//! sees events, so a counter coming in starts counting once it shows. It's as big as its biggest
//! option, so the island around it holds still when it changes.
//!
//! An alternate with a hover option shows it while the pointer is over it. The item's
//! [`Slot`](crate::slot::Slot) is the only thing that follows the pointer: it hands the alternates
//! inside where it is, with [`SetHovered`], and each one decides. The item's whole height and
//! padding count, so a pointer flung against the screen edge still hovers it. A change from hover
//! slides with the pointer: the old option slides out that way as it fades, and the new one comes
//! in from the other side. Changing back mid-fade turns around from what's on screen, so nothing
//! jumps.
//!
//! A *growing* alternate ([`eave_api::ui::expand`]) is as wide as the option on screen instead of
//! the widest, so the item changes size at the fade's midpoint and the island springs along. That
//! moves everything after it, so it differs in two more ways: it's hovered only by the pointer
//! over its own width (any height within the item), not the whole item, and it waits for the
//! pointer to rest on it ([`theme::HOVER_REST`]) before it changes, so a pointer passing over a
//! row of them doesn't open each one. Leaving changes it back at once.

use std::any::Any;
use std::time::Instant;

use iced::advanced::Renderer as _;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, window};

use crate::handover::Handover;
use crate::island;
use crate::theme;

pub struct Alternate<'a, M> {
    options: Vec<Element<'a, M>>,
    current: usize,
    /// Shown instead of `current` while hovered.
    hovered: Option<usize>,
    /// Sized to the option on screen, hovered by its own width, after a rest.
    grows: bool,
}

impl<'a, M> Alternate<'a, M> {
    pub fn new(
        options: Vec<Element<'a, M>>,
        current: usize,
        hovered: Option<usize>,
        grows: bool,
    ) -> Self {
        let last = options.len().saturating_sub(1);
        Self {
            current: current.min(last),
            hovered: hovered.map(|hovered| hovered.min(last)),
            grows,
            options,
        }
    }

    /// The option that should be on screen, once any fade is over.
    fn target(&self, hovered: bool) -> usize {
        self.hovered.filter(|_| hovered).unwrap_or(self.current)
    }
}

/// Tells every alternate it reaches where the pointer is, and where it was, so each can tell
/// whether it's hovered and which way the pointer went.
pub struct SetHovered {
    /// The bounds of the item the alternates are in.
    pub item: Rectangle,
    pub pointer: Option<Point>,
    pub before: Option<Point>,
    pub now: Instant,
    /// Set when an alternate's hover changed, so the item needs a frame.
    pub changed: bool,
}

impl Operation for SetHovered {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<State>() {
            self.changed |= state.point(self, bounds);
        }
    }
}

/// Asks whether any alternate it reaches grows: only then does the pointer moving within an item
/// matter, and not just it coming and going.
#[derive(Default)]
pub struct HasGrowing(pub bool);

impl Operation for HasGrowing {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<State>() {
            self.0 |= state.grows;
        }
    }
}

/// Which way the pointer was going as it came onto (`entered`) or left `bounds`: a unit step
/// along the axis it moved most on. With only one position, as when it comes onto or leaves the
/// surface, the nearest edge of the item to the position inside says, relative to the item's
/// size so a wide item doesn't favor its top and bottom.
fn toward(bounds: Rectangle, before: Option<Point>, now: Option<Point>, entered: bool) -> Vector {
    if let (Some(before), Some(now)) = (before, now)
        && before != now
    {
        let moved = now - before;
        return if moved.x.abs() >= moved.y.abs() {
            Vector::new(moved.x.signum(), 0.0)
        } else {
            Vector::new(0.0, moved.y.signum())
        };
    }
    let Some(inside) = (if entered { now } else { before }) else {
        return Vector::ZERO;
    };
    let x = ((inside.x - bounds.x) / bounds.width).clamp(0.0, 1.0);
    let y = ((inside.y - bounds.y) / bounds.height).clamp(0.0, 1.0);
    // Each edge, as the way out through it.
    let edges = [
        (x, Vector::new(-1.0, 0.0)),
        (1.0 - x, Vector::new(1.0, 0.0)),
        (y, Vector::new(0.0, -1.0)),
        (1.0 - y, Vector::new(0.0, 1.0)),
    ];
    let (_, out) = edges
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("four edges");
    // Coming in through an edge goes the other way.
    if entered { -out } else { out }
}

#[derive(Debug)]
struct State {
    /// Copied from the widget, for [`SetHovered`] to see.
    grows: bool,
    /// Whether the pointer is over it, as the slot last said.
    over: bool,
    /// For a growing alternate: since when the pointer has rested on it, until it's hovered, and
    /// the way it came.
    resting: Option<(Instant, Vector)>,
    /// Whether it counts as hovered: as soon as the pointer is over it, or after a rest.
    hovered: bool,
    /// The option the last layout was sized to, for a growing one.
    laid_out: Option<usize>,
    /// The way the pointer went, until the change it caused takes it.
    toward: Option<Vector>,
    /// The option on screen, and the fade from one to the next.
    handover: Handover,
}

impl Default for State {
    fn default() -> Self {
        Self {
            grows: false,
            over: false,
            resting: None,
            hovered: false,
            laid_out: None,
            toward: None,
            handover: Handover::new(None, theme::ALTERNATE_FADE),
        }
    }
}

impl State {
    fn at(current: usize) -> Self {
        Self {
            handover: Handover::new(Some(current), theme::ALTERNATE_FADE),
            ..Self::default()
        }
    }

    /// Takes in where the pointer is: over its item, or over its own width for a growing one.
    /// Returns whether that changed anything.
    fn point(&mut self, pointer: &SetHovered, bounds: Rectangle) -> bool {
        let region = if self.grows {
            Rectangle {
                x: bounds.x,
                width: bounds.width,
                ..pointer.item
            }
        } else {
            pointer.item
        };
        let over = pointer.pointer.is_some_and(|point| {
            point.x >= region.x
                && point.x < region.x + region.width
                && point.y >= region.y
                && point.y < region.y + region.height
        });
        if over == self.over {
            return false;
        }
        self.over = over;
        let toward = toward(region, pointer.before, pointer.pointer, over);
        if self.grows && over {
            self.resting = Some((pointer.now, toward));
        } else {
            self.resting = None;
            self.hovered = over;
            self.toward = Some(toward);
        }
        true
    }

    /// For a growing alternate resting under the pointer: hovered once it has rested long
    /// enough by `now`. Returns when to look again, if not yet.
    fn rest(&mut self, now: Instant) -> Option<Instant> {
        let (since, toward) = self.resting?;
        let until = since + theme::HOVER_REST;
        if now < until {
            return Some(until);
        }
        self.resting = None;
        self.hovered = true;
        self.toward = Some(toward);
        None
    }

    fn set(&mut self, current: usize) {
        let toward = self.toward.take().unwrap_or(Vector::ZERO);
        self.handover.set(current, toward);
    }

    /// Which option is on screen, how much it shows, and how far it has slid.
    fn shown(&self, current: usize) -> (usize, f32, Vector) {
        let (index, shown, slide) = self.handover.shown(current);
        (index, shown, slid(slide))
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        self.handover.step(now)
    }
}

/// A fraction of the whole slide, in pixels.
fn slid(toward: Vector) -> Vector {
    Vector::new(
        toward.x * theme::ALTERNATE_SLIDE_X,
        toward.y * theme::ALTERNATE_SLIDE_Y,
    )
}

impl<M> Alternate<'_, M> {
    /// The option on screen, with how much it shows and how far it has slid. An option that's
    /// gone shows the current one.
    fn shown(&self, tree: &Tree) -> (usize, f32, Vector) {
        let state = tree.state.downcast_ref::<State>();
        let target = self.target(state.hovered);
        let shown = state.shown(target);
        if shown.0 < self.options.len() {
            shown
        } else {
            (target, 1.0, Vector::ZERO)
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Alternate<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            grows: self.grows,
            ..State::at(self.current)
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        self.options.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.options);
        let state = tree.state.downcast_mut::<State>();
        state.grows = self.grows;
        state.set(self.target(state.hovered));
    }

    /// As big as the biggest option, with every option centered in it. A growing one is as wide
    /// as the option on screen instead, with each option at its left edge. All of them are laid
    /// out, so each keeps up with its content while it's away.
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let nodes: Vec<_> = self
            .options
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(option, tree)| option.as_widget_mut().layout(tree, renderer, limits))
            .collect();
        let mut size = nodes.iter().fold(Size::ZERO, |size, node| {
            Size::new(
                size.width.max(node.size().width),
                size.height.max(node.size().height),
            )
        });
        if self.grows {
            let (index, ..) = self.shown(tree);
            tree.state.downcast_mut::<State>().laid_out = Some(index);
            size.width = nodes.get(index).map_or(0.0, |node| node.size().width);
        }
        let grows = self.grows;
        let children = nodes
            .into_iter()
            .map(|node| {
                let own = node.size();
                let x = if grows {
                    0.0
                } else {
                    ((size.width - own.width) / 2.0).round()
                };
                node.move_to(Point::new(x, ((size.height - own.height) / 2.0).round()))
            })
            .collect();
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
        let (index, shown, slide) = self.shown(tree);
        let Some(child) = layout.children().nth(index) else {
            return;
        };
        let draw = |renderer: &mut iced::Renderer| {
            self.options[index].as_widget().draw(
                &tree.children[index],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
            );
        };
        // A little past the option's bounds, for glyphs that reach out of them.
        let cover = child.bounds().expand(2.0);
        if slide == Vector::ZERO {
            draw(renderer);
            island::fade(renderer, cover, cover, shown);
            return;
        }
        // Clipped to where a slide can reach, which stays inside the island.
        let reach = layout.bounds();
        let reach = Rectangle {
            x: reach.x - theme::ALTERNATE_SLIDE_X - 2.0,
            y: reach.y - theme::ALTERNATE_SLIDE_Y,
            width: reach.width + 2.0 * (theme::ALTERNATE_SLIDE_X + 2.0),
            height: reach.height + 2.0 * theme::ALTERNATE_SLIDE_Y,
        };
        renderer.with_layer(reach, |renderer| {
            renderer.with_translation(slide, |renderer| {
                draw(renderer);
                island::fade(renderer, cover, reach - slide, shown);
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
        let state = tree.state.downcast_mut::<State>();
        operation.custom(None, layout.bounds(), state);
        state.set(self.target(state.hovered));
        let (index, ..) = self.shown(tree);
        if let Some(child) = layout.children().nth(index) {
            self.options[index].as_widget_mut().operate(
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
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let target = self.hovered.unwrap_or(self.current);
            let state = tree.state.downcast_mut::<State>();
            if state.resting.is_some() {
                match state.rest(*now) {
                    Some(until) => shell.request_redraw_at(until),
                    None => state.set(target),
                }
            }
            if state.step(*now) {
                shell.request_redraw();
            }
        }
        // A growing one is sized to the option on screen: a new one there needs a new layout.
        if self.grows {
            let (index, ..) = self.shown(tree);
            if tree.state.downcast_ref::<State>().laid_out != Some(index) {
                shell.invalidate_layout();
                shell.request_redraw();
            }
        }
        // After stepping, so an option that just came on screen gets this frame too.
        let (index, ..) = self.shown(tree);
        if let Some(child) = layout.children().nth(index) {
            self.options[index].as_widget_mut().update(
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
        let (index, ..) = self.shown(tree);
        layout
            .children()
            .nth(index)
            .map(|child| {
                self.options[index].as_widget().mouse_interaction(
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

impl<'a, M: 'a> From<Alternate<'a, M>> for Element<'a, M> {
    fn from(alternate: Alternate<'a, M>) -> Self {
        Element::new(alternate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: Vector = Vector::new(1.0, 0.0);

    #[test]
    fn slides_the_way_the_pointer_went() {
        let mut state = State {
            toward: Some(RIGHT),
            ..State::at(0)
        };
        state.set(1);
        let start = Instant::now();
        state.step(start);
        state.step(start + theme::ALTERNATE_FADE / 4);
        let (_, _, out) = state.shown(1);
        assert!(
            out.x > 0.0 && out.y == 0.0,
            "the old one leaves to the right: {out:?}"
        );
        state.step(start + theme::ALTERNATE_FADE * 3 / 4);
        let (_, _, coming) = state.shown(1);
        assert!(
            coming.x < 0.0,
            "the new one comes from the left: {coming:?}"
        );
    }

    #[test]
    fn hovering_fades_to_the_hover_option_and_back() {
        let alternate: Alternate<'_, ()> = Alternate::new(
            vec![
                iced::widget::text("a").into(),
                iced::widget::text("b").into(),
            ],
            0,
            Some(1),
            false,
        );
        let mut tree = Tree::new(&alternate as &dyn Widget<_, _, _>);
        let state = tree.state.downcast_mut::<State>();
        let own = Rectangle::new(Point::new(10.0, 10.0), Size::new(10.0, 10.0));
        let mut hover = |x| {
            let mut pointer = pointer(x, x - 5.0);
            pointer.custom(None, own, state);
            state.set(alternate.target(state.hovered));
            (
                pointer.changed,
                state.handover.current,
                state.handover.running(),
                state.handover.slide,
            )
        };
        assert_eq!(
            hover(50.0),
            (true, Some(1), true, RIGHT),
            "anywhere on the item"
        );
        assert_eq!(
            hover(150.0),
            (true, Some(0), false, RIGHT),
            "left before the fade began: it just stays"
        );
        assert!(!hover(160.0).0, "still off: nothing changes");
    }

    /// The pointer at `x`, halfway down an item from 0 to 100 wide and 30 high, coming from
    /// `before`.
    fn pointer(x: f32, before: f32) -> SetHovered {
        SetHovered {
            item: Rectangle::new(Point::ORIGIN, Size::new(100.0, 30.0)),
            pointer: Some(Point::new(x, 15.0)),
            before: Some(Point::new(before, 15.0)),
            now: Instant::now(),
            changed: false,
        }
    }

    fn growing() -> State {
        State {
            grows: true,
            ..State::at(0)
        }
    }

    #[test]
    fn a_growing_one_is_hovered_by_its_own_width_at_any_height() {
        let mut state = growing();
        let own = Rectangle::new(Point::new(40.0, 10.0), Size::new(20.0, 10.0));
        assert!(!state.point(&pointer(20.0, 10.0), own), "over a neighbor");
        let mut against_the_edge = pointer(45.0, 30.0);
        against_the_edge.pointer = Some(Point::new(45.0, 0.0));
        assert!(state.point(&against_the_edge, own));
        assert!(state.over);
        assert!(!state.hovered, "not before it rests");
    }

    #[test]
    fn a_growing_one_waits_for_a_rest_and_leaves_at_once() {
        let mut state = growing();
        let own = Rectangle::new(Point::new(40.0, 0.0), Size::new(20.0, 30.0));
        let start = Instant::now();
        let mut enter = pointer(45.0, 30.0);
        enter.now = start;
        state.point(&enter, own);

        let half = start + theme::HOVER_REST / 2;
        assert_eq!(state.rest(half), Some(start + theme::HOVER_REST));
        assert!(!state.hovered);
        assert_eq!(state.rest(start + theme::HOVER_REST), None);
        assert!(state.hovered);
        state.set(1);
        assert_eq!(state.handover.slide, RIGHT, "slides the way it came in");

        state.point(&pointer(80.0, 55.0), own);
        assert!(!state.hovered, "leaving needs no rest");
        assert!(state.resting.is_none());
    }

    #[test]
    fn passing_over_a_growing_one_never_opens_it() {
        let mut state = growing();
        let own = Rectangle::new(Point::new(40.0, 0.0), Size::new(20.0, 30.0));
        let start = Instant::now();
        let mut pass = pointer(45.0, 30.0);
        pass.now = start;
        state.point(&pass, own);
        state.point(&pointer(70.0, 45.0), own);
        assert_eq!(state.rest(start + theme::HOVER_REST), None);
        assert!(!state.hovered, "the rest was cut short");
    }

    #[test]
    fn a_growing_one_is_as_wide_as_the_option_on_screen() {
        let mut alternate: Alternate<'_, ()> = Alternate::new(
            vec![
                iced::widget::Space::new().width(10).height(20).into(),
                iced::widget::Space::new().width(50).height(16).into(),
            ],
            0,
            Some(1),
            true,
        );
        let mut tree = Tree::new(&alternate as &dyn Widget<_, _, _>);
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let limits = layout::Limits::new(Size::ZERO, Size::new(500.0, 500.0));
        let size = |alternate: &mut Alternate<'_, ()>, tree: &mut Tree| {
            alternate.layout(tree, &renderer, &limits).size()
        };
        assert_eq!(size(&mut alternate, &mut tree), Size::new(10.0, 20.0));

        let state = tree.state.downcast_mut::<State>();
        state.hovered = true;
        state.set(1);
        let start = Instant::now();
        state.step(start);
        state.step(start + theme::ALTERNATE_FADE / 4);
        assert_eq!(
            size(&mut alternate, &mut tree).width,
            10.0,
            "the old one, while it fades out"
        );
        let state = tree.state.downcast_mut::<State>();
        state.step(start + theme::ALTERNATE_FADE * 3 / 4);
        assert_eq!(
            size(&mut alternate, &mut tree),
            Size::new(50.0, 20.0),
            "the new one's width, the tallest height"
        );
    }

    #[test]
    fn the_pointer_says_which_way() {
        let item = Rectangle::new(Point::new(100.0, 0.0), Size::new(60.0, 30.0));
        let at = |x, y| Some(Point::new(x, y));
        // Moving: the way it moved most.
        assert_eq!(
            toward(item, at(95.0, 14.0), at(104.0, 16.0), true),
            Vector::new(1.0, 0.0)
        );
        assert_eq!(
            toward(item, at(130.0, 2.0), at(131.0, -3.0), false),
            Vector::new(0.0, -1.0)
        );
        // Only one position: the nearest edge, relative to the size.
        assert_eq!(
            toward(item, None, at(120.0, 2.0), true),
            Vector::new(0.0, 1.0),
            "came down from the top"
        );
        assert_eq!(
            toward(item, at(158.0, 10.0), None, false),
            Vector::new(1.0, 0.0),
            "left through the right"
        );
        assert_eq!(toward(item, None, None, false), Vector::ZERO);
    }
}
