//! The bar's three zones across the screen: the left one against the left edge, the right one
//! against the right edge, and the center one centered on the screen, unless a side reaches into
//! where it would be. Then it's pushed aside, to stay `ISLAND_SPACING` clear of it. The islands
//! spring to wherever layout puts them, so a pushed one glides away instead of jumping.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::theme;

pub struct Zones<'a, M> {
    /// Left, center and right.
    zones: [Element<'a, M>; 3],
}

impl<'a, M> Zones<'a, M> {
    pub fn new(left: Element<'a, M>, center: Element<'a, M>, right: Element<'a, M>) -> Self {
        Self {
            zones: [left, center, right],
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
        let [left, center, right] = [0, 1, 2].map(|index| {
            self.zones[index].as_widget_mut().layout(
                &mut tree.children[index],
                renderer,
                &child_limits,
            )
        });
        let (left_width, center_width, right_width) =
            (left.size().width, center.size().width, right.size().width);
        let gap = theme::ISLAND_SPACING as f32;
        let x = center_x(width, left_width, center_width, right_width, gap).round();
        let height = [&left, &center, &right]
            .iter()
            .map(|node| node.size().height)
            .fold(0.0, f32::max);
        layout::Node::with_children(
            Size::new(width, height),
            vec![
                left.move_to(Point::ORIGIN),
                center.move_to(Point::new(x, 0.0)),
                right.move_to(Point::new((width - right_width).round(), 0.0)),
            ],
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
}
