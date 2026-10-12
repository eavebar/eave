//! The squircle: a rectangle with continuous corners, the shape of every island.

use iced::widget::canvas::{Path, path};
use iced::{Point, Rectangle, Size, Vector};

/// A rectangle with continuous ("squircle") corners: each corner is a short circular arc that
/// eases into the straight edges through a cubic on either side, instead of meeting them abruptly.
/// This is Figma's corner smoothing; `smoothing` 0 gives a plain rounded rectangle.
///
/// With a bump, the top or bottom of the shape is bent out around it, corners included, so the
/// bump looks the same wherever it is.
pub fn squircle(bounds: Rectangle, radius: f32, smoothing: f32, bump: Option<Bump>) -> Path {
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

    let path = Path::new(|builder| {
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
    });
    match bump.filter(|bump| bump.amount.abs() >= 0.01 && bump.width >= 1.0) {
        Some(bump) => warp(&path, bounds, bump),
        None => path,
    }
}

/// A bump in a squircle's top or bottom: the edge rises out of the shape (or dents into it) in
/// a raised cosine around `x`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bump {
    pub edge: Edge,
    /// Where its peak is, along the edge.
    pub x: f32,
    pub width: f32,
    /// How far its peak is pushed out of the shape; negative dents it in.
    pub amount: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
}

/// How far a flattened curve may stray from the real one, in logical pixels.
const WARP_TOLERANCE: f32 = 0.05;
/// The longest straight piece of a warped path: short enough that the bump reads as a curve.
const WARP_STEP: f32 = 1.0;

/// Bends `path` out around the bump. Every point moves straight up (or down) by the bump's height
/// at its `x`, fully on the bumped edge and less the further it is from it, down to nothing
/// halfway across the shape. So a bump near a corner lifts the corner's end along with it, and
/// the opposite edge never moves. The path is flattened to do it, so it's only worth it while a
/// bump shows.
fn warp(path: &Path, bounds: Rectangle, bump: Bump) -> Path {
    use path::lyon_path::Event;
    use path::lyon_path::iterator::PathIterator;

    let half = bump.width / 2.0;
    let reach = bounds.height / 2.0;
    let lift = |x: f32, y: f32| {
        let along = ((x - bump.x) / half).clamp(-1.0, 1.0);
        let bell = 0.5 * (1.0 + (std::f32::consts::PI * along).cos());
        let (from_edge, out) = match bump.edge {
            Edge::Top => (y - bounds.y, -bump.amount),
            Edge::Bottom => (bounds.y + bounds.height - y, bump.amount),
        };
        let near = (1.0 - from_edge / reach).clamp(0.0, 1.0);
        // Eased, so the bend has no crease where it starts.
        let near = near * near * (3.0 - 2.0 * near);
        Point::new(x, y + out * bell * near)
    };
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Where each segment of the path ends.
    fn ends(path: &Path) -> Vec<(f32, f32)> {
        path.raw()
            .iter()
            .map(|event| (event.to().x, event.to().y))
            .collect()
    }

    #[test]
    fn a_bump_pushes_out_of_its_edge_only() {
        let bounds = Rectangle::new(Point::new(0.0, 4.0), Size::new(120.0, 30.0));
        let up = Bump {
            edge: Edge::Top,
            x: 60.0,
            width: 28.0,
            amount: 3.0,
        };
        let points = ends(&squircle(bounds, 14.0, 0.6, Some(up)));
        let highest = points
            .iter()
            .copied()
            .fold((0.0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
        assert!(
            (highest.0 - 60.0).abs() < 0.5 && (highest.1 - 1.0).abs() < 1e-3,
            "{highest:?}"
        );
        assert!(
            points
                .iter()
                .all(|(_, y)| (1.0 - 1e-3..=34.0 + 1e-3).contains(y))
        );
        // Away from it, the edge stays put.
        assert!(
            points
                .iter()
                .any(|&(x, y)| x > 80.0 && x < 100.0 && y == 4.0)
        );

        let down = Bump {
            edge: Edge::Bottom,
            ..up
        };
        let points = ends(&squircle(bounds, 14.0, 0.6, Some(down)));
        assert!(
            points
                .iter()
                .any(|&(x, y)| (x - 60.0).abs() < 0.5 && (y - 37.0).abs() < 1e-3)
        );
        assert!(
            points
                .iter()
                .all(|(_, y)| (4.0 - 1e-3..=37.0 + 1e-3).contains(y))
        );
    }

    #[test]
    fn a_bump_by_a_corner_bends_the_corner_too() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(120.0, 30.0));
        let flat = ends(&squircle(bounds, 14.0, 0.6, None));
        let bump = Bump {
            edge: Edge::Top,
            x: 20.0,
            width: 64.0,
            amount: 3.0,
        };
        let points = ends(&squircle(bounds, 14.0, 0.6, Some(bump)));
        // The peak stays over the item, though the corner reaches 15 in.
        let highest = points
            .iter()
            .copied()
            .fold((0.0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
        assert!(
            (highest.0 - 20.0).abs() < 0.5 && (highest.1 + 3.0).abs() < 1e-3,
            "{highest:?}"
        );
        // The corner's end rises along with it; its far end, halfway down the side, doesn't.
        assert!(points.iter().any(|&(x, y)| x < 15.0 && y < -1.0));
        let side = |points: &[(f32, f32)]| {
            points
                .iter()
                .copied()
                .filter(|(x, _)| *x < 1e-3)
                .map(|(_, y)| y)
                .fold(f32::MAX, f32::min)
        };
        assert!((side(&points) - side(&flat)).abs() < 0.2);
        // The bottom never moves.
        assert!(points.iter().all(|(_, y)| *y <= 30.0 + 1e-3));
    }
}
