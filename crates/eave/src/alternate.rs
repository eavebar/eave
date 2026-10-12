//! The host side of [`eave_api::ui::alternate`]: one of several options at a time. When the
//! current one changes, the old one fades out and then the new one fades in. iced has no group
//! opacity, so the fades are the island's: covering with its own color. That's why one fades all
//! the way out before the other starts, instead of the two crossing. Only the option on screen
//! sees events, so a counter coming in starts counting once it shows. It's as big as its biggest
//! option, so the island around it holds still when it changes.
//!
//! An alternate with a hover option shows it while the pointer is over the item. The item's
//! [`Slot`](crate::slot::Slot) knows that, not the alternate: its whole height and padding count,
//! so a pointer flung against the screen edge still hovers it. It tells the alternates inside with
//! [`SetHovered`] when it changes, along with the way the pointer was going. A change from hover
//! slides with the pointer: the old option slides out that way as it fades, and the new one comes
//! in from the other side. Changing back mid-fade turns around from what's on screen, so nothing
//! jumps.

use std::any::Any;
use std::time::{Duration, Instant};

use iced::advanced::Renderer as _;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, window};

use crate::island;
use crate::theme;

pub struct Alternate<'a, M> {
    options: Vec<Element<'a, M>>,
    current: usize,
    /// Shown instead of `current` while the item is hovered.
    hovered: Option<usize>,
}

impl<'a, M> Alternate<'a, M> {
    pub fn new(options: Vec<Element<'a, M>>, current: usize, hovered: Option<usize>) -> Self {
        let last = options.len().saturating_sub(1);
        Self {
            current: current.min(last),
            hovered: hovered.map(|hovered| hovered.min(last)),
            options,
        }
    }

    /// The option that should be on screen, once any fade is over.
    fn target(&self, hovered: bool) -> usize {
        self.hovered.filter(|_| hovered).unwrap_or(self.current)
    }
}

/// Tells every alternate it reaches whether the pointer is over its item, and which way it was
/// going when that changed: a unit step along one axis, or zero when it isn't known.
pub struct SetHovered {
    pub hovered: bool,
    pub toward: Vector,
}

impl Operation for SetHovered {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<State>() {
            state.hovered = self.hovered;
            state.toward = Some(self.toward);
        }
    }
}

#[derive(Debug, Default)]
struct State {
    current: Option<usize>,
    /// Whether the pointer is over the item, as the slot last said.
    hovered: bool,
    /// The way the pointer went, until the change it caused takes it.
    toward: Option<Vector>,
    /// The one fading out, while it does.
    previous: Option<usize>,
    /// The way the fade slides: a unit step, or zero for a fade in place.
    slide: Vector,
    /// Where the fade starts, from 0 to 1: past 0 when it turned around.
    from: f32,
    /// Changed, and waiting for a frame to start fading on.
    pending: bool,
    start: Option<Instant>,
    elapsed: Duration,
}

impl State {
    fn set(&mut self, current: usize) {
        let toward = self.toward.take().unwrap_or(Vector::ZERO);
        let Some(was) = self.current.filter(|&was| was != current) else {
            self.current = Some(current);
            return;
        };
        self.current = Some(current);
        if !self.fading() {
            self.previous = Some(was);
            self.slide = toward;
            self.from = 0.0;
            self.pending = true;
            self.start = None;
            return;
        }
        let t = self.progress();
        let on_screen = self.shown(was).0;
        if t < 0.5 && on_screen != current {
            // Still on its way out: only what comes in next changes.
        } else if t <= 0.0 {
            // Back before anything moved.
            self.previous = None;
            self.pending = false;
            self.start = None;
        } else {
            // Turn around: what's on screen goes back the way it came, from where it is.
            self.previous = Some(if on_screen == current { was } else { on_screen });
            self.slide = -self.slide;
            self.from = 1.0 - t;
            self.pending = true;
            self.start = None;
        }
    }

    fn fading(&self) -> bool {
        self.previous.is_some() && (self.pending || self.start.is_some())
    }

    /// How far the fade has come, from 0 to 1.
    fn progress(&self) -> f32 {
        let run = if self.pending {
            0.0
        } else {
            self.elapsed.as_secs_f32() / theme::ALTERNATE_FADE.as_secs_f32()
        };
        (self.from + run).clamp(0.0, 1.0)
    }

    /// Which option is on screen, how much it shows, and how far it has slid: the old one in the
    /// first half of a fade, speeding away, the new one in the second, slowing into place.
    fn shown(&self, current: usize) -> (usize, f32, Vector) {
        match self.previous.filter(|_| self.fading()) {
            Some(previous) => {
                let t = self.progress();
                if t < 0.5 {
                    let out = 2.0 * t;
                    (previous, 1.0 - out, slid(self.slide, out * out))
                } else {
                    let rest = 2.0 - 2.0 * t;
                    (current, 1.0 - rest, slid(-self.slide, rest * rest))
                }
            }
            None => (current, 1.0, Vector::ZERO),
        }
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        if self.pending {
            self.pending = false;
            self.start = Some(now);
        }
        let Some(start) = self.start else {
            return false;
        };
        self.elapsed = now.saturating_duration_since(start);
        if self.progress() >= 1.0 {
            self.start = None;
            self.previous = None;
            self.from = 0.0;
            return false;
        }
        true
    }
}

/// `amount` of the whole slide `toward`.
fn slid(toward: Vector, amount: f32) -> Vector {
    Vector::new(
        toward.x * theme::ALTERNATE_SLIDE_X,
        toward.y * theme::ALTERNATE_SLIDE_Y,
    ) * amount
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
            current: Some(self.current),
            ..State::default()
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
        state.set(self.target(state.hovered));
    }

    /// As big as the biggest option, with every option centered in it. All of them are laid out,
    /// so each keeps up with its content while it's away.
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
        let size = nodes.iter().fold(Size::ZERO, |size, node| {
            Size::new(
                size.width.max(node.size().width),
                size.height.max(node.size().height),
            )
        });
        let children = nodes
            .into_iter()
            .map(|node| {
                let own = node.size();
                node.move_to(Point::new(
                    ((size.width - own.width) / 2.0).round(),
                    ((size.height - own.height) / 2.0).round(),
                ))
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
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && tree.state.downcast_mut::<State>().step(*now)
        {
            shell.request_redraw();
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

    fn showing(state: &State, current: usize) -> (usize, f32) {
        let (index, shown, _) = state.shown(current);
        (index, shown)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn fades_out_then_in() {
        let mut state = State {
            current: Some(0),
            ..State::default()
        };
        state.set(1);
        assert_eq!(
            showing(&state, 1),
            (0, 1.0),
            "the old one, until a frame comes"
        );
        let start = Instant::now();
        assert!(state.step(start));
        let quarter = theme::ALTERNATE_FADE / 4;
        state.step(start + quarter);
        let (index, shown, slide) = state.shown(1);
        assert_eq!(index, 0);
        assert!(close(shown, 0.5), "{shown}");
        assert_eq!(slide, Vector::ZERO, "no pointer, no slide");
        state.step(start + quarter * 3);
        let (index, shown) = showing(&state, 1);
        assert_eq!(index, 1);
        assert!(close(shown, 0.5), "{shown}");
        assert!(!state.step(start + theme::ALTERNATE_FADE));
        assert_eq!(showing(&state, 1), (1, 1.0));
    }

    #[test]
    fn slides_the_way_the_pointer_went() {
        let mut state = State {
            current: Some(0),
            toward: Some(RIGHT),
            ..State::default()
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
    fn going_back_before_the_old_one_is_gone_keeps_it() {
        let mut state = State {
            current: Some(0),
            ..State::default()
        };
        state.set(1);
        state.step(Instant::now());
        state.set(0);
        assert!(!state.fading());
        assert_eq!(showing(&state, 0), (0, 1.0));
    }

    /// Changing back mid-fade carries on from what's on screen, in either half.
    #[test]
    fn turning_around_never_jumps() {
        for fraction in [4, 2] {
            let mut state = State {
                current: Some(0),
                toward: Some(RIGHT),
                ..State::default()
            };
            state.set(1);
            let start = Instant::now();
            state.step(start);
            let now = start + theme::ALTERNATE_FADE * 3 / (fraction * 2);
            state.step(now);
            let before = state.shown(1);
            state.toward = Some(RIGHT);
            state.set(0);
            let after = state.shown(0);
            assert_eq!(before.0, after.0, "the same option on screen");
            assert!(close(before.1, after.1), "{before:?} {after:?}");
            assert!(close(before.2.x, after.2.x), "{before:?} {after:?}");
            state.step(now);
            assert!(
                close(state.shown(0).1, after.1),
                "the next frame starts there"
            );
            assert!(!state.step(now + theme::ALTERNATE_FADE));
            assert_eq!(showing(&state, 0), (0, 1.0));
        }
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
        );
        let mut tree = Tree::new(&alternate as &dyn Widget<_, _, _>);
        let state = tree.state.downcast_mut::<State>();
        let hover = |hovered| SetHovered {
            hovered,
            toward: RIGHT,
        };
        hover(true).custom(None, Rectangle::default(), state);
        state.set(alternate.target(state.hovered));
        assert_eq!(state.current, Some(1));
        assert!(state.fading());
        assert_eq!(state.slide, RIGHT);
        hover(false).custom(None, Rectangle::default(), state);
        state.set(alternate.target(state.hovered));
        assert_eq!(state.current, Some(0));
        assert!(!state.fading(), "left before the fade began: it just stays");
    }

    #[test]
    fn the_same_one_again_changes_nothing() {
        let mut state = State {
            current: Some(1),
            ..State::default()
        };
        state.set(1);
        assert!(!state.fading());
    }
}
