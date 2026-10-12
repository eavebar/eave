//! The squircle: a rectangle with continuous corners, the shape of every island.

use iced::widget::canvas::Path;
use iced::{Point, Rectangle, Size, Vector};

/// A rectangle with continuous ("squircle") corners: each corner is a short circular arc that
/// eases into the straight edges through a cubic on either side, instead of meeting them abruptly.
/// This is Figma's corner smoothing; `smoothing` 0 gives a plain rounded rectangle.
pub fn squircle(bounds: Rectangle, radius: f32, smoothing: f32) -> Path {
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
}
