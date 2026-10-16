//! Islands that join (D-122): two shapes close to each other become one, the smooth union of
//! their distances, as drops of something thick do. Where they're far apart each is exactly its
//! own outline; where they come within a blend of each other, a fillet swells between them, a
//! neck that pinches off as they part and flows in as they meet.
//!
//! Only on when asked: a dialog leaving the island it came out of, or going back into it, tells
//! that island where it is ([`Join`]), and that island draws them both as one shape, under
//! everything on either (D-122). Nothing else pays for it.
//!
//! The field is only worked out where the fillet can be: where both shapes are within reach of
//! each other. Outside that box, the union is the two outlines as they are. Inside, the outline
//! of the field is found on a grid of pixels (marching squares), looking closely only where it
//! can pass: the field changes no faster than the distance does, so a block whose middle is
//! further from the outline than its corners are from its middle doesn't have it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use iced::widget::canvas::{Path, path};
use iced::{Point, Rectangle, Size};

use crate::pour::smooth_min;
use crate::squircle::to_segment;

/// Where a shape that joins another is, from the one it joins, while they're close: the shared
/// handle, empty while they're apart.
pub type Join = Rc<RefCell<Option<Joining>>>;

/// A shape joining another, as it is this frame.
#[derive(Debug, Clone)]
pub struct Joining {
    /// Its outline, flattened, in surface coordinates.
    pub outline: Vec<Vec<Point>>,
    /// The box round it.
    pub bounds: Rectangle,
    /// How far apart they flow together, in logical pixels: less as they meet, so two shapes on
    /// top of each other are just the one.
    pub blend: f32,
    /// Whether the other shape drew it this frame. Until it has, the joining one draws itself.
    pub drawn: Cell<bool>,
}

impl Joining {
    pub fn new(outline: Vec<Vec<Point>>, blend: f32) -> Option<Self> {
        let bounds = bounds(outline.iter().flatten())?;
        Some(Self {
            outline,
            bounds,
            blend,
            drawn: Cell::new(false),
        })
    }
}

/// How far past both shapes' boxes the fillet can reach, as a share of the blend: the smooth
/// minimum only pulls in where the distances are within the blend of each other, and only so far
/// as to bring the nearer one a quarter of the blend down.
pub const REACH: f32 = 1.25;

/// How far a joining shape's flattened outline may stray from its curve, in logical pixels.
pub const TOLERANCE: f32 = 0.05;

/// How far into the shapes the fillet reaches, in logical pixels: it overlaps them, so they're
/// filled as one with no gap between.
const OVERLAP: f32 = 1.0;

/// The fillet's field from the distances to `a` and `b`: negative in it. The smooth union, less
/// what's inside either shape further than [`OVERLAP`].
fn value(a: f32, b: f32, blend: f32) -> f32 {
    smooth_min(a, b, blend).max(-a.min(b) - OVERLAP)
}

/// How far apart the grid's corners are, in logical pixels: the outline's points are as far apart,
/// which across the fillet's curve, at least a few pixels round, strays a tenth of a pixel at most.
const CELL: f32 = 2.0;

/// How many cells the coarsest blocks are across.
const BLOCK: usize = 8;

/// How many pieces of an outline go in a box together, for skipping the far ones.
const RUN: usize = 16;

/// A closed outline, flattened, ready to say how far a point is from it.
pub struct Outline {
    edges: Vec<(Point, Point)>,
    runs: Vec<Run>,
    bounds: Rectangle,
    /// Which way it winds: the sign of its area (positive clockwise on screen).
    winding: f32,
    /// The run the last point asked about was nearest: the next one is likely near it too.
    hint: Cell<usize>,
}

struct Run {
    from: usize,
    to: usize,
    low: Point,
    high: Point,
}

impl Outline {
    pub fn new(polygons: &[Vec<Point>]) -> Option<Self> {
        let edges: Vec<(Point, Point)> = polygons
            .iter()
            .filter(|polygon| polygon.len() >= 3)
            .flat_map(|polygon| polygon.iter().zip(polygon.iter().cycle().skip(1)))
            .map(|(a, b)| (*a, *b))
            .collect();
        let bounds = bounds(edges.iter().map(|(a, _)| a))?;
        let runs = (0..edges.len())
            .step_by(RUN)
            .map(|from| {
                let to = (from + RUN).min(edges.len());
                let ends = edges[from..to].iter().flat_map(|(a, b)| [a, b]);
                let (mut low, mut high) = (
                    Point::new(f32::MAX, f32::MAX),
                    Point::new(f32::MIN, f32::MIN),
                );
                for at in ends {
                    low = Point::new(low.x.min(at.x), low.y.min(at.y));
                    high = Point::new(high.x.max(at.x), high.y.max(at.y));
                }
                Run {
                    from,
                    to,
                    low,
                    high,
                }
            })
            .collect();
        let winding = polygons
            .iter()
            .map(|polygon| area(polygon))
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(0.0)
            .signum();
        Some(Self {
            edges,
            runs,
            bounds,
            winding,
            hint: Cell::new(0),
        })
    }

    /// How far `at` is from the outline: negative inside it.
    pub fn distance(&self, at: Point) -> f32 {
        // Squared until the end.
        let mut nearest = f32::INFINITY;
        let mut best = self.hint.get().min(self.runs.len().saturating_sub(1));
        let look = |index: usize, nearest: &mut f32| {
            let run = &self.runs[index];
            let dx = (run.low.x - at.x).max(at.x - run.high.x).max(0.0);
            let dy = (run.low.y - at.y).max(at.y - run.high.y).max(0.0);
            if dx * dx + dy * dy >= *nearest {
                return false;
            }
            let mut closer = false;
            for (a, b) in &self.edges[run.from..run.to] {
                let d = to_segment(at, *a, *b);
                if d < *nearest {
                    *nearest = d;
                    closer = true;
                }
            }
            closer
        };
        look(best, &mut nearest);
        for index in 0..self.runs.len() {
            if index != best && look(index, &mut nearest) {
                best = index;
            }
        }
        self.hint.set(best);

        // Inside if a ray to the right crosses the outline an odd number of times.
        let mut inside = false;
        for run in &self.runs {
            if at.y < run.low.y || at.y > run.high.y || at.x > run.high.x {
                continue;
            }
            for (a, b) in &self.edges[run.from..run.to] {
                if (a.y > at.y) != (b.y > at.y) {
                    let x = a.x + (at.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if x > at.x {
                        inside = !inside;
                    }
                }
            }
        }
        let distance = nearest.sqrt();
        if inside { -distance } else { distance }
    }
}

/// The two shapes as one, to fill by the nonzero rule: `a`'s own path, `b`'s outline, and the
/// fillet between them, all wound the same way, so what's drawn is their union, edges and all,
/// with no seam. Without a blend, or apart, it's just the two.
pub fn path(a: &Path, b: &[Vec<Point>], blend: f32) -> Path {
    let polygons = crate::squircle::polygons(a);
    let outlines = Outline::new(&polygons).zip(Outline::new(b));
    let (fillet, winding) = match &outlines {
        Some((first, second)) => (fillet(first, second, blend), first.winding),
        None => (Vec::new(), 0.0),
    };
    Path::new(|builder| {
        append(builder, a);
        for polygon in b {
            // The same way round as `a`, or they'd cancel where they overlap.
            let reversed = winding != 0.0 && area(polygon).signum() == -winding;
            if reversed {
                lines(builder, polygon.iter().rev().copied());
            } else {
                lines(builder, polygon.iter().copied());
            }
        }
        for polygon in fillet {
            lines(builder, polygon.into_iter());
        }
    })
}

fn lines(builder: &mut path::Builder, mut points: impl Iterator<Item = Point>) {
    let Some(first) = points.next() else {
        return;
    };
    builder.move_to(first);
    for point in points {
        builder.line_to(point);
    }
    builder.close();
}

/// Adds `path` to what `builder` is building, as it is.
fn append(builder: &mut path::Builder, path: &Path) {
    use path::lyon_path::Event;

    let point = |p: path::lyon_path::math::Point| Point::new(p.x, p.y);
    for event in path.raw().iter() {
        match event {
            Event::Begin { at } => builder.move_to(point(at)),
            Event::Line { to, .. } => builder.line_to(point(to)),
            Event::Quadratic { ctrl, to, .. } => builder.quadratic_curve_to(point(ctrl), point(to)),
            Event::Cubic {
                ctrl1, ctrl2, to, ..
            } => builder.bezier_curve_to(point(ctrl1), point(ctrl2), point(to)),
            Event::End { close, .. } => {
                if close {
                    builder.close();
                }
            }
        }
    }
}

/// The fillet of the smooth union of `a` and `b`: what of it is outside both, reaching a little into
/// them, so no hairline shows where it meets them. Each loop closed, outer ones wound as `a` is,
/// holes the other way. Empty when they're too far apart to flow together, or without a blend.
pub fn fillet(a: &Outline, b: &Outline, blend: f32) -> Vec<Vec<Point>> {
    if blend <= 0.0 {
        return Vec::new();
    }
    let reach = REACH * blend + 2.0;
    let Some(reachable) = a.bounds.expand(reach).intersection(&b.bounds.expand(reach)) else {
        return Vec::new();
    };
    let field = |at: Point| value(a.distance(at), b.distance(at), blend);
    let mut grid = Grid::new(reachable);
    grid.visit(&|at| (a.distance(at), b.distance(at)), blend);
    let mut loops = grid.trace(&field);
    // The biggest loop is an outer one: if it's the other way round from `a`, they all are.
    let winding = loops
        .iter()
        .map(|polygon| area(polygon))
        .max_by(|x, y| x.abs().total_cmp(&y.abs()))
        .unwrap_or(0.0)
        .signum();
    if a.winding != 0.0 && winding == -a.winding {
        for polygon in &mut loops {
            polygon.reverse();
        }
    }
    loops
}

/// The field's values on every pixel corner of a box, where they matter: elsewhere only which
/// side of the outline they're on.
struct Grid {
    origin: Point,
    /// How many cells across and down: one more corner each way.
    across: usize,
    down: usize,
    values: Vec<f32>,
    /// Whether a corner's value is the field's own, rather than only its sign.
    exact: Vec<bool>,
}

impl Grid {
    fn new(area: Rectangle) -> Self {
        let origin = Point::new(area.x.floor(), area.y.floor());
        let across = ((area.x + area.width - origin.x) / CELL).ceil().max(1.0) as usize;
        let down = ((area.y + area.height - origin.y) / CELL).ceil().max(1.0) as usize;
        let count = (across + 1) * (down + 1);
        // Unknown, but for the box's edge, which counts as outside: the fillet never gets that
        // far, and what of `a` or `b` does is drawn by them.
        let mut grid = Self {
            origin,
            across,
            down,
            values: vec![f32::NAN; count],
            exact: vec![false; count],
        };
        for j in 0..=down {
            for i in 0..=across {
                if grid.edge(i, j) {
                    let index = grid.index(i, j);
                    grid.values[index] = 1.0;
                }
            }
        }
        grid
    }

    fn at(&self, i: usize, j: usize) -> Point {
        Point::new(
            self.origin.x + i as f32 * CELL,
            self.origin.y + j as f32 * CELL,
        )
    }

    fn index(&self, i: usize, j: usize) -> usize {
        j * (self.across + 1) + i
    }

    /// Settles every corner, from the distances to both shapes at a point.
    fn visit(&mut self, distances: &impl Fn(Point) -> (f32, f32), blend: f32) {
        for j in (0..self.down).step_by(BLOCK) {
            for i in (0..self.across).step_by(BLOCK) {
                self.block(distances, blend, i, j, BLOCK);
            }
        }
    }

    /// Settles the corners of the block of cells `size` across from `(i, j)`: by its middle, if
    /// the fillet can't be in it or its outline can't pass through it, or else block by smaller
    /// block.
    fn block(
        &mut self,
        distances: &impl Fn(Point) -> (f32, f32),
        blend: f32,
        i: usize,
        j: usize,
        size: usize,
    ) {
        let (to_i, to_j) = ((i + size).min(self.across), (j + size).min(self.down));
        if i >= to_i || j >= to_j {
            return;
        }
        if size == 1 {
            for (ci, cj) in [(i, j), (to_i, j), (i, to_j), (to_i, to_j)] {
                let index = self.index(ci, cj);
                if !self.exact[index] && !self.edge(ci, cj) {
                    let (a, b) = distances(self.at(ci, cj));
                    self.values[index] = value(a, b, blend);
                    self.exact[index] = true;
                }
            }
            return;
        }
        let half = size as f32 * CELL / 2.0;
        let middle = self.at(i, j) + iced::Vector::new(half, half);
        let (a, b) = distances(middle);
        let reach = half * std::f32::consts::SQRT_2;
        let near = a.min(b);
        // No fillet anywhere in it: deep in either shape, too far from both, or where one is
        // much nearer than the other, which is the nearer one's own edge, drawn by it. (Their
        // difference changes up to twice as fast as either.)
        let none = near < -OVERLAP - reach
            || near > blend / 4.0 + reach
            || (a - b).abs() > blend + 2.0 * reach;
        let value = if none {
            reach + 1.0
        } else {
            value(a, b, blend)
        };
        if value.abs() > reach {
            // All one side: its corners are at least this far from the outline.
            let bound = value.signum() * (value.abs() - reach);
            for cj in j..=to_j {
                for ci in i..=to_i {
                    let index = self.index(ci, cj);
                    if self.values[index].is_nan() {
                        self.values[index] = bound;
                    }
                }
            }
            return;
        }
        let half = size / 2;
        for (di, dj) in [(0, 0), (half, 0), (0, half), (half, half)] {
            self.block(distances, blend, i + di, j + dj, half);
        }
    }

    fn edge(&self, i: usize, j: usize) -> bool {
        i == 0 || j == 0 || i == self.across || j == self.down
    }

    /// Where the outline crosses between corners `p` and `q`, one inside and one outside: between
    /// their values if both are the field's own, by halving otherwise.
    fn crossing(
        &self,
        field: &impl Fn(Point) -> f32,
        p: (usize, usize),
        q: (usize, usize),
    ) -> Point {
        let (ip, iq) = (self.index(p.0, p.1), self.index(q.0, q.1));
        let (vp, vq) = (self.values[ip], self.values[iq]);
        let (pp, pq) = (self.at(p.0, p.1), self.at(q.0, q.1));
        if self.exact[ip] && self.exact[iq] {
            let t = (vp / (vp - vq)).clamp(0.0, 1.0);
            return Point::new(pp.x + (pq.x - pp.x) * t, pp.y + (pq.y - pp.y) * t);
        }
        let (mut inside, mut outside) = if vp <= 0.0 { (pp, pq) } else { (pq, pp) };
        for _ in 0..6 {
            let middle = Point::new((inside.x + outside.x) / 2.0, (inside.y + outside.y) / 2.0);
            if field(middle) <= 0.0 {
                inside = middle;
            } else {
                outside = middle;
            }
        }
        Point::new((inside.x + outside.x) / 2.0, (inside.y + outside.y) / 2.0)
    }

    /// The outline through the cells, as closed loops. In each cell, walking its corners
    /// clockwise, the outline goes from where the walk leaves the inside to where it comes back
    /// in: next to it if the cell's middle is inside, or else the one before. Neighboring cells
    /// walk their shared side the other way, so the pieces join end to start.
    fn trace(&self, field: &impl Fn(Point) -> f32) -> Vec<Vec<Point>> {
        // Each side of a cell by the corner it starts from and which way it goes.
        let side = |i: usize, j: usize, down: bool| -> usize {
            (j * (self.across + 1) + i) * 2 + usize::from(down)
        };
        let mut next: HashMap<usize, (usize, Point)> = HashMap::new();
        for j in 0..self.down {
            for i in 0..self.across {
                let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let inside = corners.map(|(ci, cj)| self.values[self.index(ci, cj)] <= 0.0);
                if inside.iter().all(|&x| x) || inside.iter().all(|&x| !x) {
                    continue;
                }
                // The cell's sides, clockwise: top, right, bottom, left.
                let sides = [
                    side(i, j, false),
                    side(i + 1, j, true),
                    side(i, j + 1, false),
                    side(i, j, true),
                ];
                let leaves: Vec<usize> = (0..4)
                    .filter(|&k| inside[k] && !inside[(k + 1) % 4])
                    .collect();
                let enters: Vec<usize> = (0..4)
                    .filter(|&k| !inside[k] && inside[(k + 1) % 4])
                    .collect();
                let middle = if leaves.len() > 1 {
                    field(self.at(i, j) + iced::Vector::new(CELL / 2.0, CELL / 2.0)) <= 0.0
                } else {
                    true
                };
                for &k in &leaves {
                    let to = if middle {
                        (1..4).map(|d| (k + d) % 4).find(|e| enters.contains(e))
                    } else {
                        (1..4).map(|d| (k + 4 - d) % 4).find(|e| enters.contains(e))
                    };
                    let Some(to) = to else { continue };
                    let at = self.crossing(field, corners[k], corners[(k + 1) % 4]);
                    next.insert(sides[k], (sides[to], at));
                }
            }
        }
        let mut loops = Vec::new();
        while let Some(&start) = next.keys().next() {
            let mut polygon = Vec::new();
            let mut at = start;
            while let Some((to, point)) = next.remove(&at) {
                polygon.push(point);
                at = to;
                if at == start {
                    break;
                }
            }
            if polygon.len() >= 3 {
                loops.push(polygon);
            }
        }
        loops
    }
}

/// The signed area of a polygon: positive clockwise on screen, with y down.
fn area(polygon: &[Point]) -> f32 {
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f32>()
        / 2.0
}

fn bounds<'a>(points: impl Iterator<Item = &'a Point>) -> Option<Rectangle> {
    let (mut low, mut high) = (
        Point::new(f32::MAX, f32::MAX),
        Point::new(f32::MIN, f32::MIN),
    );
    let mut any = false;
    for at in points {
        any = true;
        low = Point::new(low.x.min(at.x), low.y.min(at.y));
        high = Point::new(high.x.max(at.x), high.y.max(at.y));
    }
    any.then(|| Rectangle::new(low, Size::new(high.x - low.x, high.y - low.y)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squircle::{CornerShape, Polygons, rectangle, squircle};

    fn square(x: f32, y: f32, size: f32) -> Vec<Vec<Point>> {
        vec![vec![
            Point::new(x, y),
            Point::new(x + size, y),
            Point::new(x + size, y + size),
            Point::new(x, y + size),
        ]]
    }

    fn outline(polygons: &[Vec<Point>]) -> Outline {
        Outline::new(polygons).expect("an outline")
    }

    /// Whether `at` is inside any of `loops`, by the nonzero rule.
    fn covers(loops: &[Vec<Point>], at: Point) -> bool {
        let mut winding = 0;
        for polygon in loops {
            for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
                if (a.y > at.y) != (b.y > at.y) {
                    let x = a.x + (at.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if x > at.x {
                        winding += if b.y > a.y { 1 } else { -1 };
                    }
                }
            }
        }
        winding != 0
    }

    #[test]
    fn the_distance_is_signed_and_exact() {
        let square = outline(&square(0.0, 0.0, 10.0));
        assert_eq!(square.distance(Point::new(5.0, 5.0)), -5.0);
        assert_eq!(square.distance(Point::new(5.0, 13.0)), 3.0);
        assert!((square.distance(Point::new(13.0, 14.0)) - 5.0).abs() < 1e-5);
        assert_eq!(square.distance(Point::new(2.0, 5.0)), -2.0);
    }

    #[test]
    fn a_squircle_is_as_far_as_its_curve() {
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(80.0, 32.0));
        let mut polygons = Polygons::within(TOLERANCE);
        rectangle(&mut polygons, bounds, [CornerShape::new(14.0, 0.6); 4]);
        let shape = outline(&polygons.0);
        assert!((shape.distance(Point::new(40.0, -3.0)) - 3.0).abs() < 0.01);
        assert!((shape.distance(Point::new(40.0, 16.0)) + 16.0).abs() < 0.01);
        // Round the corner, further than the box's corner.
        assert!(shape.distance(Point::new(0.0, 0.0)) > 2.0);
    }

    #[test]
    fn apart_or_without_a_blend_they_are_just_the_two() {
        let a = outline(&square(0.0, 0.0, 20.0));
        let far = outline(&square(100.0, 0.0, 20.0));
        assert!(fillet(&a, &far, 24.0).is_empty());
        let near = outline(&square(30.0, 0.0, 20.0));
        assert!(fillet(&a, &near, 0.0).is_empty());
    }

    /// Whether `at` is in the union of `a`, `b` and their fillet.
    fn joined(a: &[Vec<Point>], b: &[Vec<Point>], at: Point) -> bool {
        let loops = fillet(&outline(a), &outline(b), 24.0);
        covers(a, at) || covers(b, at) || covers(&loops, at)
    }

    #[test]
    fn close_they_flow_together_and_part_as_they_go() {
        let a = square(0.0, 0.0, 20.0);
        let between = |gap: f32| {
            let b = square(20.0 + gap, 0.0, 20.0);
            joined(&a, &b, Point::new(20.0 + gap / 2.0, 10.0))
        };
        assert!(between(4.0), "a neck between them");
        assert!(!between(20.0), "parted");
    }

    #[test]
    fn the_fillet_is_closed_and_wound_as_the_shape_is() {
        let a = square(0.0, 0.0, 20.0);
        let b = square(26.0, 6.0, 20.0);
        let loops = fillet(&outline(&a), &outline(&b), 24.0);
        assert!(!loops.is_empty());
        let biggest = loops
            .iter()
            .map(|polygon| area(polygon))
            .max_by(|x, y| x.abs().total_cmp(&y.abs()))
            .unwrap();
        assert_eq!(biggest.signum(), area(&a[0]).signum());
        // Between them, and a swell past both where they meet, but nothing far from either.
        assert!(covers(&loops, Point::new(23.0, 12.0)));
        assert!(joined(&a, &b, Point::new(23.0, 5.0)));
        assert!(!joined(&a, &b, Point::new(23.0, -10.0)));
        assert!(
            !covers(&loops, Point::new(10.0, 10.0)),
            "not deep in either"
        );
        assert!(
            !covers(&loops, Point::new(10.0, -3.0)),
            "nor along a side far from the other"
        );
    }

    #[test]
    fn one_on_top_of_the_other_unblended_is_the_one() {
        let a = squircle(
            Rectangle::new(Point::new(0.0, 0.0), Size::new(80.0, 32.0)),
            14.0,
            0.6,
        );
        let joined = path(&a, &square(10.0, 10.0, 10.0), 0.0);
        let polygons = crate::squircle::polygons(&joined);
        assert_eq!(polygons.len(), 2, "the two, nothing more");
    }

    /// How long a frame of a dialog landing on an island takes, in release:
    /// `cargo test --release -p eave join::tests::timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing() {
        let mut home = Polygons::within(TOLERANCE);
        rectangle(
            &mut home,
            Rectangle::new(Point::new(300.0, 6.0), Size::new(80.0, 32.0)),
            [CornerShape::new(14.0, 0.6); 4],
        );
        let mut dialog = Polygons::within(TOLERANCE);
        rectangle(
            &mut dialog,
            Rectangle::new(Point::new(280.0, 20.0), Size::new(140.0, 90.0)),
            [CornerShape::new(14.0, 0.6); 4],
        );
        let (a, b) = (outline(&home.0), outline(&dialog.0));
        let start = std::time::Instant::now();
        let runs = 200;
        let mut pieces = 0;
        for _ in 0..runs {
            pieces += fillet(&a, &b, 24.0).iter().map(Vec::len).sum::<usize>();
        }
        println!(
            "{:?} a frame, {} points",
            start.elapsed() / runs,
            pieces / runs as usize
        );
    }
}
