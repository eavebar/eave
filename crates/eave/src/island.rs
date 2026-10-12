//! An island: the squircle behind a zone's items. It reacts to the pointer like something
//! physical. A press squeezes it in a little. Dragging pulls it along with growing resistance,
//! and letting go springs it back into place. It only watches events and never captures them, so
//! the items inside work exactly as they would without it. Like the switcher, it steps one frame
//! at a time when the compositor asks, and stops asking once everything is at rest.

use std::time::{Duration, Instant};

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::canvas::{Cache, Path, Stroke};
use iced::{Element, Event, Length, Point, Rectangle, Size, Transformation, Vector, window};

use crate::theme;

pub struct Island<'a, M> {
    content: Element<'a, M>,
}

impl<'a, M> Island<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

struct State {
    motion: Motion,
    /// The squircle, rebuilt only when the island changes size. Animation frames just move it.
    shape: Cache,
}

/// Where the island is pulled to and how it is moving there. Everything is in logical pixels.
#[derive(Debug, Default)]
struct Motion {
    /// Where the pointer went down, while the primary button is held.
    press: Option<Point>,
    x: Spring,
    y: Spring,
    /// How far each side has moved in.
    inset: Spring,
    /// The previous frame, while something is moving.
    last: Option<Instant>,
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

    fn at_rest(&self) -> bool {
        [&self.x, &self.y, &self.inset]
            .iter()
            .all(|spring| spring.at_rest())
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        if self.at_rest() {
            self.last = None;
            return false;
        }
        // The first frame after rest has no elapsed time to cover; a long stall is cut short so
        // the springs pick up where they were instead of jumping.
        let elapsed = self
            .last
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last))
            .min(MAX_FRAME);
        self.last = Some(now);
        for spring in [&mut self.x, &mut self.y, &mut self.inset] {
            spring.advance(elapsed.as_secs_f32());
        }
        if self.at_rest() {
            for spring in [&mut self.x, &mut self.y, &mut self.inset] {
                spring.settle();
            }
            self.last = None;
            return false;
        }
        true
    }

    fn offset(&self) -> Vector {
        Vector::new(self.x.value, self.y.value)
    }
}

/// The longest step a single frame may take.
const MAX_FRAME: Duration = Duration::from_millis(50);
/// Springs are integrated in steps no longer than this, so they stay stable at any frame rate.
const SUBSTEP: f32 = 1.0 / 480.0;
/// Close enough to the target, and slow enough, to stop: well under a physical pixel.
const REST_DISTANCE: f32 = 0.02;
const REST_SPEED: f32 = 0.5;

/// A damped spring along one axis.
#[derive(Debug, Default, Clone, Copy)]
struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
}

impl Spring {
    fn advance(&mut self, seconds: f32) {
        let stiffness = theme::SPRING_STIFFNESS;
        let damping = 2.0 * theme::SPRING_DAMPING * stiffness.sqrt();
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

/// A rectangle with continuous ("squircle") corners: each corner is a short circular arc that
/// eases into the straight edges through a cubic on either side, instead of meeting them abruptly.
/// This is Figma's corner smoothing; `smoothing` 0 gives a plain rounded rectangle.
fn squircle(bounds: Rectangle, radius: f32, smoothing: f32) -> Path {
    let corner = Corner::new(bounds.size(), radius, smoothing);
    let (left, top) = (bounds.x, bounds.y);
    let (right, bottom) = (left + bounds.width, top + bounds.height);
    // Clockwise from the top right. Each corner: its point, the direction of the edge coming in,
    // and of the edge going out.
    let corners = [
        (
            Point::new(right, top),
            Vector::new(1.0, 0.0),
            Vector::new(0.0, 1.0),
        ),
        (
            Point::new(right, bottom),
            Vector::new(0.0, 1.0),
            Vector::new(-1.0, 0.0),
        ),
        (
            Point::new(left, bottom),
            Vector::new(-1.0, 0.0),
            Vector::new(0.0, -1.0),
        ),
        (
            Point::new(left, top),
            Vector::new(0.0, -1.0),
            Vector::new(1.0, 0.0),
        ),
    ];

    Path::new(|builder| {
        for (index, (at, incoming, outgoing)) in corners.into_iter().enumerate() {
            let place = |(x, y): (f32, f32)| at + incoming * x + outgoing * y;
            let [start, curve_in, arc, curve_out] = corner.points();
            if index == 0 {
                builder.move_to(place(start[0]));
            } else {
                builder.line_to(place(start[0]));
            }
            builder.bezier_curve_to(place(curve_in[0]), place(curve_in[1]), place(curve_in[2]));
            if corner.arc_radius > 0.0 {
                builder.arc_to(place(arc[0]), place(arc[1]), corner.arc_radius);
            }
            builder.bezier_curve_to(
                place(curve_out[0]),
                place(curve_out[1]),
                place(curve_out[2]),
            );
        }
        builder.close();
    })
}

/// The shape of one corner, in coordinates relative to the corner point: `x` along the edge
/// coming in (negative before the corner), `y` along the edge going out.
#[derive(Debug)]
struct Corner {
    /// How far along each edge the corner reaches.
    extent: f32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    /// The length of the arc's chord, projected on each axis.
    arc: f32,
    /// Where the tangents at the arc's ends meet, on the corner's diagonal.
    tangents: f32,
    arc_radius: f32,
}

impl Corner {
    fn new(size: Size, radius: f32, smoothing: f32) -> Self {
        // Nothing may reach past the middle of the shorter side.
        let budget = size.width.min(size.height) / 2.0;
        let radius = radius.clamp(0.0, budget);
        let smoothing = smoothing.clamp(0.0, 1.0);
        let wanted = (1.0 + smoothing) * radius;
        let extent = wanted.min(budget);

        let arc_degrees = 90.0 * (1.0 - smoothing);
        let arc = (arc_degrees / 2.0_f32).to_radians().sin() * radius * std::f32::consts::SQRT_2;
        let alpha = (90.0 - arc_degrees) / 2.0;
        let to_arc = radius * (alpha / 2.0_f32).to_radians().tan();
        let beta = (45.0 * smoothing).to_radians();
        let c = to_arc * beta.cos();
        let d = c * beta.tan();
        let mut b = (wanted - arc - c - d) / 3.0;
        let mut a = 2.0 * b;
        // Too short for the full spread: keep the smoothing and squeeze the easing cubics into
        // what's left instead (Figma's "preserve smoothing"). The arc and the cubics' ends always
        // fit, since `arc + c + d` never exceeds the radius.
        if wanted > budget {
            let room = (budget - arc - c - d).max(0.0);
            b = b.min(room * 5.0 / 6.0);
            a = room - b;
        }
        // From the arc's center, the tangents meet `radius / cos(half the arc)` out along the
        // diagonal toward the corner.
        let half_arc = (arc_degrees / 2.0_f32).to_radians();
        let tangents = radius - radius / (std::f32::consts::SQRT_2 * half_arc.cos());

        Self {
            extent,
            a,
            b,
            c,
            d,
            arc,
            tangents,
            arc_radius: if arc_degrees > 0.5 { radius } else { 0.0 },
        }
    }

    /// The start of the corner on the incoming edge, the cubic into the arc, the arc (through the
    /// tangents' meeting point to its end) and the cubic out of it. Symmetric about the diagonal.
    fn points(&self) -> [[(f32, f32); 3]; 4] {
        let Self {
            extent,
            a,
            b,
            c,
            d,
            arc,
            tangents,
            ..
        } = *self;
        let unused = (0.0, 0.0);
        [
            [(-extent, 0.0), unused, unused],
            [(-extent + a, 0.0), (-extent + a + b, 0.0), (-(arc + d), d)],
            [(-tangents, tangents), (-d, arc + d), unused],
            [(0.0, arc + d + c), (0.0, arc + d + b + c), (0.0, extent)],
        ]
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Island<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            motion: Motion::default(),
            shape: Cache::new(),
        })
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
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        let bounds = layout.bounds();
        // Squeezed by the same few pixels whatever the island's width, about its center.
        let scale = (1.0 - 2.0 * state.motion.inset.value / bounds.width.max(1.0)).max(0.5);
        let center = Vector::new(bounds.center_x(), bounds.center_y());
        let transformation = Transformation::translate(center.x, center.y)
            * Transformation::scale(scale)
            * Transformation::translate(-center.x, -center.y);

        renderer.with_translation(state.motion.offset(), |renderer| {
            renderer.with_transformation(transformation, |renderer| {
                let shape = state.shape.draw(renderer, bounds.size(), |frame| {
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
                            .with_color(theme::PANEL_BORDER)
                            .with_width(1.0),
                    );
                });
                renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
                    renderer.draw_geometry(shape);
                });
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    viewport,
                );
            });
        });
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
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        // Watched whether or not an item handled it: the island reacts to every press on it.
        let motion = &mut tree.state.downcast_mut::<State>().motion;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(at) = cursor.position_over(layout.bounds()) {
                    motion.press(at);
                    shell.request_redraw();
                }
            }
            // While the button is held, the compositor keeps sending motion even off the surface.
            Event::Mouse(mouse::Event::CursorMoved { position }) if motion.press.is_some() => {
                motion.drag(*position);
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Mouse(mouse::Event::CursorLeft)
                if motion.press.is_some() =>
            {
                motion.release();
                shell.request_redraw();
            }
            // Every frame starts with this event.
            Event::Window(window::Event::RedrawRequested(now)) if motion.step(*now) => {
                shell.request_redraw();
            }
            _ => {}
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Island<'a, M>> for Element<'a, M> {
    fn from(island: Island<'a, M>) -> Self {
        Element::new(island)
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

    #[test]
    fn at_rest_asks_for_no_frames() {
        let mut motion = Motion::default();
        assert!(!motion.step(Instant::now()));
        assert_eq!(motion.last, None);
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

    #[test]
    fn corners_never_reach_past_the_middle() {
        let corner = Corner::new(Size::new(200.0, 32.0), 12.0, 0.6);
        assert!(corner.extent <= 16.0);
        let corner = Corner::new(Size::new(200.0, 32.0), 40.0, 0.6);
        assert_eq!(corner.extent, 16.0);
        assert_eq!(corner.arc_radius, 16.0);
    }

    #[test]
    fn corner_curves_meet_the_edges_and_the_arc() {
        let corner = Corner::new(Size::new(200.0, 32.0), 10.0, 0.6);
        let [start, curve_in, arc, curve_out] = corner.points();
        let close =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        assert!(close(start[0], (-corner.extent, 0.0)));
        // Into the arc and out of it, mirrored across the diagonal.
        assert!(close(arc[1], (-curve_in[2].1, -curve_in[2].0)));
        assert!(close(curve_out[2], (0.0, corner.extent)));
        // The arc's end lies on its circle, centered `radius` in from both edges.
        let r = corner.arc_radius;
        let distance = ((arc[1].0 + r).powi(2) + (arc[1].1 - r).powi(2)).sqrt();
        assert!((distance - r).abs() < 1e-3, "{distance}");
    }

    #[test]
    fn a_short_island_keeps_its_smoothing() {
        let corner = Corner::new(Size::new(200.0, 32.0), 14.0, 0.6);
        assert_eq!(corner.extent, 16.0);
        assert!(corner.a > 0.0 && corner.b > 0.0);
        let reach = corner.a + corner.b + corner.c + corner.d + corner.arc;
        assert!((reach - corner.extent).abs() < 1e-4, "{reach}");
        // The arc covers 36°, not the 90° of a plain rounded corner.
        assert!(corner.arc < 14.0 * 0.5);
    }

    #[test]
    fn no_smoothing_is_a_rounded_rectangle() {
        let corner = Corner::new(Size::new(100.0, 32.0), 8.0, 0.0);
        assert_eq!(corner.extent, 8.0);
        let [_, _, arc, _] = corner.points();
        // The tangents meet at the corner itself.
        assert!(arc[0].0.abs() < 1e-4 && arc[0].1.abs() < 1e-4);
    }
}
