//! The host side of [`eave_api::ui::switcher`]: a row of options with a mark behind the current
//! one. When the current option changes, the mark slides to it, one step per frame the compositor
//! asks for, and stops asking once it arrives.

use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Border, Element, Event, Length, Point, Rectangle, Size, Vector, window};

use crate::theme;

pub struct Switcher<'a, M> {
    /// A row with one child per option.
    content: Element<'a, M>,
    current: Option<usize>,
}

impl<'a, M> Switcher<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, current: Option<usize>) -> Self {
        Self {
            content: content.into(),
            current,
        }
    }

    /// Where the mark should end up, relative to the switcher, so moving the whole switcher (a
    /// neighbour changing width) doesn't count as a change.
    fn target(&self, layout: Layout<'_>) -> Option<Rectangle> {
        let origin = layout.bounds().position();
        let child = layout.children().next()?.children().nth(self.current?)?;
        let bounds = child.bounds();
        Some(Rectangle::new(
            Point::ORIGIN + (bounds.position() - origin),
            bounds.size(),
        ))
    }
}

/// The mark's motion. Positions are relative to the switcher.
#[derive(Debug, Default)]
struct Motion {
    /// Where the mark is drawn this frame.
    shown: Option<Rectangle>,
    from: Rectangle,
    to: Option<Rectangle>,
    start: Option<Instant>,
}

impl Motion {
    /// Advances to `now`, heading for `target`. Returns whether another frame is needed.
    fn step(&mut self, target: Option<Rectangle>, now: Instant) -> bool {
        let Some(target) = target else {
            *self = Self::default();
            return false;
        };
        match self.to {
            // First appearance: no slide in from nowhere.
            None => {
                self.shown = Some(target);
                self.to = Some(target);
                self.start = None;
            }
            Some(to) if to != target => {
                self.from = self.shown.unwrap_or(target);
                self.to = Some(target);
                self.start = Some(now);
            }
            Some(_) => {}
        }

        let Some(start) = self.start else {
            return false;
        };
        let progress =
            now.duration_since(start).as_secs_f32() / theme::SWITCHER_SLIDE.as_secs_f32();
        if progress >= 1.0 {
            self.shown = Some(target);
            self.start = None;
            false
        } else {
            self.shown = Some(lerp(self.from, target, ease_out(progress)));
            true
        }
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn lerp(a: Rectangle, b: Rectangle, t: f32) -> Rectangle {
    let mix = |a: f32, b: f32| a + (b - a) * t;
    Rectangle {
        x: mix(a.x, b.x),
        y: mix(a.y, b.y),
        width: mix(a.width, b.width),
        height: mix(a.height, b.height),
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Switcher<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Motion>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Motion::default())
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
        // Wrapped in a node of our own, so `target` can reach the row's children the same way
        // whatever the row reports as its own bounds.
        let row = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(row.size(), vec![row])
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

        if let Some(shown) = tree.state.downcast_ref::<Motion>().shown {
            let origin = layout.bounds().position();
            renderer.fill_quad(
                renderer::Quad {
                    bounds: shown + Vector::new(origin.x, origin.y),
                    border: Border::default().rounded(theme::BUTTON_RADIUS),
                    ..renderer::Quad::default()
                },
                theme::SWITCHER_MARK,
            );
        }
        let Some(row) = layout.children().next() else {
            return;
        };
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            row,
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
        let Some(row) = layout.children().next() else {
            return;
        };
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], row, renderer, operation);
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
        if let Some(row) = layout.children().next() {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                row,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        // Every frame starts with this event, after layout, so the target is current.
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let target = self.target(layout);
            if tree.state.downcast_mut::<Motion>().step(target, *now) {
                shell.request_redraw();
            }
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
        let Some(row) = layout.children().next() else {
            return mouse::Interaction::None;
        };
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            row,
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
        let row = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            row,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Switcher<'a, M>> for Element<'a, M> {
    fn from(switcher: Switcher<'a, M>) -> Self {
        Element::new(switcher)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(x: f32) -> Option<Rectangle> {
        Some(Rectangle::new(Point::new(x, 0.0), Size::new(20.0, 24.0)))
    }

    #[test]
    fn appears_in_place_then_slides() {
        let start = Instant::now();
        let mut motion = Motion::default();
        assert!(!motion.step(at(0.0), start));
        assert_eq!(motion.shown, at(0.0));

        assert!(motion.step(at(100.0), start));
        assert_eq!(motion.shown, at(0.0));

        assert!(motion.step(at(100.0), start + theme::SWITCHER_SLIDE / 2));
        let middle = motion.shown.unwrap().x;
        assert!(
            middle > 50.0 && middle < 100.0,
            "eased, ahead of linear: {middle}"
        );

        assert!(!motion.step(at(100.0), start + theme::SWITCHER_SLIDE));
        assert_eq!(motion.shown, at(100.0));
    }

    #[test]
    fn retargets_from_where_it_is() {
        let start = Instant::now();
        let mut motion = Motion::default();
        motion.step(at(0.0), start);
        motion.step(at(100.0), start);
        let half = start + theme::SWITCHER_SLIDE / 2;
        motion.step(at(100.0), half);
        let there = motion.shown;
        motion.step(at(0.0), half + Duration::from_millis(1));
        assert_eq!(motion.from, there.unwrap());
    }

    #[test]
    fn vanishes_without_a_current_option() {
        let mut motion = Motion::default();
        motion.step(at(0.0), Instant::now());
        assert!(!motion.step(None, Instant::now()));
        assert_eq!(motion.shown, None);
    }
}
