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
//! [`SetHovered`] when it changes.

use std::any::Any;

use std::time::{Duration, Instant};

use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, window};

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

/// Tells every alternate it reaches whether the pointer is over its item.
pub struct SetHovered(pub bool);

impl Operation for SetHovered {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<State>() {
            state.hovered = self.0;
        }
    }
}

#[derive(Debug, Default)]
struct State {
    current: Option<usize>,
    /// Whether the pointer is over the item, as the slot last said.
    hovered: bool,
    /// The one fading out, while it does.
    previous: Option<usize>,
    /// Changed, and waiting for a frame to start fading on.
    pending: bool,
    start: Option<Instant>,
    elapsed: Duration,
}

impl State {
    fn set(&mut self, current: usize) {
        if let Some(was) = self.current
            && was != current
        {
            // Changing again mid-fade starts from what's on screen. If that's the one asked for
            // now, it just stays.
            let on_screen = self.shown(was).0;
            self.start = None;
            if on_screen == current {
                self.previous = None;
                self.pending = false;
            } else {
                self.previous = Some(on_screen);
                self.pending = true;
            }
        }
        self.current = Some(current);
    }

    fn fading(&self) -> bool {
        self.previous.is_some() && (self.pending || self.start.is_some())
    }

    /// How far the fade has come, from 0 to 1.
    fn progress(&self) -> f32 {
        if self.pending {
            return 0.0;
        }
        (self.elapsed.as_secs_f32() / theme::ALTERNATE_FADE.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Which option is on screen, and how much it shows: the old one in the first half of a fade,
    /// the new one in the second.
    fn shown(&self, current: usize) -> (usize, f32) {
        match self.previous.filter(|_| self.fading()) {
            Some(previous) => {
                let t = self.progress();
                if t < 0.5 {
                    (previous, 1.0 - 2.0 * t)
                } else {
                    (current, 2.0 * t - 1.0)
                }
            }
            None => (current, 1.0),
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
        if self.elapsed >= theme::ALTERNATE_FADE {
            self.start = None;
            self.previous = None;
            return false;
        }
        true
    }
}

impl<M> Alternate<'_, M> {
    /// The option on screen, with how much it shows. An option that's gone shows the current one.
    fn shown(&self, tree: &Tree) -> (usize, f32) {
        let state = tree.state.downcast_ref::<State>();
        let target = self.target(state.hovered);
        let (index, shown) = state.shown(target);
        if index < self.options.len() {
            (index, shown)
        } else {
            (target, 1.0)
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
        let (index, shown) = self.shown(tree);
        let Some(child) = layout.children().nth(index) else {
            return;
        };
        self.options[index].as_widget().draw(
            &tree.children[index],
            renderer,
            theme,
            style,
            child,
            cursor,
            viewport,
        );
        // A little past the option's bounds, for glyphs that reach out of them.
        let cover = child.bounds().expand(2.0);
        island::fade(renderer, cover, cover, shown);
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
        let (index, _) = self.shown(tree);
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
        let (index, _) = self.shown(tree);
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
        let (index, _) = self.shown(tree);
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

    #[test]
    fn fades_out_then_in() {
        let mut state = State {
            current: Some(0),
            ..State::default()
        };
        state.set(1);
        assert_eq!(state.shown(1), (0, 1.0), "the old one, until a frame comes");
        let start = Instant::now();
        assert!(state.step(start));
        let quarter = theme::ALTERNATE_FADE / 4;
        state.step(start + quarter);
        let (index, shown) = state.shown(1);
        assert_eq!(index, 0);
        assert!((shown - 0.5).abs() < 0.01, "{shown}");
        state.step(start + quarter * 3);
        let (index, shown) = state.shown(1);
        assert_eq!(index, 1);
        assert!((shown - 0.5).abs() < 0.01, "{shown}");
        assert!(!state.step(start + theme::ALTERNATE_FADE));
        assert_eq!(state.shown(1), (1, 1.0));
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
        assert_eq!(state.shown(0), (0, 1.0));
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
        SetHovered(true).custom(None, Rectangle::default(), state);
        state.set(alternate.target(state.hovered));
        assert_eq!(state.current, Some(1));
        assert!(state.fading());
        SetHovered(false).custom(None, Rectangle::default(), state);
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
