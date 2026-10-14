//! The neck between the notch and the dormer's drop (D-86): black that flows between the bottom of
//! the notch (or the grown edge) and the drop, like something thick. It's the smooth union of the
//! two: everywhere the line's distance and the shape's are close, the union swells out past both,
//! so a drop just below the notch sits in a wide, smooth fillet, which narrows as it falls, thins
//! to a waist and pinches off, leaving a drip on the notch that draws back in.
//!
//! The union's field is symmetric about the drop's middle and only grows away from it along a row,
//! so each row of the neck is one span, found by bisection, then kept to the root's reach. A drop
//! that comes out of an island off to the side (D-88) leans: its spans center on the drop, not on
//! the root. The outline is the spans' ends, top to bottom down the right, and back up the left.

use iced::widget::canvas::Path;
use iced::{Point, Rectangle};

/// How far apart the rows are, in logical pixels: under a physical pixel at scale 2.
const ROW: f32 = 0.5;
/// How many halvings find a span's end: well under a hundredth of a pixel across `spread`.
const HALVINGS: u32 = 16;

/// Where the neck flows from, and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Root {
    /// The bottom of what it hangs from, from the surface's top.
    pub y: f32,
    /// The middle of where it comes out, across.
    pub x: f32,
    /// How far each way from the middle the neck may reach along the root: the fillet flattens
    /// into it out there. Short of the notch's lower corners.
    pub spread: f32,
    /// How far apart the root and the shape still flow together, in logical pixels: the wider,
    /// the wider and softer the fillet, and the further it stretches before it pinches off.
    pub blend: f32,
}

/// The neck's rows, top to bottom: each row's height, and where its span starts and ends across,
/// if it has one.
pub struct Neck {
    root: Root,
    rows: Vec<(f32, Option<(f32, f32)>)>,
}

impl Neck {
    /// The neck between `root` and `shape`, rounded `radius` at its corners, down to the shape's
    /// bottom or a blend's depth into it, whichever is higher. `None` while they're too far apart
    /// to touch.
    pub fn new(root: Root, shape: Rectangle, radius: f32) -> Option<Self> {
        let blend = root.blend;
        if blend <= 0.0 || shape.width <= 0.0 || shape.height <= 0.0 {
            return None;
        }
        if shape.y - root.y > 2.0 * blend {
            return None;
        }
        let bottom = (shape.y + blend).min(shape.y + shape.height);
        let center = shape.center();
        let half = (shape.width / 2.0, shape.height / 2.0);
        let radius = radius.clamp(0.0, half.0.min(half.1));
        let field = |x: f32, y: f32| {
            let line = y - root.y;
            let shape = rounded_box(x - center.x, y - center.y, half, radius);
            smooth_min(line, shape, blend)
        };
        // From half a row under the root: on it, every row reaches all the way along it.
        let top = root.y + ROW / 2.0;
        let count = ((bottom - top) / ROW).ceil().max(0.0) as usize + 1;
        // Out from the drop's middle, as far as the far end of the root's reach, then kept to it.
        let (from, to) = (root.x - root.spread, root.x + root.spread);
        let reach = root.spread + (center.x - root.x).abs();
        let rows = (0..count)
            .map(|row| {
                let y = (top + row as f32 * ROW).min(bottom);
                let span = span(|x| field(center.x + x, y), reach).and_then(|half| {
                    let (left, right) = ((center.x - half).max(from), (center.x + half).min(to));
                    (left < right).then_some((left, right))
                });
                (y, span)
            })
            .collect();
        Some(Self { root, rows })
    }

    /// Whether it still joins the root to the shape: no row along it is empty.
    pub fn joined(&self) -> bool {
        self.rows.iter().all(|(_, span)| span.is_some())
    }

    /// How far across it reaches, from its leftmost to its rightmost, if it has any span.
    pub fn across(&self) -> Option<(f32, f32)> {
        let spans = self.rows.iter().filter_map(|(_, span)| *span);
        spans.reduce(|(left, right), (l, r)| (left.min(l), right.max(r)))
    }

    /// Its outline, to fill: a piece for each run of rows with spans. The piece at the top reaches
    /// a pixel up into what it hangs from, as wide as its first row, so no seam shows between them.
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
                if start == 0 {
                    let (left, right) = rows[0].1.unwrap_or_default();
                    builder.move_to(Point::new(left, self.root.y - 1.0));
                    builder.line_to(Point::new(right, self.root.y - 1.0));
                    for (y, (_, right)) in spans.clone() {
                        builder.line_to(Point::new(right, y));
                    }
                } else {
                    for (index, (y, (_, right))) in spans.clone().enumerate() {
                        let at = Point::new(right, y);
                        if index == 0 {
                            builder.move_to(at);
                        } else {
                            builder.line_to(at);
                        }
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

/// How far from the middle, up to `spread`, a row's `field` stays inside (0 or less): it only
/// grows away from the middle. `None` if even the middle is outside.
fn span(field: impl Fn(f32) -> f32, spread: f32) -> Option<f32> {
    if field(0.0) > 0.0 {
        return None;
    }
    if field(spread) <= 0.0 {
        return Some(spread);
    }
    let (mut inside, mut outside) = (0.0, spread);
    for _ in 0..HALVINGS {
        let middle = (inside + outside) / 2.0;
        if field(middle) <= 0.0 {
            inside = middle;
        } else {
            outside = middle;
        }
    }
    Some((inside + outside) / 2.0)
}

/// The smooth union of two distances: their smaller one, pulled in where they're within `blend`
/// of each other (the polynomial smooth minimum).
fn smooth_min(a: f32, b: f32, blend: f32) -> f32 {
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
    use iced::Size;

    const ROOT: Root = Root {
        y: 0.0,
        x: 100.0,
        spread: 60.0,
        blend: 36.0,
    };

    /// A ball 26 across, its top `gap` below the root.
    fn ball(gap: f32) -> Rectangle {
        Rectangle::new(Point::new(87.0, gap), Size::new(26.0, 26.0))
    }

    fn neck(gap: f32) -> Neck {
        Neck::new(ROOT, ball(gap), 13.0).expect("close enough")
    }

    /// How wide it is at its narrowest, between the root and the ball's top.
    fn waist(neck: &Neck, gap: f32) -> f32 {
        neck.rows
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
    }

    #[test]
    fn it_pinches_off_leaving_a_drip_then_lets_go() {
        let pinched = (1..40)
            .map(|gap| gap as f32)
            .find(|&gap| !neck(gap).joined())
            .expect("pinches off");
        assert!(pinched > 5.0 && pinched < ROOT.blend, "at {pinched}");
        assert!(neck(pinched).path().is_some(), "a drip stays a moment");
        assert!(Neck::new(ROOT, ball(2.0 * ROOT.blend + 1.0), 13.0).is_none());
    }

    #[test]
    fn it_never_reaches_past_its_spread() {
        let neck = neck(0.0);
        assert!(
            neck.rows
                .iter()
                .all(|(_, span)| span.is_none_or(|(left, right)| {
                    left >= ROOT.x - ROOT.spread && right <= ROOT.x + ROOT.spread
                }))
        );
    }

    #[test]
    fn a_drop_off_to_the_side_leans_its_way_and_stays_on_the_root() {
        // The same ball, 20 to the right of the root's middle.
        let moved = ball(3.0) + iced::Vector::new(20.0, 0.0);
        let neck = Neck::new(ROOT, moved, 13.0).expect("close enough");
        assert!(neck.joined());
        // Where it meets the ball, it's around the ball's middle, not the root's.
        let (left, right) = neck.rows.last().unwrap().1.unwrap();
        assert!(((left + right) / 2.0 - 120.0).abs() < 0.5, "{left}..{right}");
        // Up at the root, it never reaches past the root's spread.
        let (_, right) = neck.rows[0].1.unwrap();
        assert!(right <= ROOT.x + ROOT.spread);
        // Far off to the side, it can't reach the root at all.
        let far = ball(3.0) + iced::Vector::new(200.0, 0.0);
        assert!(Neck::new(ROOT, far, 13.0).is_none_or(|neck| !neck.joined()));
    }

    #[test]
    fn the_field_is_the_shapes_far_from_the_root() {
        assert_eq!(rounded_box(0.0, 0.0, (13.0, 13.0), 13.0), -13.0);
        assert!((rounded_box(20.0, 0.0, (13.0, 13.0), 13.0) - 7.0).abs() < 1e-4);
        assert_eq!(smooth_min(50.0, 2.0, 20.0), 2.0);
        assert!(smooth_min(5.0, 5.0, 20.0) < 5.0);
    }
}
