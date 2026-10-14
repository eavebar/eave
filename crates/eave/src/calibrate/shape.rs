//! The screen as `eave calibrate` models it, and the pattern that shows how well the model fits:
//! a one-pixel red line along the inside of what the model says is shown, and blue over what it
//! says is hidden (D-71). The model is right when every red pixel shows and no blue one does.
//!
//! Everything is in physical pixels, with pixel edges on whole numbers: a pixel is in when its
//! center is, with no anti-aliasing, so it's either red, blue or black and nothing between. The
//! outline is flattened finely and filled here, exactly: a renderer's fill without anti-aliasing
//! flattens curves too coarsely to trust a single pixel's color.

use iced::{Point, Rectangle, Size};

use crate::config::{NotchShape, Screen};
use crate::squircle::{self, Corner, CornerShape, DOWN, LEFT, Polygons, RIGHT, Sink, UP};

/// The model: how the screen's edges are rounded and hidden, and its notch.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Shape {
    pub screen: Screen,
    /// Always with a height.
    pub notch: Option<NotchShape>,
}

/// What the model says is shown, as a path into `sink`: the screen inside its insets, with its
/// corners, and around its notch. A notch hanging from the top edge is part of the outline; one
/// below it is a hole of its own, so the path is filled even-odd.
pub fn outline(sink: &mut impl Sink, shape: &Shape, size: Size) {
    let Screen {
        corners,
        smoothing,
        inset,
        ..
    } = shape.screen;
    let [top, right, bottom, left] = inset;
    let (l, t, r, b) = (left, top, size.width - right, size.height - bottom);
    if r <= l || b <= t {
        return;
    }
    let budget = (r - l).min(b - t) / 2.0;
    let corner = |radius| Corner::new(radius, smoothing, budget);
    let [top_left, top_right, bottom_right, bottom_left] = corners;

    squircle::corner_into(sink, &corner(top_left), Point::new(l, t), UP, RIGHT, true);
    let notch = shape.notch.filter(|notch| notch.width > 0.0);
    if let Some(notch) = notch.filter(|notch| notch.top <= 0.0) {
        let bottom = notch.height.unwrap_or_default();
        let side = bottom - t;
        let from = size.width / 2.0 - notch.width / 2.0;
        let to = size.width / 2.0 + notch.width / 2.0;
        // A notch no lower than the inset is under the bezel already.
        if side > 0.0 && from > l && to < r {
            // The shoulders round the screen where it meets the notch, and the notch's lower
            // corners round it back: they share the notch's sides.
            let shoulder = Corner::new(
                notch.shoulder,
                notch.smoothing,
                (side / 2.0).min((from - l) / 2.0),
            );
            let lower = Corner::new(
                notch.radius,
                notch.smoothing,
                (notch.width / 2.0).min(side - shoulder.extent()),
            );
            squircle::corner_into(sink, &shoulder, Point::new(from, t), RIGHT, DOWN, false);
            squircle::corner_into(sink, &lower, Point::new(from, bottom), DOWN, RIGHT, false);
            squircle::corner_into(sink, &lower, Point::new(to, bottom), RIGHT, UP, false);
            squircle::corner_into(sink, &shoulder, Point::new(to, t), UP, RIGHT, false);
        }
    }
    squircle::corner_into(
        sink,
        &corner(top_right),
        Point::new(r, t),
        RIGHT,
        DOWN,
        false,
    );
    squircle::corner_into(
        sink,
        &corner(bottom_right),
        Point::new(r, b),
        DOWN,
        LEFT,
        false,
    );
    squircle::corner_into(
        sink,
        &corner(bottom_left),
        Point::new(l, b),
        LEFT,
        UP,
        false,
    );
    sink.close();

    if let Some(notch) = notch.filter(|notch| notch.top > 0.0) {
        let bounds = Rectangle::new(
            Point::new(size.width / 2.0 - notch.width / 2.0, notch.top),
            Size::new(notch.width, notch.height.unwrap_or_default()),
        );
        squircle::rectangle(
            sink,
            bounds,
            [CornerShape::new(notch.radius, notch.smoothing); 4],
        );
    }
}

/// Inside what's shown, along its edge: every one of these should show.
pub const RED: [u8; 4] = [255, 40, 40, 255];
/// Outside it: none of these should.
pub const BLUE: [u8; 4] = [40, 140, 255, 255];
/// Inside it, away from the edge.
pub const BLACK: [u8; 4] = [0, 0, 0, 255];

/// Which pixels of a `width` × `height` screen the model says are shown, row by row: the ones
/// whose center is inside the outline, even-odd.
pub fn shown(shape: &Shape, width: u32, height: u32) -> Vec<bool> {
    let mut polygons = Polygons::default();
    outline(&mut polygons, shape, Size::new(width as f32, height as f32));
    // Every edge, closing each polygon.
    let edges: Vec<(Point, Point)> = polygons
        .0
        .iter()
        .filter(|polygon| polygon.len() > 2)
        .flat_map(|polygon| {
            polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .map(|(from, to)| (*from, *to))
        })
        .filter(|(from, to)| from.y != to.y)
        .collect();

    let (w, h) = (width as usize, height as usize);
    let mut shown = vec![false; w * h];
    let mut crossings = Vec::new();
    for y in 0..h {
        let center = y as f32 + 0.5;
        crossings.clear();
        // Half-open, so a vertex on the line is crossed once.
        crossings.extend(
            edges
                .iter()
                .filter(|(from, to)| (from.y <= center) != (to.y <= center))
                .map(|(from, to)| from.x + (center - from.y) * (to.x - from.x) / (to.y - from.y)),
        );
        crossings.sort_unstable_by(f32::total_cmp);
        let row = &mut shown[y * w..(y + 1) * w];
        for [from, to] in crossings.as_chunks::<2>().0 {
            // The pixels whose centers lie from one crossing up to the next.
            let first = (from - 0.5).ceil().clamp(0.0, w as f32) as usize;
            let end = (to - 0.5).ceil().clamp(0.0, w as f32) as usize;
            row[first..end.max(first)].fill(true);
        }
    }
    shown
}

/// The pattern, RGBA: red where a shown pixel touches a hidden one or the screen's edge, black
/// on the rest that's shown, blue where it's hidden.
pub fn pattern(shape: &Shape, width: u32, height: u32) -> Vec<u8> {
    let shown = shown(shape, width, height);
    let (w, h) = (width as usize, height as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let at = y * w + x;
            let color = if !shown[at] {
                BLUE
            } else if x == 0
                || y == 0
                || x == w - 1
                || y == h - 1
                || !shown[at - 1]
                || !shown[at + 1]
                || !shown[at - w]
                || !shown[at + w]
            {
                RED
            } else {
                BLACK
            };
            rgba.extend_from_slice(&color);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * width + x) * 4) as usize;
        rgba[at..at + 4].try_into().unwrap()
    }

    fn notch(width: f32, height: f32) -> NotchShape {
        NotchShape {
            width,
            height: Some(height),
            ..NotchShape::default()
        }
    }

    #[test]
    fn a_plain_screen_is_outlined_all_round() {
        let (w, h) = (30, 20);
        let rgba = pattern(&Shape::default(), w, h);
        let reds = rgba.chunks(4).filter(|pixel| *pixel == RED).count();
        assert_eq!(reds, (2 * w + 2 * h - 4) as usize);
        assert!(!rgba.chunks(4).any(|pixel| pixel == BLUE));
    }

    #[test]
    fn an_inset_hides_its_rows_and_the_line_moves_in() {
        let mut shape = Shape::default();
        shape.screen.inset = [2.0, 0.0, 0.0, 1.0];
        let rgba = pattern(&shape, 30, 20);
        assert_eq!(color(&rgba, 30, 10, 1), BLUE);
        assert_eq!(color(&rgba, 30, 10, 2), RED);
        assert_eq!(color(&rgba, 30, 0, 10), BLUE);
        assert_eq!(color(&rgba, 30, 1, 10), RED);
        assert_eq!(color(&rgba, 30, 10, 10), BLACK);
    }

    #[test]
    fn a_round_corner_hides_what_falls_outside_its_circle() {
        let mut shape = Shape::default();
        shape.screen.corners = [10.0; 4];
        let (w, h) = (40, 40);
        let shown = shown(&shape, w, h);
        let is_shown = |x: u32, y: u32| shown[(y * w + x) as usize];
        for y in 0..10 {
            for x in 0..10 {
                // Centered 10 in from both edges, through the pixel's center.
                let distance =
                    ((x as f32 + 0.5 - 10.0).powi(2) + (y as f32 + 0.5 - 10.0).powi(2)).sqrt();
                if (distance - 10.0).abs() > 0.1 {
                    assert_eq!(is_shown(x, y), distance < 10.0, "({x}, {y}): {distance}");
                }
            }
        }
        // All four corners alike.
        for y in 0..h {
            for x in 0..w {
                assert_eq!(is_shown(x, y), is_shown(w - 1 - x, y), "({x}, {y})");
                assert_eq!(is_shown(x, y), is_shown(x, h - 1 - y), "({x}, {y})");
            }
        }
    }

    #[test]
    fn every_red_pixel_is_shown_and_touches_what_isnt() {
        let mut shape = Shape::default();
        shape.screen.corners = [12.0, 6.0, 0.0, 3.0];
        shape.screen.smoothing = 0.6;
        shape.notch = Some(NotchShape {
            radius: 5.0,
            shoulder: 3.0,
            smoothing: 0.6,
            ..notch(24.0, 12.0)
        });
        let (w, h) = (80, 40);
        let rgba = pattern(&shape, w, h);
        let shown = shown(&shape, w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                if color(&rgba, w, x, y) == RED {
                    let at = (y * w + x) as usize;
                    let w = w as usize;
                    assert!(shown[at]);
                    assert!(
                        !shown[at - 1] || !shown[at + 1] || !shown[at - w] || !shown[at + w],
                        "({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_notch_hangs_from_the_top_in_the_middle() {
        let shape = Shape {
            notch: Some(NotchShape {
                radius: 4.0,
                shoulder: 2.0,
                ..notch(20.0, 10.0)
            }),
            ..Shape::default()
        };
        let (w, h) = (100, 40);
        let rgba = pattern(&shape, w, h);
        assert_eq!(color(&rgba, w, 50, 0), BLUE);
        assert_eq!(color(&rgba, w, 50, 9), BLUE);
        assert_eq!(color(&rgba, w, 50, 10), RED);
        // From 40 to 60, and wider at the top where its shoulders round it outwards.
        assert_eq!(color(&rgba, w, 40, 5), BLUE);
        assert_eq!(color(&rgba, w, 39, 5), RED);
        assert_eq!(color(&rgba, w, 39, 0), BLUE);
        let shown = shown(&shape, w, h);
        for y in 0..h {
            for x in 0..w {
                let mirrored = w - 1 - x;
                assert_eq!(
                    shown[(y * w + x) as usize],
                    shown[(y * w + mirrored) as usize],
                    "({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_notch_below_the_top_is_a_hole_with_the_top_row_shown() {
        let shape = Shape {
            notch: Some(NotchShape {
                top: 4.0,
                ..notch(20.0, 10.0)
            }),
            ..Shape::default()
        };
        let (w, h) = (100, 40);
        let rgba = pattern(&shape, w, h);
        assert!((0..w).all(|x| color(&rgba, w, x, 0) == RED));
        assert_eq!(color(&rgba, w, 50, 3), RED);
        assert_eq!(color(&rgba, w, 50, 4), BLUE);
        assert_eq!(color(&rgba, w, 50, 13), BLUE);
        assert_eq!(color(&rgba, w, 50, 14), RED);
    }
}
