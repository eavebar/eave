//! A notification's questions, asked one at a time on the same card ([`eave_api::ui::Notice::step`]).
//! When the step changes, the old one slides away and fades out, then the new one slides in from
//! the other side, like tabs' pages. Unlike tabs, the widget doesn't hold every step: only the one
//! asked now, and, while it leaves, the one before, which the host keeps for as long as that takes.
//! The new step sets the height at once, so the panel's shape eases to it while the old one goes.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::Space;
use iced::{Element, Event, Length, Padding, Rectangle, Size, Vector, window};

use crate::handover::Handover;
use crate::island;
use crate::theme;

pub struct Steps<'a, M> {
    /// The step asked now.
    index: usize,
    current: Element<'a, M>,
    /// The step before, and what it showed, while it may still be leaving.
    previous: Option<(usize, Element<'a, M>)>,
    /// Stands in for the step before when there's none, so the current one keeps its place in
    /// the tree, and a field in it keeps what's typed.
    placeholder: Element<'a, M>,
}

impl<'a, M: 'a> Steps<'a, M> {
    pub fn new(
        index: usize,
        current: impl Into<Element<'a, M>>,
        previous: Option<(usize, Element<'a, M>)>,
    ) -> Self {
        Self {
            index,
            current: current.into(),
            previous: previous.filter(|(at, _)| *at != index),
            placeholder: Space::new().into(),
        }
    }
}

struct State {
    handover: Handover,
}

/// The way the old step leaves going from `from` to `to`: forward slides left, back right.
fn away(from: usize, to: usize) -> Vector {
    Vector::new(if to > from { -1.0 } else { 1.0 }, 0.0)
}

impl<M> Steps<'_, M> {
    fn previous_element(&self) -> &Element<'_, M> {
        match &self.previous {
            Some((_, element)) => element,
            None => &self.placeholder,
        }
    }

    /// The step on its way out, if the host still has it.
    fn leaving(&self, tree: &Tree) -> Option<usize> {
        let state = tree.state.downcast_ref::<State>();
        let leaving = state.handover.leaving()?;
        self.previous
            .as_ref()
            .filter(|(at, _)| *at == leaving)
            .map(|(at, _)| *at)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Steps<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            handover: Handover::new(Some(self.index), theme::TAB_FADE),
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.current), Tree::new(self.previous_element())]
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if let Some(was) = state.handover.current
            && was != self.index
        {
            state.handover.set(self.index, away(was, self.index));
        }
        tree.diff_children(&[&self.current, self.previous_element()]);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fill);
        let current = self
            .current
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);
        let width = limits.max().width;
        let width = if width.is_finite() {
            width
        } else {
            current.size().width
        };
        let size = Size::new(width, current.size().height);
        let mut children = vec![current];
        if self.leaving(tree).is_some()
            && let Some((_, previous)) = &mut self.previous
        {
            children.push(previous.as_widget_mut().layout(
                &mut tree.children[1],
                renderer,
                &limits,
            ));
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
        let (index, shown, slide) = state.handover.shown(self.index);
        let leaving = self.leaving(tree);
        let (element, child, place) = match (leaving, &self.previous) {
            (Some(at), Some((_, previous))) if at == index => {
                (previous, &tree.children[1], layout.children().nth(1))
            }
            _ => (&self.current, &tree.children[0], layout.children().next()),
        };
        let Some(place) = place else {
            return;
        };
        let draw = |renderer: &mut iced::Renderer| {
            element
                .as_widget()
                .draw(child, renderer, theme, style, place, cursor, viewport);
        };
        if shown >= 1.0 && slide == Vector::ZERO {
            draw(renderer);
            return;
        }
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
        // Only the step asked now: a field in the one leaving can't take the focus.
        if let Some(place) = layout.children().next() {
            self.current
                .as_widget_mut()
                .operate(&mut tree.children[0], place, renderer, operation);
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
            let state = tree.state.downcast_mut::<State>();
            if state.handover.step(*now) {
                shell.request_redraw();
            }
        }
        if let Some(place) = layout.children().next() {
            self.current.as_widget_mut().update(
                &mut tree.children[0],
                event,
                place,
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
        layout
            .children()
            .next()
            .map(|place| {
                self.current.as_widget().mouse_interaction(
                    &tree.children[0],
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
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        let place = layout.children().next()?;
        self.current.as_widget_mut().overlay(
            &mut tree.children[0],
            place,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Steps<'a, M>> for Element<'a, M> {
    fn from(steps: Steps<'a, M>) -> Self {
        Element::new(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_leaves_to_the_left_and_back_to_the_right() {
        assert_eq!(away(0, 1), Vector::new(-1.0, 0.0));
        assert_eq!(away(2, 1), Vector::new(1.0, 0.0));
    }
}
