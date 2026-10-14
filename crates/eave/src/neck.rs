//! The neck between the notch and the dormer's drop (D-86): a bridge of black between two
//! circles, the root hidden in the notch and the ball coming out of it, curving in at the waist
//! like a drop of something thick. It thins as they part and is gone past a few radii: the drop
//! has pinched off. The bridge is the classic metaball one: the circles' tangent points, joined by
//! cubic curves whose handles follow each circle's tangent there.

use std::f32::consts::{FRAC_PI_2, PI};

use iced::widget::canvas::Path;
use iced::{Point, Vector};

use crate::theme;

/// A circle: its center and radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pub center: Point,
    pub radius: f32,
}

/// How far toward the other circle each end of the neck reaches round its own: 0 grips at the
/// nearest points, 1 at the widest the two circles' common tangents allow.
const SPREAD: f32 = 0.5;
/// How long the curves' handles are, before they're cut down while the circles are close.
const HANDLE: f32 = 2.4;

/// The bridge's outline: from the root's one side, down to the ball, across it and back up the
/// other side, as `[from, handle, handle, to]` for each of the two curves. `None` while either is
/// empty, while one holds the other, or once they're further apart than the root's radius and
/// `NECK_REACH` of the ball's.
pub fn bridge(root: Circle, ball: Circle) -> Option<[[Point; 4]; 2]> {
    let (r1, r2) = (root.radius, ball.radius);
    let d = root.center.distance(ball.center);
    if r1 <= 0.0 || r2 <= 0.0 || d <= (r1 - r2).abs() || d > r1 + r2 * theme::NECK_REACH {
        return None;
    }
    // How far round each circle the overlap already reaches.
    let (u1, u2) = if d < r1 + r2 {
        (
            ((r1 * r1 + d * d - r2 * r2) / (2.0 * r1 * d))
                .clamp(-1.0, 1.0)
                .acos(),
            ((r2 * r2 + d * d - r1 * r1) / (2.0 * r2 * d))
                .clamp(-1.0, 1.0)
                .acos(),
        )
    } else {
        (0.0, 0.0)
    };
    let toward = Vector::new(ball.center.x - root.center.x, ball.center.y - root.center.y);
    let between = toward.y.atan2(toward.x);
    let widest = ((r1 - r2) / d).clamp(-1.0, 1.0).acos();
    let a1 = between + u1 + (widest - u1) * SPREAD;
    let a2 = between - u1 - (widest - u1) * SPREAD;
    let a3 = between + PI - u2 - (PI - u2 - widest) * SPREAD;
    let a4 = between - PI + u2 + (PI - u2 - widest) * SPREAD;
    let on = |circle: Circle, angle: f32| {
        circle.center + Vector::new(angle.cos(), angle.sin()) * circle.radius
    };
    let (p1, p2, p3, p4) = (on(root, a1), on(root, a2), on(ball, a3), on(ball, a4));
    // Shorter handles while the circles are close, so the waist doesn't bulge past them.
    let total = r1 + r2;
    let length = (SPREAD * HANDLE).min(p1.distance(p3) / total) * (2.0 * d / total).min(1.0);
    let handle = |from: Point, angle: f32, radius: f32| {
        from + Vector::new(angle.cos(), angle.sin()) * (radius * length)
    };
    let h1 = handle(p1, a1 - FRAC_PI_2, r1);
    let h2 = handle(p2, a2 + FRAC_PI_2, r1);
    let h3 = handle(p3, a3 + FRAC_PI_2, r2);
    let h4 = handle(p4, a4 - FRAC_PI_2, r2);
    Some([[p1, h1, h3, p3], [p4, h4, h2, p2]])
}

/// The bridge as a path to fill. Its ends are straight across each circle, inside it: the ball
/// is drawn over them, and the root is in the notch.
pub fn path(curves: [[Point; 4]; 2]) -> Path {
    let [[p1, h1, h3, p3], [p4, h4, h2, p2]] = curves;
    Path::new(|builder| {
        builder.move_to(p1);
        builder.bezier_curve_to(h1, h3, p3);
        builder.line_to(p4);
        builder.bezier_curve_to(h4, h2, p2);
        builder.close();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(y: f32, radius: f32) -> Circle {
        Circle {
            center: Point::new(100.0, y),
            radius,
        }
    }

    /// How wide the bridge is halfway down: between its two curves at t = ½.
    fn waist(curves: [[Point; 4]; 2]) -> f32 {
        let mid = |[a, b, c, d]: [Point; 4]| {
            Point::new(
                (a.x + 3.0 * b.x + 3.0 * c.x + d.x) / 8.0,
                (a.y + 3.0 * b.y + 3.0 * c.y + d.y) / 8.0,
            )
        };
        mid(curves[0]).distance(mid(curves[1]))
    }

    #[test]
    fn the_neck_is_symmetric_about_the_line_between_them() {
        let [[p1, h1, h3, p3], [p4, h4, h2, p2]] =
            bridge(circle(0.0, 10.0), circle(30.0, 13.0)).expect("close enough for a neck");
        for (a, b) in [(p1, p2), (h1, h2), (h3, h4), (p3, p4)] {
            assert!((a.y - b.y).abs() < 1e-3, "{a:?} {b:?}");
            assert!(((a.x - 100.0) + (b.x - 100.0)).abs() < 1e-3, "{a:?} {b:?}");
        }
    }

    #[test]
    fn the_neck_thins_as_they_part_then_pinches_off() {
        let root = circle(0.0, 10.0);
        let mut last = f32::INFINITY;
        for y in [22.0, 26.0, 30.0, 34.0, 38.0] {
            let width = waist(bridge(root, circle(y, 13.0)).unwrap());
            assert!(width < last, "thinner at {y}: {width} after {last}");
            last = width;
        }
        let reach = 10.0 + 13.0 * theme::NECK_REACH;
        assert!(bridge(root, circle(reach + 0.5, 13.0)).is_none());
    }

    #[test]
    fn nothing_while_one_holds_the_other_or_either_is_empty() {
        assert!(bridge(circle(0.0, 10.0), circle(2.0, 3.0)).is_none());
        assert!(bridge(circle(0.0, 10.0), circle(20.0, 0.0)).is_none());
    }

    #[test]
    fn the_neck_stays_between_the_circles() {
        let (root, ball) = (circle(0.0, 10.0), circle(34.0, 13.0));
        for curve in bridge(root, ball).unwrap() {
            for point in curve {
                assert!((87.0..=113.0).contains(&point.x), "{point:?}");
                assert!((-10.0..=47.0).contains(&point.y), "{point:?}");
            }
        }
    }
}
