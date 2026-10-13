//! The squircle: a rectangle with continuous corners, the shape of every island. Its corners are
//! also the screen's and the notch's, which `eave calibrate` measures (D-71): the same curve here
//! and there, so what it measures is what the bar draws.

use iced::widget::canvas::{Path, path};
use iced::{Point, Rectangle, Vector};

/// How one corner is rounded: `smoothing` 0 is a circle's quarter, above it a squircle's, up to 1.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CornerShape {
    pub radius: f32,
    pub smoothing: f32,
}

impl CornerShape {
    pub const SQUARE: Self = Self {
        radius: 0.0,
        smoothing: 0.0,
    };

    pub fn new(radius: f32, smoothing: f32) -> Self {
        Self { radius, smoothing }
    }
}

/// What a path is drawn into: iced's builder for the bar, tiny-skia's for the calibrator's
/// pattern.
pub trait Sink {
    fn move_to(&mut self, to: Point);
    fn line_to(&mut self, to: Point);
    fn cubic_to(&mut self, a: Point, b: Point, to: Point);
    fn close(&mut self);
}

impl Sink for path::Builder {
    fn move_to(&mut self, to: Point) {
        path::Builder::move_to(self, to);
    }
    fn line_to(&mut self, to: Point) {
        path::Builder::line_to(self, to);
    }
    fn cubic_to(&mut self, a: Point, b: Point, to: Point) {
        self.bezier_curve_to(a, b, to);
    }
    fn close(&mut self) {
        path::Builder::close(self);
    }
}

/// A rectangle with continuous ("squircle") corners: each corner is a short circular arc that
/// eases into the straight edges through a cubic on either side, instead of meeting them abruptly.
/// This is Figma's corner smoothing; `smoothing` 0 gives a plain rounded rectangle.
pub fn squircle(bounds: Rectangle, radius: f32, smoothing: f32) -> Path {
    squircle_corners(bounds, [CornerShape::new(radius, smoothing); 4])
}

/// A squircle with each corner its own: top left, top right, bottom right, bottom left.
pub fn squircle_corners(bounds: Rectangle, corners: [CornerShape; 4]) -> Path {
    Path::new(|builder| rectangle(builder, bounds, corners))
}

/// A rounded rectangle into `sink`, clockwise from its top right corner, closed. Its corners are
/// top left, top right, bottom right, bottom left.
pub fn rectangle(sink: &mut impl Sink, bounds: Rectangle, corners: [CornerShape; 4]) {
    let (left, top) = (bounds.x, bounds.y);
    let (right, bottom) = (left + bounds.width, top + bounds.height);
    // Nothing may reach past the middle of the shorter side.
    let budget = bounds.width.min(bounds.height) / 2.0;
    let [top_left, top_right, bottom_right, bottom_left] = corners;
    // Clockwise from the top right. Each corner: its point, the direction of the edge coming in,
    // and of the edge going out.
    let corners = [
        (top_right, Point::new(right, top), RIGHT, DOWN),
        (bottom_right, Point::new(right, bottom), DOWN, LEFT),
        (bottom_left, Point::new(left, bottom), LEFT, UP),
        (top_left, Point::new(left, top), UP, RIGHT),
    ];
    for (index, (shape, at, incoming, outgoing)) in corners.into_iter().enumerate() {
        let corner = Corner::new(shape.radius, shape.smoothing, budget);
        corner_into(sink, &corner, at, incoming, outgoing, index == 0);
    }
    sink.close();
}

pub const RIGHT: Vector = Vector::new(1.0, 0.0);
pub const DOWN: Vector = Vector::new(0.0, 1.0);
pub const LEFT: Vector = Vector::new(-1.0, 0.0);
pub const UP: Vector = Vector::new(0.0, -1.0);

/// One corner into `sink`: at `at`, where an edge going `incoming` turns to go `outgoing`. It turns
/// either way, so the same corner rounds a shape's outside or, turning back, its inside. It starts
/// with a line to where it begins, or a move there if it's the `first` thing drawn.
pub fn corner_into(
    sink: &mut impl Sink,
    corner: &Corner,
    at: Point,
    incoming: Vector,
    outgoing: Vector,
    first: bool,
) {
    let place = |(x, y): (f32, f32)| at + incoming * x + outgoing * y;
    let [start, curve_in, arc, curve_out] = corner.points();
    if first {
        sink.move_to(place(start[0]));
    } else {
        sink.line_to(place(start[0]));
    }
    sink.cubic_to(place(curve_in[0]), place(curve_in[1]), place(curve_in[2]));
    if corner.arc_radius > 0.0 {
        // A circular arc as one cubic: its handles lie on the tangents at its ends, `4/3 tan(θ/4)`
        // radii along them, where the tangents meet `tan(θ/2)` radii out.
        let theta = corner.arc_degrees.to_radians();
        let k = (4.0 / 3.0) * (theta / 4.0).tan() / (theta / 2.0).tan();
        let (from, meet, to) = (place(curve_in[2]), place(arc[0]), place(arc[1]));
        sink.cubic_to(from + (meet - from) * k, to + (meet - to) * k, to);
    }
    sink.cubic_to(
        place(curve_out[0]),
        place(curve_out[1]),
        place(curve_out[2]),
    );
}

/// How far a flattened curve may stray from the real one, in logical pixels.
const WARP_TOLERANCE: f32 = 0.05;
/// The longest straight piece of a warped path: short enough that a bend reads as a curve.
const WARP_STEP: f32 = 1.0;

/// `path` with every point moved by `map`. The path is flattened and its lines cut into short
/// pieces to do it, so they can bend, which makes it only worth it while something bends it.
pub fn warp(path: &Path, map: impl Fn(Point) -> Point) -> Path {
    use path::lyon_path::Event;
    use path::lyon_path::iterator::PathIterator;

    let lift = |x: f32, y: f32| map(Point::new(x, y));
    // Straight lines are cut into short pieces, so they can bend.
    let line = |builder: &mut path::Builder, from: Point, to: Point| {
        let pieces = (from.distance(to) / WARP_STEP).ceil().max(1.0);
        for piece in 1..=pieces as u32 {
            let t = piece as f32 / pieces;
            builder.line_to(lift(
                from.x + (to.x - from.x) * t,
                from.y + (to.y - from.y) * t,
            ));
        }
    };
    let point = |p: path::lyon_path::math::Point| Point::new(p.x, p.y);

    Path::new(|builder| {
        for event in path.raw().iter().flattened(WARP_TOLERANCE) {
            match event {
                Event::Begin { at } => builder.move_to(lift(at.x, at.y)),
                Event::Line { from, to } => line(builder, point(from), point(to)),
                Event::End { last, first, close } => {
                    if close {
                        line(builder, point(last), point(first));
                        builder.close();
                    }
                }
                // Flattening leaves only lines.
                Event::Quadratic { .. } | Event::Cubic { .. } => {}
            }
        }
    })
}

/// The shape of one corner, in coordinates relative to the corner point: `x` along the edge
/// coming in (negative before the corner), `y` along the edge going out.
#[derive(Debug)]
pub struct Corner {
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
    /// How far the arc turns, in degrees.
    arc_degrees: f32,
}

impl Corner {
    /// A corner of `radius`, eased by `smoothing`, that reaches no further than `budget` along
    /// either edge.
    pub fn new(radius: f32, smoothing: f32, budget: f32) -> Self {
        let budget = budget.max(0.0);
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
            arc_degrees,
        }
    }

    /// How far along each edge the corner reaches.
    pub fn extent(&self) -> f32 {
        self.extent
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

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    #[test]
    fn corners_never_reach_past_the_middle() {
        let corner = Corner::new(12.0, 0.6, 16.0);
        assert!(corner.extent <= 16.0);
        let corner = Corner::new(40.0, 0.6, 16.0);
        assert_eq!(corner.extent, 16.0);
        assert_eq!(corner.arc_radius, 16.0);
    }

    #[test]
    fn corner_curves_meet_the_edges_and_the_arc() {
        let corner = Corner::new(10.0, 0.6, 16.0);
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
        let corner = Corner::new(14.0, 0.6, 16.0);
        assert_eq!(corner.extent, 16.0);
        assert!(corner.a > 0.0 && corner.b > 0.0);
        let reach = corner.a + corner.b + corner.c + corner.d + corner.arc;
        assert!((reach - corner.extent).abs() < 1e-4, "{reach}");
        // The arc covers 36°, not the 90° of a plain rounded corner.
        assert!(corner.arc < 14.0 * 0.5);
    }

    #[test]
    fn no_smoothing_is_a_rounded_rectangle() {
        let corner = Corner::new(8.0, 0.0, 16.0);
        assert_eq!(corner.extent, 8.0);
        let [_, _, arc, _] = corner.points();
        // The tangents meet at the corner itself.
        assert!(arc[0].0.abs() < 1e-4 && arc[0].1.abs() < 1e-4);
    }

    /// Every point a path is drawn through, handles too.
    #[derive(Default)]
    struct Points(Vec<Point>);

    impl Sink for Points {
        fn move_to(&mut self, to: Point) {
            self.0.push(to);
        }
        fn line_to(&mut self, to: Point) {
            self.0.push(to);
        }
        fn cubic_to(&mut self, a: Point, b: Point, to: Point) {
            self.0.extend([a, b, to]);
        }
        fn close(&mut self) {}
    }

    #[test]
    fn the_arc_as_a_cubic_stays_on_its_circle() {
        // A plain quarter circle of radius 10 at the origin's corner, centered at (-10, 10).
        let corner = Corner::new(10.0, 0.0, 50.0);
        let mut points = Points::default();
        corner_into(&mut points, &corner, Point::ORIGIN, RIGHT, DOWN, true);
        let [from, a, b, to] = [points.0[3], points.0[4], points.0[5], points.0[6]];
        // The cubic's middle, at t = 1/2.
        let middle = Point::new(
            (from.x + 3.0 * a.x + 3.0 * b.x + to.x) / 8.0,
            (from.y + 3.0 * a.y + 3.0 * b.y + to.y) / 8.0,
        );
        let distance = middle.distance(Point::new(-10.0, 10.0));
        assert!((distance - 10.0).abs() < 0.01, "{distance}");
    }

    #[test]
    fn a_corner_turning_back_is_the_mirror_of_one_turning_on() {
        let corner = Corner::new(8.0, 0.6, 50.0);
        let (mut on, mut back) = (Points::default(), Points::default());
        // Right then down rounds an outside corner; right then up, the same corner flipped.
        corner_into(&mut on, &corner, Point::ORIGIN, RIGHT, DOWN, true);
        corner_into(&mut back, &corner, Point::ORIGIN, RIGHT, UP, true);
        for (a, b) in on.0.iter().zip(&back.0) {
            assert!(
                (a.x - b.x).abs() < 1e-4 && (a.y + b.y).abs() < 1e-4,
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn each_corner_keeps_its_own_radius() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(100.0, 40.0));
        let mut points = Points::default();
        let round = CornerShape::new(30.0, 0.0);
        rectangle(
            &mut points,
            bounds,
            [
                round,
                CornerShape::SQUARE,
                CornerShape::new(5.0, 0.0),
                round,
            ],
        );
        // The square top right corner starts right at its point.
        assert_eq!(points.0[0], Point::new(100.0, 0.0));
        // The bottom right one reaches 5 in; the round ones only to the middle of the short side.
        assert!(points.0.contains(&Point::new(100.0, 35.0)));
        assert!(points.0.contains(&Point::new(0.0, 20.0)));
        assert!(!points.0.iter().any(|p| p.y > 40.0 + 1e-4 || p.x < -1e-4));
    }

    /// Where each segment of the path ends.
    fn ends(path: &Path) -> Vec<(f32, f32)> {
        path.raw()
            .iter()
            .map(|event| (event.to().x, event.to().y))
            .collect()
    }

    #[test]
    fn a_warp_bends_only_what_it_moves() {
        let bounds = Rectangle::new(Point::new(0.0, 4.0), Size::new(120.0, 30.0));
        // Lifts the top's middle by up to 3 px, and nothing else.
        let lift = |at: Point| {
            let bell = (1.0 - ((at.x - 60.0) / 20.0).abs()).max(0.0);
            let near = if at.y < 5.0 { 1.0 } else { 0.0 };
            Point::new(at.x, at.y - 3.0 * bell * near)
        };
        let points = ends(&warp(&squircle(bounds, 14.0, 0.6), lift));
        let highest = points
            .iter()
            .copied()
            .fold((0.0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
        assert!(
            (highest.0 - 60.0).abs() < 0.5 && (highest.1 - 1.0).abs() < 1e-3,
            "{highest:?}"
        );
        // The straight top was cut into pieces, so it bends; away from the lift it stays put.
        assert!(
            points
                .iter()
                .any(|&(x, y)| x > 80.0 && x < 100.0 && y == 4.0)
        );
        assert!(
            points
                .iter()
                .all(|(_, y)| (1.0 - 1e-3..=34.0 + 1e-3).contains(y))
        );
    }
}
