//! The bar's three zones across the screen: the left one against the left edge, the right one
//! against the right edge, and the center one centered on the screen, unless a side reaches into
//! where it would be. Then it's pushed aside, to stay `ISLAND_SPACING` clear of it. The islands
//! spring to wherever layout puts them, so a pushed one glides away instead of jumping.
//!
//! With a notch (D-69), the center zone can't be centered: it comes in two halves, one each side
//! of the notch. A half whose island there is fused to the notch is right against it, so they read
//! as one; any other keeps `ISLAND_SPACING` from it (D-73). The notch itself is drawn black, as
//! big as the bar has it, which may be bigger than it is (D-75). So is the top edge, when the bar
//! grows it (D-81).

use std::cell::Cell;

use iced::advanced::Renderer as _;
use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::canvas::{Cache, Path};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::notch::Notch;
use crate::squircle::{self, CornerShape, squircle_corners};
use crate::theme;

pub struct Zones<'a, M> {
    /// Left, center and right; or left, the center's two halves and right, around a notch.
    zones: Vec<Element<'a, M>>,
    /// The notch, centered, and the gap each half keeps from it.
    notch: Option<(Notch, [f32; 2])>,
    /// How far below where the zones start the notch does: up, so negative.
    at: f32,
    /// The top edge the bar grows: how much, and the screen's top left and right corners, which
    /// round what shows below it.
    edge: Option<(f32, [CornerShape; 2])>,
}

/// The notch and the grown edge as last drawn, and what they were drawn from.
struct State {
    cache: Cache,
    drawn: Cell<Option<(Notch, f32)>>,
    edge_cache: Cache,
    edge_drawn: Cell<Option<(f32, [CornerShape; 2], f32)>>,
}

impl<'a, M> Zones<'a, M> {
    pub fn new(left: Element<'a, M>, center: Element<'a, M>, right: Element<'a, M>) -> Self {
        Self {
            zones: vec![left, center, right],
            notch: None,
            at: 0.0,
            edge: None,
        }
    }

    /// Around `notch`, centered: the center zone's halves go each side of it, right against it
    /// where `fused`. It starts `at` below where the zones do: above them, so negative.
    pub fn around_notch(
        left: Element<'a, M>,
        [before, after]: [Element<'a, M>; 2],
        right: Element<'a, M>,
        notch: Notch,
        at: f32,
        fused: [bool; 2],
    ) -> Self {
        let gaps = fused.map(|fused| {
            if fused {
                0.0
            } else {
                theme::ISLAND_SPACING as f32
            }
        });
        Self {
            zones: vec![left, before, after, right],
            notch: Some((notch, gaps)),
            at,
            edge: None,
        }
    }

    /// With the screen's top edge drawn `grow` thicker than it is, above the gap over the zones,
    /// and rounded into the sides by the screen's top `corners` (D-81). Nothing if it isn't grown.
    pub fn edge(mut self, grow: f32, corners: [CornerShape; 2]) -> Self {
        self.edge = (grow > 0.0).then_some((grow, corners));
        self
    }
}

/// The grown top edge across `width`, `grow` tall, in a frame its own size: a band whose lower
/// ends curve down into the sides, the way the screen's top `corners` round what shows below it.
/// The frame is as tall as the band and the deeper of its curves.
fn edge_outline(grow: f32, corners: [CornerShape; 2], width: f32) -> (Path, f32) {
    let budget = width / 2.0;
    let [left, right] =
        corners.map(|corner| squircle::Corner::new(corner.radius, corner.smoothing, budget));
    let path = Path::new(|builder| {
        builder.move_to(Point::ORIGIN);
        builder.line_to(Point::new(width, 0.0));
        // Down the right side to where the corner starts, round it back the way what shows below
        // turns there, along the band's bottom, and round the left one down the left side.
        builder.line_to(Point::new(width, grow + right.extent()));
        squircle::corner_into(
            builder,
            &right,
            Point::new(width, grow),
            squircle::UP,
            squircle::LEFT,
            false,
        );
        squircle::corner_into(
            builder,
            &left,
            Point::new(0.0, grow),
            squircle::LEFT,
            squircle::DOWN,
            false,
        );
        builder.close();
    });
    (path, grow + left.extent().max(right.extent()))
}

/// The notch starting `y` below where the zones start, in a frame its own size: what's drawn, and
/// the frame's top left in the zones' coordinates, centered in `width`. Hanging from the screen's
/// top edge, it's rounded outwards there like the screen's is, and reaches past it out of sight;
/// lower down, it's a cut-out, rounded all round.
fn notch_outline(notch: Notch, y: f32, width: f32) -> (Path, Point) {
    let lower = CornerShape::new(notch.radius, notch.smoothing);
    let x = (width - notch.width) / 2.0;
    if notch.top > 0.0 {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(notch.width, notch.height));
        return (squircle_corners(bounds, [lower; 4]), Point::new(x, y));
    }
    // Room beside it for the shoulders, and above it to reach past the edge.
    let room = Vector::new(notch.shoulder * 2.0, theme::ATTACH_OVERHANG);
    let path = Path::new(|builder| {
        let bounds = Rectangle::new(
            Point::new(room.x, 0.0),
            Size::new(notch.width, room.y + notch.height),
        );
        let corners = [CornerShape::SQUARE, CornerShape::SQUARE, lower, lower];
        let shoulder = CornerShape::new(notch.shoulder, notch.smoothing);
        squircle::hanging(builder, bounds, corners, room.y, shoulder, [true; 2]);
    });
    (path, Point::new(x, y) - room)
}

/// Where the center zone starts: centered in `width`, or as near that as it can be while
/// keeping `gap` from a side zone that isn't empty. With no room for both gaps, it sits in the
/// middle of what room there is.
fn center_x(width: f32, left: f32, center: f32, right: f32, gap: f32) -> f32 {
    let from = if left > 0.0 { left + gap } else { 0.0 };
    let to = width - center - if right > 0.0 { right + gap } else { 0.0 };
    let centered = (width - center) / 2.0;
    if from <= to {
        centered.clamp(from, to)
    } else {
        (from + to) / 2.0
    }
}

/// Where the center's halves start around a notch `notch` wide, centered in `width`: the first,
/// `before` wide, ends `gaps[0]` before the notch starts, and the second starts `gaps[1]` after it
/// ends.
fn around(width: f32, notch: f32, before: f32, gaps: [f32; 2]) -> (f32, f32) {
    let start = (width - notch) / 2.0;
    (start - gaps[0] - before, start + notch + gaps[1])
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Zones<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        let width = max.width;
        let child_limits = layout::Limits::new(Size::ZERO, max);
        let nodes: Vec<_> = self
            .zones
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(zone, tree)| zone.as_widget_mut().layout(tree, renderer, &child_limits))
            .collect();
        let widths: Vec<_> = nodes.iter().map(|node| node.size().width).collect();
        let gap = theme::ISLAND_SPACING as f32;
        let xs = match self.notch {
            None => vec![
                0.0,
                center_x(width, widths[0], widths[1], widths[2], gap).round(),
                (width - widths[2]).round(),
            ],
            Some((notch, gaps)) => {
                let (before, after) = around(width, notch.width, widths[1], gaps);
                vec![
                    0.0,
                    before.round(),
                    after.round(),
                    (width - widths[3]).round(),
                ]
            }
        };
        let height = nodes
            .iter()
            .map(|node| node.size().height)
            .fold(0.0, f32::max);
        layout::Node::with_children(
            Size::new(width, height),
            nodes
                .into_iter()
                .zip(xs)
                .map(|(node, x)| node.move_to(Point::new(x, 0.0)))
                .collect(),
        )
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
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        // Across the whole surface, gaps included, from its top.
        if let Some((grow, corners)) = self.edge {
            let gap = f32::from(theme::EDGE_GAP);
            let width = bounds.width + 2.0 * gap;
            let drawn = (grow, corners, width);
            if state.edge_drawn.replace(Some(drawn)) != Some(drawn) {
                state.edge_cache.clear();
            }
            let (path, height) = edge_outline(grow, corners, width);
            let geometry = state
                .edge_cache
                .draw(renderer, Size::new(width, height), |frame| {
                    frame.fill(&path, theme::ISLAND_BG);
                });
            let at = Vector::new(bounds.x - gap, bounds.y - gap - grow);
            renderer.with_translation(at, |renderer| renderer.draw_geometry(geometry));
        }
        // Under the islands, so what's fused to it covers it.
        if let Some((notch, _)) = self.notch {
            let drawn = (notch, self.at);
            if state.drawn.replace(Some(drawn)) != Some(drawn) {
                state.cache.clear();
            }
            let (path, at) = notch_outline(notch, self.at, bounds.width);
            let size = Size::new(
                notch.width + notch.shoulder * 4.0,
                notch.height + theme::ATTACH_OVERHANG,
            );
            let geometry = state.cache.draw(renderer, size, |frame| {
                frame.fill(&path, theme::ISLAND_BG);
            });
            renderer.with_translation(Vector::new(bounds.x + at.x, bounds.y + at.y), |renderer| {
                renderer.draw_geometry(geometry);
            });
        }
        for ((zone, tree), layout) in self.zones.iter().zip(&tree.children).zip(layout.children()) {
            zone.as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            cache: Cache::new(),
            drawn: Cell::new(None),
            edge_cache: Cache::new(),
            edge_drawn: Cell::new(None),
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.zones.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.zones);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((zone, tree), layout) in self
            .zones
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            zone.as_widget_mut()
                .operate(tree, layout, renderer, operation);
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
        for ((zone, tree), layout) in self
            .zones
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            zone.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
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
        self.zones
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((zone, tree), layout)| {
                zone.as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
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
        overlay::from_children(
            &mut self.zones,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Zones<'a, M>> for Element<'a, M> {
    fn from(zones: Zones<'a, M>) -> Self {
        Element::new(zones)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_center_stays_centered_until_a_side_reaches_it() {
        // 1000 wide, a 100-wide center: centered at 450, with room on both sides.
        assert_eq!(center_x(1000.0, 200.0, 100.0, 200.0, 6.0), 450.0);
        // The left side grows to 500: pushed right, to 6 past it.
        assert_eq!(center_x(1000.0, 500.0, 100.0, 200.0, 6.0), 506.0);
        // The right side grows to 500: pushed left.
        assert_eq!(center_x(1000.0, 200.0, 100.0, 500.0, 6.0), 394.0);
        // Empty sides need no gap.
        assert_eq!(center_x(100.0, 0.0, 100.0, 0.0, 6.0), 0.0);
        // No room at all: halfway between where each side would have it, which is centered.
        assert_eq!(center_x(1000.0, 600.0, 100.0, 600.0, 6.0), 450.0);
    }

    #[test]
    fn the_centers_fused_halves_hug_the_notch() {
        // 1000 wide, a 200-wide notch from 400 to 600: a 150-wide half before it starts at 250.
        assert_eq!(around(1000.0, 200.0, 150.0, [0.0; 2]), (250.0, 600.0));
        // Nothing before it: an empty half, ending where the notch starts.
        assert_eq!(around(1000.0, 200.0, 0.0, [0.0; 2]), (400.0, 600.0));
    }

    #[test]
    fn the_centers_other_halves_keep_a_gap_from_the_notch() {
        assert_eq!(around(1000.0, 200.0, 150.0, [6.0, 0.0]), (244.0, 600.0));
        assert_eq!(around(1000.0, 200.0, 150.0, [0.0, 6.0]), (250.0, 606.0));
    }
}
