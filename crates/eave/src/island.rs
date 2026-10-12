//! An island: the squircle behind a zone's items. It never snaps to a new shape. When items come
//! or go, its edges spring to the new size, the items that stay slide to their new places and new
//! ones fade in. It reacts to the pointer like something physical: a press squeezes it in a
//! little, dragging pulls it along with growing resistance, and letting go springs it back.
//!
//! An island can also become a popover. Given one, it morphs into the panel's shape, growing from
//! where it sits, while its items fade out and the panel's content fades in. Closing runs that
//! backwards, and reports when it's done. The panel lives in an overlay, so it's laid out against
//! the whole surface, drawn over the other islands, and sees events first.
//!
//! It only watches pointer events on the items and never captures them, so the items work exactly
//! as they would without it. Like the switcher, it steps one frame at a time when the compositor
//! asks, and stops asking once everything is at rest.

use std::time::{Duration, Instant};

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::alignment::Horizontal;
use iced::widget::canvas::{Cache, Stroke};
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Transformation, Vector, window};

use crate::squircle::squircle;
use crate::theme;

pub struct Island<'a, M> {
    /// Each item's key, which follows it when items before it come or go.
    keys: Vec<u64>,
    items: Vec<Element<'a, M>>,
    /// Which way the island grows into a popover: from its left edge, its right edge or its center.
    align: Horizontal,
    popover: Option<Popover<'a, M>>,
}

/// What an island turns into while it's a popover.
pub struct Popover<'a, M> {
    pub content: Element<'a, M>,
    /// False while closing: the island morphs back, then sends `on_closed`.
    pub open: bool,
    /// Where the panel goes, in surface coordinates, whenever that changes.
    pub on_placed: Box<dyn Fn(Rectangle) -> M + 'a>,
    pub on_closed: M,
}

impl<'a, M> Island<'a, M> {
    pub fn new(items: impl IntoIterator<Item = (u64, Element<'a, M>)>) -> Self {
        let (keys, items) = items.into_iter().unzip();
        Self {
            keys,
            items,
            align: Horizontal::Center,
            popover: None,
        }
    }

    pub fn align(mut self, align: Horizontal) -> Self {
        self.align = align;
        self
    }

    pub fn popover(mut self, popover: Option<Popover<'a, M>>) -> Self {
        self.popover = popover;
        self
    }
}

struct State {
    motion: Motion,
    /// The squircle, rebuilt only when its size changes: at rest it's built once.
    shape: Cache,
    /// The popover panel's widget state, while there is one.
    panel: Option<Tree>,
}

/// How a spring moves: how hard it pulls toward its target, and its damping ratio (below 1 it
/// overshoots a little before settling).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Feel {
    stiffness: f32,
    damping: f32,
}

/// The press squeeze and the drag: springy, so letting go overshoots a little.
const PLAYFUL: Feel = Feel {
    stiffness: theme::SPRING_STIFFNESS,
    damping: theme::SPRING_DAMPING,
};
/// Shape and content changes: settle without wobbling.
const MORPH: Feel = Feel {
    stiffness: theme::MORPH_STIFFNESS,
    damping: theme::MORPH_DAMPING,
};

/// Where everything is going and how it's moving there. Positions are in logical pixels, in
/// surface coordinates; fades go from 0 (hidden) to 1 (shown).
#[derive(Debug)]
struct Motion {
    /// Where the pointer went down, while the primary button is held.
    press: Option<Point>,
    x: Spring,
    y: Spring,
    /// How far each side has moved in.
    inset: Spring,
    /// Where the island's layout puts it. Unknown until it's first laid out.
    island: Option<Rectangle>,
    /// The drawn shape's edges, chasing `island`, or the panel while a popover is open.
    edges: Edges,
    items: Vec<ItemMotion>,
    popover: PopoverMotion,
    /// The previous frame, while something is moving.
    last: Option<Instant>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            press: None,
            x: Spring::new(0.0, PLAYFUL),
            y: Spring::new(0.0, PLAYFUL),
            inset: Spring::new(0.0, PLAYFUL),
            island: None,
            edges: Edges::at(Rectangle::default()),
            items: Vec::new(),
            popover: PopoverMotion::default(),
            last: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Edges {
    left: Spring,
    top: Spring,
    right: Spring,
    bottom: Spring,
}

impl Edges {
    fn at(rect: Rectangle) -> Self {
        Self {
            left: Spring::new(rect.x, MORPH),
            top: Spring::new(rect.y, MORPH),
            right: Spring::new(rect.x + rect.width, MORPH),
            bottom: Spring::new(rect.y + rect.height, MORPH),
        }
    }

    fn aim(&mut self, rect: Rectangle) {
        self.left.target = rect.x;
        self.top.target = rect.y;
        self.right.target = rect.x + rect.width;
        self.bottom.target = rect.y + rect.height;
    }

    fn rect(&self) -> Rectangle {
        let (left, top) = (self.left.value, self.top.value);
        Rectangle::new(
            Point::new(left, top),
            Size::new(
                (self.right.value - left).max(0.0),
                (self.bottom.value - top).max(0.0),
            ),
        )
    }

    /// How far, from 0 to 1, the shape has come from `from` toward `to`.
    fn progress(&self, from: Rectangle, to: Rectangle) -> f32 {
        let distance = |a: Rectangle, b: [f32; 4]| {
            (a.x - b[0]).abs()
                + (a.y - b[1]).abs()
                + (a.x + a.width - b[2]).abs()
                + (a.y + a.height - b[3]).abs()
        };
        let total = distance(from, edges_of(to));
        if total < 1.0 {
            return 1.0;
        }
        let now = [
            self.left.value,
            self.top.value,
            self.right.value,
            self.bottom.value,
        ];
        (1.0 - distance(to, now) / total).clamp(0.0, 1.0)
    }
}

fn edges_of(rect: Rectangle) -> [f32; 4] {
    [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height]
}

/// One item's slide and fade, found again by its key after a change.
#[derive(Debug)]
struct ItemMotion {
    key: u64,
    x: Spring,
    shown: Spring,
}

#[derive(Debug)]
struct PopoverMotion {
    /// Whether the island has a popover at all, open or closing.
    present: bool,
    open: bool,
    /// Where the panel goes, once laid out.
    rect: Option<Rectangle>,
    /// Whether the surface is tall enough to show it. Until it is, the island waits.
    fits: bool,
    /// The last place reported to the host.
    reported: Option<Rectangle>,
    /// Whether the host has been told the island is back to itself.
    closed: bool,
    /// How much the island's items show: they make way for the panel.
    items: Spring,
    /// How much the panel's content shows.
    panel: Spring,
}

impl Default for PopoverMotion {
    fn default() -> Self {
        Self {
            present: false,
            open: false,
            rect: None,
            fits: false,
            reported: None,
            closed: false,
            items: Spring::new(1.0, MORPH),
            panel: Spring::new(0.0, MORPH),
        }
    }
}

impl PopoverMotion {
    /// Open, and with somewhere to open to.
    fn opening(&self) -> Option<Rectangle> {
        self.rect.filter(|_| self.open && self.fits)
    }
}

impl Motion {
    fn press(&mut self, at: Point) {
        self.press = Some(at);
        self.inset.target = theme::PRESS_INSET;
    }

    /// Pulls toward the pointer, harder to move the further it goes.
    fn drag(&mut self, to: Point) {
        let Some(from) = self.press else {
            return;
        };
        self.x.target = rubber_band(to.x - from.x, theme::DRAG_LIMIT_X);
        self.y.target = rubber_band(to.y - from.y, theme::DRAG_LIMIT_Y);
    }

    fn release(&mut self) {
        self.press = None;
        self.x.target = 0.0;
        self.y.target = 0.0;
        self.inset.target = 0.0;
    }

    /// Takes in where layout put the island and each item (by key, with its x), and whether it has
    /// a popover and if that's open. Everything that changed starts moving there.
    fn arrange(
        &mut self,
        island: Rectangle,
        items: impl Iterator<Item = (u64, f32)>,
        popover: Option<bool>,
    ) {
        // A new island grows out of a sliver at its center.
        if self.island.is_none() {
            let sliver = Rectangle::new(
                Point::new(island.center_x(), island.y),
                Size::new(0.0, island.height),
            );
            self.edges = Edges::at(sliver);
        }
        self.island = Some(island);

        let mut kept = Vec::with_capacity(self.items.len());
        for (key, x) in items {
            match self.items.iter().position(|item| item.key == key) {
                Some(index) => {
                    let mut item = self.items.swap_remove(index);
                    item.x.target = x;
                    kept.push(item);
                }
                // New: in place at once, fading in.
                None => kept.push(ItemMotion {
                    key,
                    x: Spring::new(x, MORPH),
                    shown: Spring {
                        target: 1.0,
                        ..Spring::new(0.0, MORPH)
                    },
                }),
            }
        }
        // Whatever is left in `self.items` is gone.
        self.items = kept;

        let popover_state = &mut self.popover;
        match popover {
            Some(open) => {
                popover_state.present = true;
                if open && !popover_state.open {
                    popover_state.closed = false;
                }
                popover_state.open = open;
            }
            None if popover_state.present => {
                // Taken away without closing: back to the island, wherever it got to.
                let (items, panel) = (popover_state.items, popover_state.panel);
                *popover_state = PopoverMotion {
                    items: Spring {
                        target: 1.0,
                        ..items
                    },
                    panel: Spring {
                        target: 0.0,
                        ..panel
                    },
                    ..PopoverMotion::default()
                };
            }
            None => {}
        }
        self.aim();
    }

    /// Points the shape and the fades where they should go now. During a popover's morph, each
    /// side's content only fades in once the shape is more than halfway its way.
    fn aim(&mut self) {
        let Some(island) = self.island else {
            return;
        };
        let popover = &mut self.popover;
        match (popover.opening(), popover.rect) {
            (Some(panel), _) => {
                self.edges.aim(panel);
                popover.items.target = 0.0;
                popover.panel.target = if self.edges.progress(island, panel) > 0.5 {
                    1.0
                } else {
                    0.0
                };
            }
            (None, Some(panel)) if popover.present => {
                self.edges.aim(island);
                popover.panel.target = 0.0;
                popover.items.target = if self.edges.progress(panel, island) > 0.5 {
                    1.0
                } else {
                    0.0
                };
            }
            _ => {
                self.edges.aim(island);
                popover.items.target = 1.0;
                popover.panel.target = 0.0;
            }
        }
    }

    fn springs(&mut self) -> impl Iterator<Item = &mut Spring> {
        let edges = &mut self.edges;
        [
            &mut self.x,
            &mut self.y,
            &mut self.inset,
            &mut edges.left,
            &mut edges.top,
            &mut edges.right,
            &mut edges.bottom,
            &mut self.popover.items,
            &mut self.popover.panel,
        ]
        .into_iter()
        .chain(
            self.items
                .iter_mut()
                .flat_map(|item| [&mut item.x, &mut item.shown]),
        )
    }

    fn at_rest(&mut self) -> bool {
        self.springs().all(|spring| spring.at_rest())
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        self.aim();
        if self.at_rest() {
            self.last = None;
            return false;
        }
        // The first frame after rest has no elapsed time to cover; a long stall is cut short so
        // the springs pick up where they were instead of jumping.
        let elapsed = self
            .last
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last))
            .min(MAX_FRAME)
            .as_secs_f32();
        self.last = Some(now);
        for spring in self.springs() {
            spring.advance(elapsed);
        }
        // The fades wait on how far the shape has come.
        self.aim();
        if self.at_rest() {
            self.springs().for_each(Spring::settle);
            self.last = None;
            return false;
        }
        true
    }

    /// Whether a closing popover is all the way back to the island, and nobody's been told yet.
    fn closing_done(&mut self) -> bool {
        let done = self.popover.present && !self.popover.open && !self.popover.closed;
        if done && self.at_rest() {
            self.popover.closed = true;
            return true;
        }
        false
    }

    fn offset(&self) -> Vector {
        Vector::new(self.x.value, self.y.value)
    }

    /// How far an item laid out at `x` is from where it's drawn, and how much it shows.
    fn item(&self, key: u64, x: f32) -> (f32, f32) {
        let fade = self.popover.items.value;
        match self.items.iter().find(|item| item.key == key) {
            Some(item) => (item.x.value - x, item.shown.value * fade),
            // Not arranged yet: new, so not shown yet.
            None => (0.0, 0.0),
        }
    }
}

/// The longest step a single frame may take.
const MAX_FRAME: Duration = Duration::from_millis(50);
/// Springs are integrated in steps no longer than this, so they stay stable at any frame rate.
const SUBSTEP: f32 = 1.0 / 480.0;
/// Close enough to the target, and slow enough, to stop: well under a physical pixel, or a
/// hundredth of a fade.
const REST_DISTANCE: f32 = 0.01;
const REST_SPEED: f32 = 0.5;

/// A damped spring along one axis.
#[derive(Debug, Clone, Copy)]
struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
    feel: Feel,
}

impl Spring {
    /// At rest at `value`.
    fn new(value: f32, feel: Feel) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            feel,
        }
    }

    fn advance(&mut self, seconds: f32) {
        let Feel { stiffness, damping } = self.feel;
        let damping = 2.0 * damping * stiffness.sqrt();
        let mut left = seconds;
        while left > 0.0 {
            let dt = left.min(SUBSTEP);
            let force = -stiffness * (self.value - self.target) - damping * self.velocity;
            self.velocity += force * dt;
            self.value += self.velocity * dt;
            left -= dt;
        }
    }

    fn at_rest(&self) -> bool {
        (self.value - self.target).abs() < REST_DISTANCE && self.velocity.abs() < REST_SPEED
    }

    fn settle(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

/// Moves almost one to one at first, then gives less and less, approaching `limit` without ever
/// reaching it.
fn rubber_band(distance: f32, limit: f32) -> f32 {
    let pulled = distance.abs() * theme::DRAG_RESISTANCE;
    (limit * (1.0 - 1.0 / (1.0 + pulled / limit))).copysign(distance)
}

/// Where a popover goes: as tall as the panel and never narrower than the island, with its top
/// where the island's is. It grows from the island's left edge, right edge or center, and stays
/// `EDGE_GAP` clear of the surface's sides.
fn place(island: Rectangle, panel: Size, align: Horizontal, surface_width: f32) -> Rectangle {
    let padding = 2.0 * f32::from(theme::PANEL_PADDING);
    let width = (panel.width + padding).max(island.width).ceil();
    let height = (panel.height + padding).max(island.height).ceil();
    let x = match align {
        Horizontal::Left => island.x,
        Horizontal::Right => island.x + island.width - width,
        Horizontal::Center => island.center_x() - width / 2.0,
    };
    let gap = f32::from(theme::EDGE_GAP);
    let x = x.min(surface_width - gap - width).max(gap).round();
    Rectangle::new(Point::new(x, island.y), Size::new(width, height))
}

/// The panel's content, centered in its place.
fn panel_offset(place: Size, content: Size) -> Vector {
    Vector::new(
        ((place.width - content.width) / 2.0).round(),
        ((place.height - content.height) / 2.0).round(),
    )
}

/// Draws the island as it is right now: the shape, the items that show, and the popover panel if
/// one is given (with its layout).
#[allow(clippy::too_many_arguments)]
fn draw_island<M>(
    state: &State,
    keys: &[u64],
    items: &[Element<'_, M>],
    trees: &[Tree],
    layout: Layout<'_>,
    panel: Option<(&Element<'_, M>, &Tree, Layout<'_>)>,
    renderer: &mut iced::Renderer,
    theme: &iced::Theme,
    style: &renderer::Style,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
) {
    use iced::advanced::Renderer as _;

    let motion = &state.motion;
    let shape = if motion.island.is_some() {
        motion.edges.rect()
    } else {
        layout.bounds()
    };
    if shape.width < 1.0 || shape.height < 1.0 {
        return;
    }
    // Squeezed by the same few pixels whatever the island's width, about its center.
    let scale = (1.0 - 2.0 * motion.inset.value / shape.width).max(0.5);
    let center = shape.center();
    let transformation = Transformation::translate(center.x, center.y)
        * Transformation::scale(scale)
        * Transformation::translate(-center.x, -center.y);

    renderer.with_translation(motion.offset(), |renderer| {
        renderer.with_transformation(transformation, |renderer| {
            let geometry = state.shape.draw(renderer, shape.size(), |frame| {
                let size = frame.size();
                let radius = theme::ISLAND_RADIUS;
                let smoothing = theme::ISLAND_SMOOTHING;
                frame.fill(
                    &squircle(Rectangle::with_size(size), radius, smoothing),
                    theme::ISLAND_BG,
                );
                // Half a pixel in, so the whole hairline lands inside the fill.
                let inset = Rectangle::new(
                    Point::new(0.5, 0.5),
                    Size::new(size.width - 1.0, size.height - 1.0),
                );
                frame.stroke(
                    &squircle(inset, radius - 0.5, smoothing),
                    Stroke::default()
                        .with_color(theme::ISLAND_BORDER)
                        .with_width(1.0),
                );
            });
            renderer.with_translation(Vector::new(shape.x, shape.y), |renderer| {
                renderer.draw_geometry(geometry);
            });

            // Nothing pokes out of the shape while it changes.
            renderer.with_layer(shape, |renderer| {
                for ((key, item), (tree, child)) in keys
                    .iter()
                    .zip(items)
                    .zip(trees.iter().zip(layout.children()))
                {
                    let bounds = child.bounds();
                    let (dx, shown) = motion.item(*key, bounds.x);
                    if shown < REST_DISTANCE {
                        continue;
                    }
                    renderer.with_translation(Vector::new(dx, 0.0), |renderer| {
                        item.as_widget()
                            .draw(tree, renderer, theme, style, child, cursor, viewport);
                        // In the item's moved coordinates, the shape is the other way.
                        fade(renderer, bounds, shape - Vector::new(dx, 0.0), shown);
                    });
                }
                if let Some((panel, tree, layout)) = panel {
                    let shown = motion.popover.panel.value;
                    if shown >= REST_DISTANCE {
                        panel.as_widget().draw(
                            tree,
                            renderer,
                            theme,
                            style,
                            layout,
                            cursor,
                            &layout.bounds(),
                        );
                        fade(renderer, layout.bounds(), shape, shown);
                    }
                }
            });
        });
    });
}

/// Fades what was just drawn in `bounds` toward the island's background. iced has no group
/// opacity, but the island is opaque, so covering content with its own color is the same thing.
/// Kept inside `shape`: a nested layer isn't clipped by the one around it.
fn fade(renderer: &mut iced::Renderer, bounds: Rectangle, shape: Rectangle, shown: f32) {
    use iced::advanced::Renderer as _;

    let Some(bounds) = bounds
        .intersection(&shape)
        .filter(|_| shown <= 1.0 - REST_DISTANCE)
    else {
        return;
    };
    // A layer of its own, so it lands over the text, which draws after quads within a layer.
    renderer.with_layer(bounds, |renderer| {
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            Color {
                a: 1.0 - shown,
                ..theme::ISLAND_BG
            },
        );
    });
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Island<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            motion: Motion::default(),
            shape: Cache::new(),
            panel: None,
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Fixed(theme::ISLAND_HEIGHT as f32))
    }

    /// A row of items, centered vertically, with padding at the ends.
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let height = theme::ISLAND_HEIGHT as f32;
        let padding = f32::from(theme::ISLAND_PADDING);
        let spacing = theme::ITEM_SPACING as f32;
        let limits = layout::Limits::new(Size::new(0.0, height), Size::new(f32::INFINITY, height));
        let mut x = padding;
        let children: Vec<_> = self
            .items
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(item, tree)| {
                let node = item.as_widget_mut().layout(tree, renderer, &limits);
                let size = node.size();
                let node = node.move_to(Point::new(x, ((height - size.height) / 2.0).round()));
                x += size.width + spacing;
                node
            })
            .collect();
        let width = if children.is_empty() {
            2.0 * padding
        } else {
            x - spacing + padding
        };
        layout::Node::with_children(Size::new(width, height), children)
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
        // A popover's overlay draws the whole island, over everything else.
        if self.popover.is_some() {
            return;
        }
        draw_island(
            tree.state.downcast_ref::<State>(),
            &self.keys,
            &self.items,
            &tree.children,
            layout,
            None,
            renderer,
            theme,
            style,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        self.items.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.items);
        let panel = &mut tree.state.downcast_mut::<State>().panel;
        match (&self.popover, panel.as_mut()) {
            (Some(popover), Some(tree)) => tree.diff(&popover.content),
            (Some(popover), None) => *panel = Some(Tree::new(&popover.content)),
            (None, _) => *panel = None,
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((item, tree), layout) in self
            .items
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            item.as_widget_mut()
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
        // Items under a popover are hidden, and stay out of it.
        if self.popover.is_none() {
            for ((item, tree), layout) in self
                .items
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                item.as_widget_mut().update(
                    tree, event, layout, cursor, renderer, clipboard, shell, viewport,
                );
            }
        }

        let motion = &mut tree.state.downcast_mut::<State>().motion;
        let positions = self
            .keys
            .iter()
            .zip(layout.children())
            .map(|(key, child)| (*key, child.bounds().x));
        motion.arrange(
            layout.bounds(),
            positions,
            self.popover.as_ref().map(|popover| popover.open),
        );

        match event {
            // Watched whether or not an item handled it: the island reacts to every press on it.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if self.popover.is_none() =>
            {
                if let Some(at) = cursor.position_over(layout.bounds()) {
                    motion.press(at);
                }
            }
            // While the button is held, the compositor keeps sending motion even off the surface.
            Event::Mouse(mouse::Event::CursorMoved { position }) if motion.press.is_some() => {
                motion.drag(*position);
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Mouse(mouse::Event::CursorLeft)
                if motion.press.is_some() =>
            {
                motion.release();
            }
            // Every frame starts with this event.
            Event::Window(window::Event::RedrawRequested(now)) => {
                motion.step(*now);
            }
            _ => {}
        }
        if let Some(popover) = &self.popover
            && motion.closing_done()
        {
            shell.publish(popover.on_closed.clone());
        }
        if !motion.at_rest() {
            shell.request_redraw();
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
        if self.popover.is_some() {
            return mouse::Interaction::None;
        }
        self.items
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((item, tree), layout)| {
                item.as_widget()
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
        if self.popover.is_none() {
            return overlay::from_children(
                &mut self.items,
                tree,
                layout,
                renderer,
                viewport,
                translation,
            );
        }
        Some(overlay::Element::new(Box::new(Morph {
            island: self,
            tree,
            layout,
            translation,
        })))
    }
}

impl<'a, M: Clone + 'a> From<Island<'a, M>> for Element<'a, M> {
    fn from(island: Island<'a, M>) -> Self {
        Element::new(island)
    }
}

/// The island while it has a popover: drawn whole, panel included, above everything else.
struct Morph<'a, 'b, M> {
    island: &'b mut Island<'a, M>,
    tree: &'b mut Tree,
    /// The island's own layout in the bar.
    layout: Layout<'b>,
    translation: Vector,
}

impl<M: Clone> overlay::Overlay<M, iced::Theme, iced::Renderer> for Morph<'_, '_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let popover = self.island.popover.as_mut().expect("a morph has a popover");
        let state = self.tree.state.downcast_mut::<State>();
        let panel = state
            .panel
            .get_or_insert_with(|| Tree::new(&popover.content));
        // Unbounded and compressed: the content takes its natural size, and `Fill` children (a
        // separator) stretch to their siblings, not to infinity.
        let natural =
            layout::Limits::with_compression(Size::ZERO, Size::INFINITE, Size::new(true, true));
        let content = popover
            .content
            .as_widget_mut()
            .layout(panel, renderer, &natural);

        let island = self.layout.bounds() + self.translation;
        let rect = place(island, content.size(), self.island.align, bounds.width);
        let popover = &mut state.motion.popover;
        popover.rect = Some(rect);
        popover.fits = rect.y + rect.height <= bounds.height;

        let offset = panel_offset(rect.size(), content.size());
        let content = content.move_to(Point::ORIGIN + offset);
        layout::Node::with_children(rect.size(), vec![content]).move_to(rect.position())
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let island = &*self.island;
        let state = self.tree.state.downcast_ref::<State>();
        let (Some(popover), Some(panel)) = (&island.popover, &state.panel) else {
            return;
        };
        let content = layout.children().next().expect("a morph has its panel");
        draw_island(
            state,
            &island.keys,
            &island.items,
            &self.tree.children,
            self.layout,
            Some((&popover.content, panel, content)),
            renderer,
            theme,
            style,
            cursor,
            &layout.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let popover = self.island.popover.as_mut().expect("a morph has a popover");
        let state = self.tree.state.downcast_mut::<State>();
        let rect = layout.bounds();
        if state.motion.popover.reported != Some(rect) {
            state.motion.popover.reported = Some(rect);
            shell.publish((popover.on_placed)(rect));
        }
        let Some(panel) = &mut state.panel else {
            return;
        };
        let shown = state.motion.popover.panel.target > 0.0;
        if popover.open && shown {
            let content = layout.children().next().expect("a morph has its panel");
            popover.content.as_widget_mut().update(
                panel, event, content, cursor, renderer, clipboard, shell, &rect,
            );
        }
        // A press anywhere on the shape is the popover's, handled or not: it isn't a click on the
        // bar, which would close it.
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && cursor.is_over(state.motion.edges.rect())
        {
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = self.tree.state.downcast_ref::<State>();
        if !cursor.is_over(state.motion.edges.rect()) {
            return mouse::Interaction::None;
        }
        let inner = match (&self.island.popover, &state.panel) {
            (Some(popover), Some(panel)) if popover.open => {
                let content = layout.children().next().expect("a morph has its panel");
                popover.content.as_widget().mouse_interaction(
                    panel,
                    content,
                    cursor,
                    &layout.bounds(),
                    renderer,
                )
            }
            _ => mouse::Interaction::None,
        };
        // Anything but `None`, so whatever is under the shape doesn't see the pointer.
        inner.max(mouse::Interaction::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    /// Steps frame by frame until the motion asks for no more; returns the frames taken.
    fn run(motion: &mut Motion, start: Instant, mut each: impl FnMut(&Motion)) -> usize {
        let mut now = start;
        for frame in 0..1000 {
            if !motion.step(now) {
                return frame;
            }
            each(motion);
            now += FRAME;
        }
        panic!("never came to rest");
    }

    fn rect(x: f32, width: f32) -> Rectangle {
        Rectangle::new(Point::new(x, 6.0), Size::new(width, 32.0))
    }

    /// An island already laid out and at rest at `island`, with items `keys` at 10, 50, 90…
    fn settled(island: Rectangle, keys: &[u64]) -> Motion {
        let mut motion = Motion::default();
        let positions = keys
            .iter()
            .zip(0..)
            .map(|(key, i)| (*key, 10.0 + 40.0 * i as f32));
        motion.arrange(island, positions, None);
        run(&mut motion, Instant::now(), |_| {});
        motion
    }

    #[test]
    fn at_rest_asks_for_no_frames() {
        let mut motion = Motion::default();
        assert!(!motion.step(Instant::now()));
        assert_eq!(motion.last, None);
    }

    #[test]
    fn a_new_island_grows_from_its_center() {
        let mut motion = Motion::default();
        motion.arrange(rect(100.0, 200.0), [(1, 110.0)].into_iter(), None);
        let first = motion.edges.rect();
        assert_eq!(first.width, 0.0);
        assert_eq!(first.center_x(), 200.0);
        let mut wider = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            let width = motion.edges.rect().width;
            assert!(width >= wider - 1.0, "only grows: {width} after {wider}");
            wider = width;
        });
        assert_eq!(motion.edges.rect(), rect(100.0, 200.0));
        assert_eq!(motion.item(1, 110.0), (0.0, 1.0));
    }

    #[test]
    fn a_change_of_size_morphs_without_wobbling() {
        let mut motion = settled(rect(100.0, 200.0), &[1]);
        motion.arrange(rect(100.0, 300.0), [(1, 110.0)].into_iter(), None);
        assert_eq!(motion.edges.rect(), rect(100.0, 200.0), "doesn't snap");
        let mut widest: f32 = 0.0;
        let frames = run(&mut motion, Instant::now(), |motion| {
            widest = widest.max(motion.edges.rect().width);
        });
        assert!(frames > 5, "takes a moment: {frames} frames");
        assert!(widest < 301.0, "no overshoot past a pixel: {widest}");
        assert_eq!(motion.edges.rect(), rect(100.0, 300.0));
    }

    #[test]
    fn items_are_followed_by_key() {
        let mut motion = settled(rect(0.0, 200.0), &[1, 2, 3]);
        // 1 goes, 4 comes in between 2 and 3, which both move.
        motion.arrange(
            rect(0.0, 200.0),
            [(2, 10.0), (4, 50.0), (3, 90.0)].into_iter(),
            None,
        );
        assert_eq!(motion.item(2, 10.0), (40.0, 1.0), "2 slides from 50");
        assert_eq!(
            motion.item(4, 50.0),
            (0.0, 0.0),
            "4 starts hidden, in place"
        );
        assert_eq!(motion.item(3, 90.0), (0.0, 1.0), "3 stays");
        assert!(motion.items.iter().all(|item| item.key != 1));
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.item(2, 10.0), (0.0, 1.0));
        assert_eq!(motion.item(4, 50.0), (0.0, 1.0));
    }

    fn popover(motion: &mut Motion, open: bool, panel: Rectangle, fits: bool) {
        motion.popover.rect = Some(panel);
        motion.popover.fits = fits;
        let island = motion.island.unwrap();
        motion.arrange(island, [(1, 10.0)].into_iter(), Some(open));
    }

    #[test]
    fn a_popover_waits_for_room_then_morphs_and_closes_back() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let mut motion = settled(island, &[1]);

        // The surface hasn't grown yet: nothing moves.
        popover(&mut motion, true, panel, false);
        assert!(!motion.step(Instant::now()));
        assert_eq!(motion.edges.rect(), island);

        popover(&mut motion, true, panel, true);
        let mut panel_before_halfway = false;
        run(&mut motion, Instant::now(), |motion| {
            if motion.edges.progress(island, panel) < 0.4 && motion.popover.panel.value > 0.01 {
                panel_before_halfway = true;
            }
        });
        assert!(!panel_before_halfway, "the panel waits for the shape");
        assert_eq!(motion.edges.rect(), panel);
        assert_eq!(motion.item(1, 10.0), (0.0, 0.0), "the items made way");
        assert_eq!(motion.popover.panel.value, 1.0);
        assert!(!motion.closing_done(), "open, not closing");

        popover(&mut motion, false, panel, true);
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.edges.rect(), island);
        assert_eq!(motion.item(1, 10.0), (0.0, 1.0));
        assert_eq!(motion.popover.panel.value, 0.0);
        assert!(motion.closing_done());
        assert!(!motion.closing_done(), "said only once");
    }

    #[test]
    fn reopening_while_closing_turns_around() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        run(&mut motion, Instant::now(), |_| {});
        popover(&mut motion, false, panel, true);
        let start = Instant::now();
        for frame in 0..5 {
            motion.step(start + FRAME * frame);
        }
        popover(&mut motion, true, panel, true);
        run(&mut motion, start + FRAME * 5, |_| {});
        assert_eq!(motion.edges.rect(), panel);
        assert!(!motion.closing_done());
    }

    #[test]
    fn popovers_grow_from_their_island_and_stay_on_screen() {
        let panel = Size::new(200.0, 100.0);
        let pad = 2.0 * f32::from(theme::PANEL_PADDING);
        let gap = f32::from(theme::EDGE_GAP);
        let width = 200.0 + pad;

        let left = place(rect(6.0, 100.0), panel, Horizontal::Left, 1000.0);
        assert_eq!((left.x, left.y), (6.0, 6.0));
        assert_eq!(left.size(), Size::new(width, 100.0 + pad));

        let right = place(rect(894.0, 100.0), panel, Horizontal::Right, 1000.0);
        assert_eq!(right.x + right.width, 994.0);

        let center = place(rect(450.0, 100.0), panel, Horizontal::Center, 1000.0);
        assert_eq!(center.center_x(), 500.0);

        // Never narrower than the island.
        let wide = place(rect(300.0, 400.0), panel, Horizontal::Center, 1000.0);
        assert_eq!(wide.width, 400.0);

        // Kept on screen.
        let clamped = place(rect(950.0, 44.0), panel, Horizontal::Left, 1000.0);
        assert_eq!(clamped.x + clamped.width, 1000.0 - gap);
    }

    #[test]
    fn a_press_squeezes_then_holds_still() {
        let mut motion = Motion::default();
        motion.press(Point::new(10.0, 10.0));
        let frames = run(&mut motion, Instant::now(), |_| {});
        assert!(frames > 1 && frames < 60, "settled in {frames} frames");
        assert_eq!(motion.inset.value, theme::PRESS_INSET);
        assert_eq!(motion.last, None);
    }

    #[test]
    fn letting_go_overshoots_once_then_settles_in_place() {
        let mut motion = Motion::default();
        motion.press(Point::new(0.0, 0.0));
        motion.drag(Point::new(200.0, 0.0));
        run(&mut motion, Instant::now(), |_| {});
        let pulled = motion.x.value;
        assert!(pulled > 0.0 && pulled < theme::DRAG_LIMIT_X);

        motion.release();
        let mut lowest = f32::MAX;
        run(&mut motion, Instant::now(), |motion| {
            lowest = lowest.min(motion.x.value)
        });
        assert!(lowest < 0.0, "springs past its place: {lowest}");
        assert!(lowest > -pulled / 2.0, "but only a little: {lowest}");
        assert_eq!(motion.offset(), Vector::ZERO);
        assert_eq!(motion.inset.value, 0.0);
    }

    #[test]
    fn a_long_stall_does_not_throw_the_island() {
        let mut motion = Motion::default();
        motion.press(Point::ORIGIN);
        motion.drag(Point::new(100.0, 100.0));
        let start = Instant::now();
        motion.step(start);
        motion.step(start + Duration::from_secs(30));
        for spring in [motion.x, motion.y, motion.inset] {
            assert!(spring.value.is_finite() && spring.value.abs() <= theme::DRAG_LIMIT_X * 2.0);
        }
    }

    #[test]
    fn rubber_band_gives_less_and_less() {
        assert_eq!(rubber_band(0.0, 10.0), 0.0);
        assert_eq!(rubber_band(-30.0, 10.0), -rubber_band(30.0, 10.0));
        let near = rubber_band(1.0, 10.0);
        assert!(
            (near - theme::DRAG_RESISTANCE).abs() < 0.05,
            "about 1:1 near zero: {near}"
        );
        let mut previous = 0.0;
        for distance in [1.0, 10.0, 100.0, 10_000.0] {
            let moved = rubber_band(distance, 10.0);
            assert!(moved > previous && moved < 10.0);
            previous = moved;
        }
    }
}
