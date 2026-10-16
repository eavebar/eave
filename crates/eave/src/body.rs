//! The body (D-118): an island's outline as a soft body while it morphs. A ring of points all
//! the way round the outline, each with its own place and speed, so a shape in motion is any
//! closed curve, not a box between two boxes.
//!
//! Each point is pulled toward its place on the outline the shape is going to (its anchor), a
//! squircle sampled the same way whatever its size: so many points round each corner and so many
//! along each side, so corners go to corners and nothing swims round the outline. On top of that:
//! - **Lead and lag.** Where a shape is going out, its points pull harder and get there first;
//!   where it's coming in, they pull softer and hang behind. How much depends on how far each
//!   point goes along the outline's outward normal there, so the middle of a side going out runs
//!   on ahead (a popover pours out of its island, rounded, with no tongue of its own) and the
//!   middle of one coming in hangs on, like a drip drawn back up.
//! - **Bounce.** The pull is underdamped (`BODY_DAMPING`), so the outline goes past where it's
//!   going and wobbles back.
//! - **Tension.** Each point is pulled toward its neighbors, as far as they're away from where
//!   they belong: what one point does spreads smoothly to the next, the anchors' own curves (the
//!   corners) aren't flattened.
//! - **Area.** The outline keeps the area its frame has, pushing out or in along its normals, so
//!   a side running ahead draws the others in, and one that splats bulges them out. The top,
//!   which hangs from the bar, takes no part.
//! - **The top.** Nothing goes up past it: a point that gets there stops, as against the bar.
//!
//! At rest it's the anchors exactly, and the island draws its plain squircle instead.

use iced::{Point, Rectangle, Vector};

use crate::spring::SUBSTEP;
use crate::squircle::{self, Corner, CornerShape, DOWN, LEFT, Polygons, RIGHT, Sink, UP};
use crate::theme;

/// Points round each corner, ends included, and between the corners along the top and the
/// bottom, and down each side.
const CORNER: usize = 8;
const ACROSS: usize = 24;
const SIDE: usize = 16;
const RING: usize = 4 * CORNER + 2 * (ACROSS + SIDE);

/// How close to its anchor, and how slow, a point counts as there. Looser than a spring's rest:
/// the spline already strays this far from the squircle it gives way to, so the last of a wobble
/// isn't seen, and an island that waits for it (a popover closing) doesn't hang on for it.
const REST_DISTANCE: f32 = 0.25;
const REST_SPEED: f32 = 4.0;

/// How hard a point pulls toward its anchor that swings once in `response` seconds, undamped.
fn stiffness(response: f32) -> f32 {
    let omega = std::f32::consts::TAU / response;
    omega * omega
}

#[derive(Debug, Clone)]
pub struct Body {
    at: [Point; RING],
    velocity: [Vector; RING],
    anchor: [Point; RING],
    /// How hard each point pulls toward its anchor: harder where it leads.
    pull: [f32; RING],
    /// How damped each point is: going out, it bounces; coming in, it doesn't go through.
    damping: [f32; RING],
    /// Where it's going and with which corners, once it's been placed.
    goal: Option<(Rectangle, [CornerShape; 4])>,
    /// The anchors' area over the goal's box: what the frame's box is worth in outline.
    fill: f32,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            at: [Point::ORIGIN; RING],
            velocity: [Vector::ZERO; RING],
            anchor: [Point::ORIGIN; RING],
            pull: [stiffness(theme::BODY_RESPONSE); RING],
            damping: [theme::BODY_DAMPING; RING],
            goal: None,
            fill: 1.0,
        }
    }
}

impl Body {
    /// At rest at `rect`, rounded by `corners` (top left, top right, bottom right, bottom left).
    pub fn snap(&mut self, rect: Rectangle, corners: [CornerShape; 4]) {
        self.place(rect, corners);
        self.at = self.anchor;
        self.velocity = [Vector::ZERO; RING];
        self.pull = [stiffness(theme::BODY_RESPONSE); RING];
        self.damping = [theme::BODY_DAMPING; RING];
    }

    /// Heads for `rect`, rounded by `corners`. Where it's going changing sets each point's lead
    /// or lag again, from how far it has to go from where it is now; only the corners changing
    /// (a drop rounding off) just moves the anchors. Nothing it has placed jumps.
    pub fn aim(&mut self, rect: Rectangle, corners: [CornerShape; 4]) {
        match self.goal {
            None => self.snap(rect, corners),
            Some(goal) if goal == (rect, corners) => {}
            Some((goal, _)) => {
                self.place(rect, corners);
                if goal != rect {
                    self.lead();
                }
            }
        }
    }

    /// Samples the anchors off the squircle at `rect`.
    fn place(&mut self, rect: Rectangle, corners: [CornerShape; 4]) {
        self.goal = Some((rect, corners));
        self.anchor = outline(rect, corners);
        let area = rect.width * rect.height;
        self.fill = if area > 1.0 {
            (area_of(&self.anchor) / area).clamp(0.5, 1.0)
        } else {
            1.0
        };
    }

    /// Each point's pull, from how far it goes along the outward normal at its anchor, as a share
    /// of the furthest any point goes: going out all the way, it swings in `BODY_LEAD` of `MORPH`'s
    /// time, coming in all the way, in `BODY_LAG` of it, and in between, between them.
    fn lead(&mut self) {
        let travel: [Vector; RING] = std::array::from_fn(|i| self.anchor[i] - self.at[i]);
        let furthest = travel.iter().map(|t| length(*t)).fold(0.0f32, f32::max);
        let normals = normals(&self.anchor);
        for i in 0..RING {
            let out = if furthest > 1.0 {
                dot(travel[i], normals[i]) / furthest
            } else {
                0.0
            };
            // Sharpened, so it's the middle of a side that leads, more than the corners round it.
            let out = out.signum() * out.abs().powf(theme::BODY_FOCUS);
            let tempo = if out > 0.0 {
                theme::BODY_LEAD.powf(out)
            } else {
                theme::BODY_LAG.powf(-out)
            };
            self.pull[i] = stiffness(theme::BODY_RESPONSE * tempo);
            // Coming in, it's damped up to `BODY_LAND`, so a shape drawn back in lands without
            // going through its top.
            self.damping[i] = if out < 0.0 {
                let firm = theme::BODY_LAND - theme::BODY_DAMPING;
                theme::BODY_DAMPING + firm * (-out).min(1.0).powf(0.25)
            } else {
                theme::BODY_DAMPING
            };
        }
    }

    /// Advances by `seconds`, keeping the area of `frame`, the box the shape is laid out in.
    pub fn advance(&mut self, seconds: f32, frame: Rectangle) {
        if self.goal.is_none() {
            return;
        }
        let base = stiffness(theme::BODY_RESPONSE);
        let tension = theme::BODY_TENSION * base;
        let pressure = theme::BODY_AREA * base;
        let wanted = frame.width * frame.height * self.fill;
        let ceiling = self.goal.map_or(frame.y, |(goal, _)| goal.y.min(frame.y));
        let mut left = seconds;
        while left > 0.0 {
            let dt = left.min(SUBSTEP);
            left -= dt;
            let off: [Vector; RING] = std::array::from_fn(|i| self.at[i] - self.anchor[i]);
            let normals = normals(&self.at);
            let perimeter = perimeter(&self.at).max(1.0);
            let swell = (wanted - area_of(&self.at)) / perimeter;
            for i in 0..RING {
                let (before, after) = ((i + RING - 1) % RING, (i + 1) % RING);
                let spread = off[before] + off[after] - off[i] * 2.0;
                let k = self.pull[i];
                let damping = 2.0 * self.damping[i] * k.sqrt();
                let force = off[i] * -k - self.velocity[i] * damping
                    + spread * tension
                    + normals[i] * (pressure * swell * hangs(normals[i]));
                self.velocity[i] += force * dt;
            }
            for i in 0..RING {
                self.at[i] += self.velocity[i] * dt;
                // Nothing goes up past the top, which hangs from the bar: it stops there.
                if self.at[i].y < ceiling {
                    self.at[i].y = ceiling;
                    self.velocity[i].y = self.velocity[i].y.max(0.0);
                }
            }
        }
    }

    /// Whether every point is where it belongs, and still.
    pub fn at_rest(&self) -> bool {
        self.at
            .iter()
            .zip(&self.anchor)
            .zip(&self.velocity)
            .all(|((at, anchor), velocity)| {
                at.distance(*anchor) < REST_DISTANCE && length(*velocity) < REST_SPEED
            })
    }

    /// Puts every point where it belongs, still.
    pub fn settle(&mut self) {
        self.at = self.anchor;
        self.velocity = [Vector::ZERO; RING];
    }

    /// The box round the outline as it is.
    pub fn bounds(&self) -> Rectangle {
        let (mut low, mut high) = (self.at[0], self.at[0]);
        for at in &self.at {
            low = Point::new(low.x.min(at.x), low.y.min(at.y));
            high = Point::new(high.x.max(at.x), high.y.max(at.y));
        }
        Rectangle::new(low, iced::Size::new(high.x - low.x, high.y - low.y))
    }

    /// The outline as it is, moved by `by`, into `sink`: a smooth curve through every point
    /// (Catmull-Rom), closed. Points on top of each other count once, so a side with no length
    /// leaves no kink.
    pub fn outline(&self, sink: &mut impl Sink, by: Vector) {
        let mut points: Vec<Point> = Vec::with_capacity(RING);
        for at in self.at {
            let at = at + by;
            if points.last().is_none_or(|last| last.distance(at) > 0.01) {
                points.push(at);
            }
        }
        while points.len() > 1 && points[0].distance(points[points.len() - 1]) <= 0.01 {
            points.pop();
        }
        let count = points.len();
        if count < 3 {
            return;
        }
        let point = |i: usize| points[i % count];
        sink.move_to(point(0));
        for i in 0..count {
            let (p0, p1, p2, p3) = (point(i + count - 1), point(i), point(i + 1), point(i + 2));
            let a = p1 + (p2 - p0) * (1.0 / 6.0);
            let b = p2 - (p3 - p1) * (1.0 / 6.0);
            sink.cubic_to(a, b, p2);
        }
        sink.close();
    }
}

/// The squircle at `rect` as the ring's points, clockwise from where the top right corner starts:
/// each corner's points from its start to its end, evenly along it, then the side after it's.
fn outline(rect: Rectangle, corners: [CornerShape; 4]) -> [Point; RING] {
    let (left, top) = (rect.x, rect.y);
    let (right, bottom) = (left + rect.width, top + rect.height);
    let budget = rect.width.min(rect.height) / 2.0;
    let [top_left, top_right, bottom_right, bottom_left] = corners;
    // As `squircle::rectangle` goes: each corner, where it is, and its edges in and out, then
    // how many points the side after it has.
    let around = [
        (top_right, Point::new(right, top), RIGHT, DOWN, SIDE),
        (bottom_right, Point::new(right, bottom), DOWN, LEFT, ACROSS),
        (bottom_left, Point::new(left, bottom), LEFT, UP, SIDE),
        (top_left, Point::new(left, top), UP, RIGHT, ACROSS),
    ];
    let mut points = [Point::ORIGIN; RING];
    let mut next = 0;
    let mut ends = [(Point::ORIGIN, Point::ORIGIN); 4];
    for (index, (shape, at, incoming, outgoing, _)) in around.iter().enumerate() {
        let corner = Corner::new(shape.radius, shape.smoothing, budget);
        let mut curve = Polygons::default();
        squircle::corner_into(&mut curve, &corner, *at, *incoming, *outgoing, true);
        let curve = curve.0.into_iter().next().unwrap_or_default();
        let start = next;
        let sampled = evenly(&curve, CORNER);
        points[start..start + CORNER].copy_from_slice(&sampled);
        ends[index] = (sampled[0], sampled[CORNER - 1]);
        next += CORNER + around[index].4;
    }
    // Each side, evenly between the end of the corner before it and the start of the one after.
    let mut start = 0;
    for index in 0..4 {
        let count = around[index].4;
        let from = ends[index].1;
        let to = ends[(index + 1) % 4].0;
        for j in 0..count {
            let t = (j + 1) as f32 / (count + 1) as f32;
            points[start + CORNER + j] = from + (to - from) * t;
        }
        start += CORNER + count;
    }
    points
}

/// `count` points evenly along the polyline `line`, its ends included.
fn evenly(line: &[Point], count: usize) -> Vec<Point> {
    let Some(first) = line.first() else {
        return vec![Point::ORIGIN; count];
    };
    let lengths: Vec<f32> = line.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f32 = lengths.iter().sum();
    if total < 1e-4 {
        return vec![*first; count];
    }
    let mut out = Vec::with_capacity(count);
    let (mut segment, mut walked) = (0, 0.0);
    for k in 0..count {
        let wanted = total * k as f32 / (count - 1) as f32;
        while segment < lengths.len() - 1 && walked + lengths[segment] < wanted {
            walked += lengths[segment];
            segment += 1;
        }
        let t = if lengths[segment] > 0.0 {
            ((wanted - walked) / lengths[segment]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (a, b) = (line[segment], line[segment + 1]);
        out.push(a + (b - a) * t);
    }
    out
}

/// Each point's outward normal, from its neighbors: the ring runs clockwise on a screen, whose
/// y runs down. Zero where they're on top of each other.
fn normals(points: &[Point; RING]) -> [Vector; RING] {
    std::array::from_fn(|i| {
        let along = points[(i + 1) % RING] - points[(i + RING - 1) % RING];
        let length = length(along);
        if length < 1e-4 {
            Vector::ZERO
        } else {
            Vector::new(along.y / length, -along.x / length)
        }
    })
}

/// How much of the area's push a point with the outward `normal` takes: none on the top, which
/// hangs from the bar, and all of it from the sides down.
fn hangs(normal: Vector) -> f32 {
    (1.0 + normal.y).clamp(0.0, 1.0)
}

fn area_of(points: &[Point; RING]) -> f32 {
    let mut twice = 0.0;
    for i in 0..RING {
        let (a, b) = (points[i], points[(i + 1) % RING]);
        twice += a.x * b.y - b.x * a.y;
    }
    twice / 2.0
}

fn perimeter(points: &[Point; RING]) -> f32 {
    (0..RING)
        .map(|i| points[i].distance(points[(i + 1) % RING]))
        .sum()
}

fn length(v: Vector) -> f32 {
    (v.x * v.x + v.y * v.y).sqrt()
}

fn dot(a: Vector, b: Vector) -> f32 {
    a.x * b.x + a.y * b.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    const FRAME: f32 = 1.0 / 120.0;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(width, height))
    }

    fn round(radius: f32) -> [CornerShape; 4] {
        [CornerShape::new(radius, 0.6); 4]
    }

    /// The island: a pill. The panel it opens into, from its top.
    const ISLAND: Rectangle = Rectangle {
        x: 80.0,
        y: 0.0,
        width: 200.0,
        height: 32.0,
    };
    const PANEL: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 360.0,
        height: 400.0,
    };

    /// Which point is the middle of the bottom, and which the bottom corners' middles.
    fn bottom_middle() -> usize {
        2 * CORNER + SIDE + ACROSS / 2
    }
    fn bottom_corners() -> [usize; 2] {
        [
            CORNER + SIDE + CORNER / 2,
            2 * CORNER + SIDE + ACROSS + CORNER / 2,
        ]
    }

    #[test]
    fn at_rest_it_draws_its_squircle() {
        for (at, radius) in [
            (ISLAND, 16.0),
            (PANEL, 20.0),
            (rect(10.0, 10.0, 90.0, 40.0), 12.0),
        ] {
            let mut body = Body::default();
            body.snap(at, round(radius));
            let mut drawn = Polygons::default();
            body.outline(&mut drawn, Vector::ZERO);
            let mut exact = Polygons::default();
            squircle::rectangle(&mut exact, at, round(radius));
            let off = drawn.0[0]
                .iter()
                .map(|point| {
                    let one = Polygons::new(vec![vec![*point]]);
                    squircle::clearance(&exact, &one)
                })
                .fold(0.0f32, f32::max);
            assert!(off < 0.25, "{at:?} strays {off} px from its squircle");
            assert!(body.at_rest());
        }
    }

    #[test]
    fn opening_pours_out_and_bounces() {
        let mut body = Body::default();
        body.snap(ISLAND, round(16.0));
        body.aim(PANEL, round(20.0));
        let goal = PANEL.y + PANEL.height;
        let (mut lead, mut deepest) = (0.0f32, 0.0f32);
        for _ in 0..240 {
            body.advance(FRAME, PANEL);
            let middle = body.at[bottom_middle()].y;
            let corners = bottom_corners().map(|i| body.at[i].y);
            lead = lead.max(middle - corners[0].max(corners[1]));
            deepest = deepest.max(middle);
        }
        assert!(
            lead > 20.0,
            "the middle runs on ahead of the corners: {lead}"
        );
        assert!(
            deepest > goal + 5.0,
            "it goes past where it's going: {deepest}"
        );
        for _ in 0..600 {
            body.advance(FRAME, PANEL);
        }
        assert!(body.at_rest(), "and settles");
    }

    #[test]
    fn closing_hangs_on_in_the_middle() {
        let mut body = Body::default();
        body.snap(PANEL, round(20.0));
        body.aim(ISLAND, round(16.0));
        let mut hang = 0.0f32;
        for _ in 0..120 {
            body.advance(FRAME, ISLAND);
            let middle = body.at[bottom_middle()].y;
            let corners = bottom_corners().map(|i| body.at[i].y);
            hang = hang.max(middle - corners[0].max(corners[1]));
        }
        assert!(hang > 4.0, "the middle hangs on behind: {hang}");
    }

    #[test]
    fn turning_back_keeps_going_smoothly() {
        let mut body = Body::default();
        body.snap(ISLAND, round(16.0));
        body.aim(PANEL, round(20.0));
        for _ in 0..10 {
            body.advance(FRAME, PANEL);
        }
        let (at, velocity) = (body.at, body.velocity);
        body.aim(ISLAND, round(16.0));
        assert_eq!(body.at, at, "nothing jumps");
        assert_eq!(body.velocity, velocity, "nothing stops");
        for _ in 0..900 {
            body.advance(FRAME, ISLAND);
        }
        assert!(body.at_rest());
    }

    #[test]
    fn it_keeps_clear_of_nothing_it_shouldnt() {
        // Every point stays near the boxes it goes between, plus its bounce.
        let mut body = Body::default();
        body.snap(ISLAND, round(16.0));
        body.aim(PANEL, round(20.0));
        for _ in 0..600 {
            body.advance(FRAME, PANEL);
            let bounds = body.bounds();
            assert!(
                bounds.x > -60.0 && bounds.x + bounds.width < 420.0,
                "{bounds:?}"
            );
            assert!(
                bounds.y > -60.0 && bounds.y + bounds.height < 480.0,
                "{bounds:?}"
            );
        }
    }
}
