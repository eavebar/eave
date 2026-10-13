//! The bar's three zones across the screen: the left one against the left edge, the right one
//! against the right edge, and the center one centered on the screen, unless a side reaches into
//! where it would be. Then it's pushed aside, to stay `ISLAND_SPACING` clear of it. The islands
//! spring to wherever layout puts them, so a pushed one glides away instead of jumping.
//!
//! With a notch (D-69), the center zone can't be centered: it comes in two halves, one each side
//! of the notch and right against it, so the islands there and the notch read as one.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::theme;

pub struct Zones<'a, M> {
    /// Left, center and right; or left, the center's two halves and right, around a notch.
    zones: Vec<Element<'a, M>>,
    /// The notch's width, centered.
    notch: Option<f32>,
}

impl<'a, M> Zones<'a, M> {
    pub fn new(left: Element<'a, M>, center: Element<'a, M>, right: Element<'a, M>) -> Self {
        Self {
            zones: vec![left, center, right],
            notch: None,
        }
    }

    /// Around a notch `width` wide, centered: the center zone's halves go each side of it.
    pub fn around_notch(
        left: Element<'a, M>,
        [before, after]: [Element<'a, M>; 2],
        right: Element<'a, M>,
        width: f32,
    ) -> Self {
        Self {
            zones: vec![left, before, after, right],
            notch: Some(width),
        }
    }
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
/// `before` wide, ends where the notch starts, and the second starts where it ends.
fn around(width: f32, notch: f32, before: f32) -> (f32, f32) {
    let start = (width - notch) / 2.0;
    (start - before, start + notch)
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
            Some(notch) => {
                let (before, after) = around(width, notch, widths[1]);
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
        for ((zone, tree), layout) in self.zones.iter().zip(&tree.children).zip(layout.children()) {
            zone.as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
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
    fn the_centers_halves_hug_the_notch() {
        // 1000 wide, a 200-wide notch from 400 to 600: a 150-wide half before it starts at 250.
        assert_eq!(around(1000.0, 200.0, 150.0), (250.0, 600.0));
        // Nothing before it: an empty half, ending where the notch starts.
        assert_eq!(around(1000.0, 200.0, 0.0), (400.0, 600.0));
    }
}
