//! The host side of [`eave_api::ui::switcher`]: a row of options with a mark behind the current
//! one. When the current option changes, the mark slides to it, one step per frame the compositor
//! asks for, and stops asking once it arrives.

use std::time::Instant;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Border, Element, Event, Length, Point, Rectangle, Size, Vector, window};

use crate::glass::frame::{self, Relief, Shape};
use crate::spring::{Feel, MAX_FRAME, MORPH, Spring};
use crate::theme;

pub struct Switcher<'a, M> {
    /// A row with one child per option.
    content: Element<'a, M>,
    current: Option<usize>,
    /// The mark's corner, nested in what it's in (D-125).
    radius: f32,
}

impl<'a, M> Switcher<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, current: Option<usize>, radius: f32) -> Self {
        Self {
            content: content.into(),
            current,
            radius,
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

/// The mark's motion. Positions are relative to the switcher. Tabs slide their mark with it too.
///
/// Each edge springs on `MORPH` (D-127): the one going ahead in `BODY_LEAD` of its time and the
/// one coming behind in `BODY_LAG` of it, as the body's sides do, so the mark stretches across
/// and draws in after itself.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    /// Where the mark is drawn this frame.
    pub(crate) shown: Option<Rectangle>,
    /// Left, top, right and bottom.
    edges: Option<[Spring; 4]>,
    to: Option<Rectangle>,
    last: Option<Instant>,
}

impl Motion {
    /// Advances to `now`, heading for `target`. Returns whether another frame is needed.
    pub(crate) fn step(&mut self, target: Option<Rectangle>, now: Instant) -> bool {
        let Some(target) = target else {
            *self = Self::default();
            return false;
        };
        let goal = [
            target.x,
            target.y,
            target.x + target.width,
            target.y + target.height,
        ];
        let Some(edges) = &mut self.edges else {
            // First appearance: no slide in from nowhere.
            self.edges = Some(goal.map(|edge| Spring::new(edge, MORPH)));
            self.shown = Some(target);
            self.to = Some(target);
            return false;
        };
        if self.to != Some(target) {
            self.to = Some(target);
            let now_at = [0, 1, 2, 3].map(|i| edges[i].value);
            // Which way it goes, across and down: the edges on that side lead.
            let way = [
                (goal[0] + goal[2]) - (now_at[0] + now_at[2]),
                (goal[1] + goal[3]) - (now_at[1] + now_at[3]),
            ];
            for (i, edge) in edges.iter_mut().enumerate() {
                let (way, far) = (way[i % 2], i >= 2);
                let tempo = if way.abs() < 0.5 {
                    1.0
                } else if (way > 0.0) == far {
                    theme::BODY_LEAD
                } else {
                    theme::BODY_LAG
                };
                edge.feel = Feel::new(theme::MORPH_RESPONSE * tempo, theme::MORPH_DAMPING);
                edge.target = goal[i];
            }
        }
        let seconds = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last)
                .min(MAX_FRAME)
                .as_secs_f32()
        });
        let mut moving = false;
        for edge in edges.iter_mut() {
            edge.advance(seconds);
            if edge.at_rest() {
                edge.settle();
            } else {
                moving = true;
            }
        }
        let [left, top, right, bottom] = edges.map(|edge| edge.value);
        self.shown = Some(Rectangle {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        });
        self.last = moving.then_some(now);
        moving
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
        let p = crate::theme::Palette::of(theme);
        use iced::advanced::Renderer as _;

        if let Some(shown) = tree.state.downcast_ref::<Motion>().shown {
            let origin = layout.bounds().position();
            let shown = shown + Vector::new(origin.x, origin.y);
            // On glass, the current option is a bead on it (D-126).
            if crate::press::on_glass() {
                frame::relief(
                    Relief {
                        rect: shown,
                        radius: self.radius.min(shown.height / 2.0),
                        shape: Shape::Bead,
                        depth: theme::RELIEF_BEAD,
                        press: 0.0,
                        milk: 0.0,
                        fill_x: shown.x + shown.width,
                        light: shown.center(),
                        lit: 0.0,
                    },
                    viewport,
                );
            } else {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: shown,
                        border: Border::default().rounded(self.radius),
                        ..renderer::Quad::default()
                    },
                    p.switcher_mark,
                );
            }
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

    /// Steps `motion` toward `target` until it rests, calling `each` after every frame.
    fn run(
        motion: &mut Motion,
        target: Option<Rectangle>,
        mut now: Instant,
        mut each: impl FnMut(&Motion),
    ) {
        for _ in 0..1000 {
            now += Duration::from_millis(8);
            let moving = motion.step(target, now);
            each(motion);
            if !moving {
                return;
            }
        }
        panic!("it never came to rest");
    }

    #[test]
    fn appears_in_place_then_slides() {
        let start = Instant::now();
        let mut motion = Motion::default();
        assert!(!motion.step(at(0.0), start));
        assert_eq!(motion.shown, at(0.0));

        assert!(motion.step(at(100.0), start));
        assert_eq!(motion.shown, at(0.0));

        run(&mut motion, at(100.0), start, |_| {});
        assert_eq!(motion.shown, at(100.0));
        assert!(!motion.step(at(100.0), start), "there, it asks for nothing");
    }

    #[test]
    fn the_leading_edge_goes_ahead_and_the_mark_stretches_across() {
        let start = Instant::now();
        let mut motion = Motion::default();
        motion.step(at(0.0), start);
        motion.step(at(100.0), start);
        let mut widest: f32 = 0.0;
        run(&mut motion, at(100.0), start, |motion| {
            widest = widest.max(motion.shown.unwrap().width);
        });
        assert!(widest > 40.0, "it stretches: {widest}");
        assert_eq!(motion.shown, at(100.0), "and draws in after itself");

        // Back the other way, the left edge leads.
        motion.step(at(0.0), start);
        let mut led = false;
        run(&mut motion, at(0.0), start, |motion| {
            let shown = motion.shown.unwrap();
            led |= 100.0 - shown.x > 120.0 - (shown.x + shown.width) + 10.0;
        });
        assert!(led, "the left edge goes ahead going left");
    }

    #[test]
    fn retargets_from_where_it_is() {
        let start = Instant::now();
        let mut motion = Motion::default();
        motion.step(at(0.0), start);
        motion.step(at(100.0), start);
        motion.step(at(100.0), start + Duration::from_millis(80));
        let there = motion.shown.unwrap();
        assert!(there.x > 0.0 && there.x < 100.0);
        motion.step(at(0.0), start + Duration::from_millis(81));
        let next = motion.shown.unwrap();
        assert!(
            (next.x - there.x).abs() < 2.0,
            "no jump: {} to {}",
            there.x,
            next.x
        );
    }

    #[test]
    fn vanishes_without_a_current_option() {
        let mut motion = Motion::default();
        motion.step(at(0.0), Instant::now());
        assert!(!motion.step(None, Instant::now()));
        assert_eq!(motion.shown, None);
    }
}
