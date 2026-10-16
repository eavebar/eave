//! Pouring: a shape that comes out of something flows out of it like something thick. What it
//! comes out of is a line, the bottom of the notch or the grown edge, for the dormer's drop
//! (D-86). Every other morph flows as a body ([`crate::body`], D-118).
//!
//! The pour is the smooth union of the two: everywhere their distances are close, the union swells
//! out past both, so a shape just out of where it comes from sits in a wide, smooth fillet, which
//! narrows as it goes, thins to a waist and pinches off, leaving a drip that draws back in. It's
//! drawn under both, so only the fillet shows.
//!
//! Along a row, the union's field only grows away from a point inside it, so each row of the pour
//! is one span, found by bisection each way. The outline is the spans' ends, top to bottom down
//! the right, and back up the left.

use iced::widget::canvas::Path;
use iced::{Point, Rectangle, Size};

/// How far apart the rows are, in logical pixels: under a physical pixel at scale 2.
const ROW: f32 = 0.5;
/// How many halvings find a span's end: well under a hundredth of a pixel across its reach.
const HALVINGS: u32 = 16;

/// What a shape pours out of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum From {
    /// The bottom of what it hangs from, `y` from the surface's top. The pour flows from about `x`
    /// along it, and reaches no further than `spread` each way: the fillet flattens into it out
    /// there, short of the notch's lower corners.
    Line { y: f32, x: f32, spread: f32 },
}

/// The pour's rows, top to bottom: each row's height, and where its span starts and ends across,
/// if it has one.
pub struct Pour {
    /// Where a line it flows from is: its top reaches a pixel up into it.
    seam: Option<f32>,
    rows: Vec<(f32, Option<(f32, f32)>)>,
}

impl Pour {
    /// The pour between `from` and `shape`, rounded `radius` at its corners, flowing together
    /// where they're within `blend` of each other, in logical pixels: the wider, the wider and
    /// softer the fillet, and the further it stretches before it pinches off. It reaches down to
    /// the shape's bottom or a blend's depth into it, whichever is higher. `None` while they're
    /// too far apart to touch, and with no blend.
    pub fn new(from: From, blend: f32, shape: Rectangle, radius: f32) -> Option<Self> {
        if blend <= 0.0 || shape.width <= 0.0 || shape.height <= 0.0 {
            return None;
        }
        let center = shape.center();
        let half = (shape.width / 2.0, shape.height / 2.0);
        let radius = radius.clamp(0.0, half.0.min(half.1));
        let body = move |x: f32, y: f32| rounded_box(x - center.x, y - center.y, half, radius);
        let bottom = (shape.y + blend).min(shape.y + shape.height);
        match from {
            From::Line { y: root, x, spread } => {
                if shape.y - root > 2.0 * blend {
                    return None;
                }
                let field = move |at: f32, y: f32| smooth_min(y - root, body(at, y), blend);
                // From half a row under the root: on it, every row reaches all the way along it.
                let rows = each_row(root + ROW / 2.0, bottom, |y| {
                    span(|at| field(at, y), x, x - spread, x + spread)
                });
                Some(Self {
                    seam: Some(root),
                    rows,
                })
            }
        }
    }

    /// Whether it still joins what it comes out of to the shape: no row along it is empty.
    pub fn joined(&self) -> bool {
        self.rows.iter().all(|(_, span)| span.is_some())
    }

    /// How far right and down it reaches, from the surface's origin, and a pixel more.
    pub fn reach(&self) -> Size {
        let (right, bottom) = self
            .rows
            .iter()
            .filter_map(|&(y, span)| span.map(|(_, right)| (right, y)))
            .fold((0.0_f32, 0.0_f32), |(r, b), (right, y)| {
                (r.max(right), b.max(y))
            });
        Size::new(right + 1.0, bottom + 1.0)
    }

    /// Its outline, to fill: a piece for each run of rows with spans. Flowing from a line, the
    /// piece at the top reaches a pixel up into it, as wide as its first row, so no seam shows
    /// between them.
    pub fn path(&self) -> Option<Path> {
        if self.rows.iter().all(|(_, span)| span.is_none()) {
            return None;
        }
        Some(Path::new(|builder| {
            let mut start = 0;
            while start < self.rows.len() {
                let run = self.rows[start..]
                    .iter()
                    .take_while(|(_, span)| span.is_some())
                    .count();
                if run == 0 {
                    start += 1;
                    continue;
                }
                let rows = &self.rows[start..start + run];
                let spans = rows.iter().map(|&(y, span)| (y, span.unwrap_or_default()));
                let seam = self.seam.filter(|_| start == 0);
                if let Some(seam) = seam {
                    let (left, right) = rows[0].1.unwrap_or_default();
                    builder.move_to(Point::new(left, seam - 1.0));
                    builder.line_to(Point::new(right, seam - 1.0));
                }
                for (index, (y, (_, right))) in spans.clone().enumerate() {
                    let at = Point::new(right, y);
                    if index == 0 && seam.is_none() {
                        builder.move_to(at);
                    } else {
                        builder.line_to(at);
                    }
                }
                for (y, (left, _)) in spans.rev() {
                    builder.line_to(Point::new(left, y));
                }
                builder.close();
                start += run;
            }
        }))
    }
}

/// The rows from `top` down to `bottom`, `ROW` apart, the last on `bottom`, each with its `span`.
fn each_row(
    top: f32,
    bottom: f32,
    span: impl Fn(f32) -> Option<(f32, f32)>,
) -> Vec<(f32, Option<(f32, f32)>)> {
    let count = ((bottom - top) / ROW).ceil().max(0.0) as usize + 1;
    (0..count)
        .map(|row| {
            let y = (top + row as f32 * ROW).min(bottom);
            (y, span(y))
        })
        .collect()
}

/// Where a row's `field` stays inside (0 or less) each way from `inside`, no further than `low`
/// and `high`: it only grows away from there. `None` if even there is outside.
fn span(field: impl Fn(f32) -> f32, inside: f32, low: f32, high: f32) -> Option<(f32, f32)> {
    if field(inside) > 0.0 {
        return None;
    }
    let end = |far: f32| {
        if field(far) <= 0.0 {
            return far;
        }
        let (mut inside, mut outside) = (inside, far);
        for _ in 0..HALVINGS {
            let middle = (inside + outside) / 2.0;
            if field(middle) <= 0.0 {
                inside = middle;
            } else {
                outside = middle;
            }
        }
        (inside + outside) / 2.0
    };
    Some((end(low), end(high)))
}

/// The smooth union of two distances: their smaller one, pulled in where they're within `blend`
/// of each other (the polynomial smooth minimum).
pub(crate) fn smooth_min(a: f32, b: f32, blend: f32) -> f32 {
    let h = (blend - (a - b).abs()).max(0.0) / blend;
    a.min(b) - h * h * blend / 4.0
}

/// How far `(x, y)`, from its middle, is outside a box `half` across and down from its middle,
/// rounded `radius` at its corners. Negative inside.
fn rounded_box(x: f32, y: f32, half: (f32, f32), radius: f32) -> f32 {
    let qx = x.abs() - (half.0 - radius);
    let qy = y.abs() - (half.1 - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - radius
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: From = From::Line {
        y: 0.0,
        x: 100.0,
        spread: 60.0,
    };
    const BLEND: f32 = 36.0;

    /// A ball 26 across, its top `gap` below the root.
    fn ball(gap: f32) -> Rectangle {
        Rectangle::new(Point::new(87.0, gap), Size::new(26.0, 26.0))
    }

    fn neck(gap: f32) -> Pour {
        Pour::new(ROOT, BLEND, ball(gap), 13.0).expect("close enough")
    }

    /// How wide it is at its narrowest, between the root and the ball's top.
    fn waist(pour: &Pour, gap: f32) -> f32 {
        pour.rows
            .iter()
            .filter(|(y, _)| *y <= gap)
            .map(|(_, span)| span.map_or(0.0, |(left, right)| right - left))
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn a_close_drop_sits_in_a_wide_fillet_that_narrows_as_it_falls() {
        let mut last = f32::INFINITY;
        for gap in [1.0, 3.0, 5.0, 7.0] {
            let neck = neck(gap);
            assert!(neck.joined(), "joined at {gap}");
            let width = waist(&neck, gap);
            assert!(width < last, "narrower at {gap}: {width} after {last}");
            last = width;
        }
        // Right under the root it flattens into it: a wide bell, three times the ball's width.
        let (left, right) = neck(1.0).rows[0].1.unwrap();
        assert!(right - left > 6.0 * 13.0);
        assert!(
            ((right - 100.0) - (100.0 - left)).abs() < 0.01,
            "even about the drop"
        );
    }

    #[test]
    fn it_pinches_off_leaving_a_drip_then_lets_go() {
        let pinched = (1..40)
            .map(|gap| gap as f32)
            .find(|&gap| !neck(gap).joined())
            .expect("pinches off");
        assert!(pinched > 5.0 && pinched < BLEND, "at {pinched}");
        assert!(neck(pinched).path().is_some(), "a drip stays a moment");
        assert!(Pour::new(ROOT, BLEND, ball(2.0 * BLEND + 1.0), 13.0).is_none());
    }

    #[test]
    fn it_never_reaches_past_its_spread() {
        assert!(
            neck(0.0)
                .rows
                .iter()
                .all(|(_, span)| span.is_none_or(|(left, right)| left >= 40.0 && right <= 160.0))
        );
    }

    #[test]
    fn the_field_is_the_shapes_far_from_the_root() {
        assert_eq!(rounded_box(0.0, 0.0, (13.0, 13.0), 13.0), -13.0);
        assert!((rounded_box(20.0, 0.0, (13.0, 13.0), 13.0) - 7.0).abs() < 1e-4);
        assert_eq!(smooth_min(50.0, 2.0, 20.0), 2.0);
        assert!(smooth_min(5.0, 5.0, 20.0) < 5.0);
    }
}
