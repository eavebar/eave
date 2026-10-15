//! An island's outline as a jelly: a ring of springs all the way round its edge, each pulled back
//! to where it belongs and to its neighbors. A nudge pokes the ring (kicks some of it outward or
//! in), and the ring smooths the poke out by itself: it spreads along the edge and round the
//! corners, then settles flat. Nothing that pokes it has to care what curve it makes.
//!
//! Each spring is how far out (or, negative, in) its piece of the edge is, in logical pixels. The
//! ring has a fixed number of them, spread over the shape's size, so it keeps working while the
//! shape morphs.
//!
//! On top of the ring, the top and bottom can each hold a shelf out (D-105): a flat stretch of the
//! edge pushed out under an item, easing back into the edge at both ends. The island moves the
//! shelf; the jelly only draws it, with the ring, inside the same room.

use iced::{Point, Rectangle};

use crate::spring::{REACT, REST_DISTANCE, REST_SPEED, SUBSTEP};
use crate::theme;

/// Springs across the top and the bottom, and down each side.
const ACROSS: usize = 24;
const DOWN: usize = 6;
const RING: usize = 2 * (ACROSS + DOWN);

/// How fast to poke a piece of the ring so the edge goes out about `peak` there, at its furthest
/// (`REACT_SIZE` included). A poke is shared with the neighbors as it goes, so the ring goes out
/// less far than one spring alone: by about `SHARED`, for pokes as wide as a nudge's.
pub fn poke(peak: f32) -> f32 {
    const SHARED: f32 = 1.3;
    REACT.kick(peak * theme::REACT_SIZE) * SHARED
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    /// Where the side's springs start in the ring, and how many it has. The ring runs clockwise
    /// from the top left, so the bottom runs right to left and the left side bottom to top.
    fn span(self) -> (usize, usize) {
        match self {
            Side::Top => (0, ACROSS),
            Side::Right => (ACROSS, DOWN),
            Side::Bottom => (ACROSS + DOWN, ACROSS),
            Side::Left => (2 * ACROSS + DOWN, DOWN),
        }
    }

    fn backwards(self) -> bool {
        matches!(self, Side::Bottom | Side::Left)
    }
}

impl Side {
    fn index(self) -> usize {
        match self {
            Side::Top => 0,
            Side::Right => 1,
            Side::Bottom => 2,
            Side::Left => 3,
        }
    }
}

/// A stretch of the top or bottom held out flat, `depth` out from `from` to `to` (in logical
/// pixels from the shape's left), easing back into the edge over `NUDGE_SHOULDER` at each end.
/// The ease is a smootherstep, flat at both of its ends: the shelf leaves the edge and meets its
/// flat top without a crease.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Shelf {
    pub depth: f32,
    pub from: f32,
    pub to: f32,
}

impl Shelf {
    /// How far out it holds the edge at `x`, from the shape's left.
    pub fn at(&self, x: f32) -> f32 {
        if self.depth == 0.0 {
            return 0.0;
        }
        let outside = (self.from - x).max(x - self.to).max(0.0);
        let t = (outside / theme::NUDGE_SHOULDER).min(1.0);
        self.depth * (1.0 - t * t * t * (t * (6.0 * t - 15.0) + 10.0))
    }
}

#[derive(Debug, Clone)]
pub struct Jelly {
    value: [f32; RING],
    velocity: [f32; RING],
    /// The shelves on the top and the bottom, as the island last set them.
    pub shelves: [Shelf; 2],
    /// How far each side may go out (top, right, bottom, left): the room the island's frame has
    /// around it, less what a wall takes away.
    pub room: [f32; 4],
}

impl Default for Jelly {
    fn default() -> Self {
        Self {
            value: [0.0; RING],
            velocity: [0.0; RING],
            shelves: [Shelf::default(); 2],
            room: [theme::NUDGE_BUMP_ROOM; 4],
        }
    }
}

impl Jelly {
    /// Sets one side moving: each of its springs at least `speed` at how far along the side it is
    /// (0 to 1, left to right or top to bottom), outward in pixels per second. Kicks in a row
    /// don't pile up: each brings a spring up to its speed rather than adding to it, and less of
    /// it the further out the spring already is, so a stream of them levels off inside the room
    /// instead of running into its end.
    pub fn kick(&mut self, side: Side, speed: impl Fn(f32) -> f32) {
        let (start, count) = side.span();
        for k in 0..count {
            let along = (k as f32 + 0.5) / count as f32;
            let along = if side.backwards() { 1.0 - along } else { along };
            let speed = speed(along);
            let i = start + k;
            let gone = (self.value[i] * speed.signum() / theme::NUDGE_BUMP_ROOM).clamp(0.0, 1.0);
            let speed = speed * (1.0 - gone);
            self.velocity[i] = if speed > 0.0 {
                self.velocity[i].max(speed)
            } else {
                self.velocity[i].min(speed)
            };
        }
    }

    pub fn advance(&mut self, seconds: f32) {
        // Each piece rides REACT, like everything else an event kicks.
        let stiffness = REACT.stiffness;
        let damping = 2.0 * REACT.damping * stiffness.sqrt();
        let coupling = theme::JELLY_SPREAD * stiffness;
        let mut left = seconds;
        while left > 0.0 {
            let dt = left.min(SUBSTEP);
            for i in 0..RING {
                let (prev, next) = (
                    self.value[(i + RING - 1) % RING],
                    self.value[(i + 1) % RING],
                );
                let value = self.value[i];
                let force = -stiffness * value - damping * self.velocity[i]
                    + coupling * (prev + next - 2.0 * value);
                self.velocity[i] += force * dt;
            }
            for (value, velocity) in self.value.iter_mut().zip(&self.velocity) {
                *value += velocity * dt;
            }
            left -= dt;
        }
    }

    pub fn at_rest(&self) -> bool {
        self.value.iter().all(|value| value.abs() < REST_DISTANCE)
            && self.velocity.iter().all(|speed| speed.abs() < REST_SPEED)
    }

    /// Brings the ring to rest, flat. The shelves are the island's, and stay as they are.
    pub fn settle(&mut self) {
        self.value = [0.0; RING];
        self.velocity = [0.0; RING];
    }

    /// Whether the outline is where it belongs, so the shape needs no bending.
    pub fn is_flat(&self) -> bool {
        self.value.iter().all(|value| *value == 0.0)
            && self.shelves.iter().all(|shelf| shelf.depth == 0.0)
    }

    /// How far out one side is at `along` (0 to 1), smoothly between its springs.
    fn side(&self, side: Side, along: f32) -> f32 {
        let (start, count) = side.span();
        let at = |i: isize| {
            let i = i.clamp(0, count as isize - 1) as usize;
            let i = if side.backwards() { count - 1 - i } else { i };
            self.value[start + i]
        };
        let t = along.clamp(0.0, 1.0) * count as f32 - 0.5;
        let i = t.floor();
        let t = t - i;
        let i = i as isize;
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        // Catmull-Rom: through every spring, with no creases between them.
        0.5 * (2.0 * p1
            + (p2 - p0) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
    }

    /// How far out one side is at `along`, its shelf included, kept inside the room it has
    /// (`room`), and from caving in past its middle. `width` is the shape's, to place the shelf.
    fn out(&self, side: Side, along: f32, width: f32, reach: f32) -> f32 {
        let shelf = match side {
            Side::Top => self.shelves[0].at(along * width),
            Side::Bottom => self.shelves[1].at(along * width),
            Side::Left | Side::Right => 0.0,
        };
        let room = self.room[side.index()].max(0.0);
        (self.side(side, along) + shelf).clamp((-reach / 2.0).min(room), room)
    }

    /// Where the point `at` of the outline at rest, in a shape at `bounds`, is now. Each side
    /// moves the points near it, fully on the side and less the further in they are, down to
    /// nothing halfway across the shape's short way. So the corners go with both their sides.
    pub fn displace(&self, bounds: Rectangle, at: Point) -> Point {
        let reach = (bounds.width.min(bounds.height) / 2.0).max(1.0);
        let near = |distance: f32| {
            let near = (1.0 - distance / reach).clamp(0.0, 1.0);
            // Eased, so the bend has no crease where it starts.
            near * near * (3.0 - 2.0 * near)
        };
        let across = (at.x - bounds.x) / bounds.width;
        let down = (at.y - bounds.y) / bounds.height;
        let (right, bottom) = (bounds.x + bounds.width, bounds.y + bounds.height);
        let width = bounds.width;
        let dx = self.out(Side::Right, down, width, reach) * near(right - at.x)
            - self.out(Side::Left, down, width, reach) * near(at.x - bounds.x);
        let dy = self.out(Side::Bottom, across, width, reach) * near(bottom - at.y)
            - self.out(Side::Top, across, width, reach) * near(at.y - bounds.y);
        Point::new(at.x + dx, at.y + dy)
    }

    /// At `x` in a shape at `bounds`: how many times its height the shape is there, and how far
    /// down its middle has moved. What sits there squishes and moves with it.
    pub fn at(&self, bounds: Rectangle, x: f32) -> (f32, f32) {
        if self.is_flat() || bounds.height < 1.0 {
            return (1.0, 0.0);
        }
        let reach = (bounds.width.min(bounds.height) / 2.0).max(1.0);
        let across = (x - bounds.x) / bounds.width;
        let top = self.out(Side::Top, across, bounds.width, reach);
        let bottom = self.out(Side::Bottom, across, bounds.width, reach);
        (
            (bounds.height + top + bottom) / bounds.height,
            (bottom - top) / 2.0,
        )
    }

    #[cfg(test)]
    pub fn value(&self, side: Side, along: f32) -> f32 {
        self.side(side, along)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    const FRAME: f32 = 1.0 / 60.0;

    fn settle(jelly: &mut Jelly, mut each: impl FnMut(&Jelly)) -> usize {
        for frame in 0..1000 {
            jelly.advance(FRAME);
            each(jelly);
            if jelly.at_rest() {
                jelly.settle();
                return frame;
            }
        }
        panic!("never came to rest");
    }

    #[test]
    fn a_poke_spreads_then_settles() {
        let mut jelly = Jelly::default();
        jelly.kick(Side::Top, |along| {
            if (along - 0.5).abs() < 0.03 {
                200.0
            } else {
                0.0
            }
        });
        let mut spread: f32 = 0.0;
        settle(&mut jelly, |jelly| {
            spread = spread.max(jelly.value(Side::Top, 0.5 + 3.0 / ACROSS as f32));
        });
        assert!(spread > 0.1, "it reaches its neighbors: {spread}");
        assert!(jelly.is_flat());
    }

    #[test]
    fn a_poke_by_a_corner_moves_the_side_too() {
        let mut jelly = Jelly::default();
        jelly.kick(Side::Top, |along| if along > 0.9 { 200.0 } else { 0.0 });
        let mut side: f32 = 0.0;
        settle(&mut jelly, |jelly| {
            side = side.max(jelly.value(Side::Right, 0.0))
        });
        assert!(side > 0.1, "{side}");
    }

    #[test]
    fn flat_moves_nothing_and_the_profile_is_continuous() {
        let mut jelly = Jelly::default();
        let bounds = Rectangle::new(Point::new(10.0, 4.0), Size::new(120.0, 30.0));
        let at = Point::new(40.0, 4.0);
        assert_eq!(jelly.displace(bounds, at), at);
        assert_eq!(jelly.at(bounds, 40.0), (1.0, 0.0));

        jelly.kick(Side::Top, |along| {
            300.0 * (std::f32::consts::PI * along).sin()
        });
        jelly.advance(0.05);
        let mut previous = jelly.value(Side::Top, 0.0);
        for step in 1..=500 {
            let value = jelly.value(Side::Top, step as f32 / 500.0);
            assert!((value - previous).abs() < 0.05, "{previous} → {value}");
            previous = value;
        }
        // The top rose, so the shape's taller, and its middle went up.
        let (taller, down) = jelly.at(bounds, 70.0);
        assert!(taller > 1.0 && down < 0.0);
        assert!(jelly.displace(bounds, Point::new(70.0, 4.0)).y < 4.0);
        // Nothing moves the bottom.
        assert_eq!(jelly.displace(bounds, Point::new(70.0, 34.0)).y, 34.0);
    }

    #[test]
    fn a_stream_of_kicks_levels_off_inside_the_room() {
        let mut jelly = Jelly::default();
        let speed = poke(theme::JELLY_BUMP);
        let bump = |along: f32| {
            if (along - 0.5).abs() < 0.15 {
                speed
            } else {
                0.0
            }
        };
        let (mut furthest, mut previous): (f32, f32) = (0.0, 0.0);
        // Nudged every 3 frames for 2 s, like a held volume key.
        for frame in 0..120 {
            if frame % 3 == 0 {
                jelly.kick(Side::Top, bump);
            }
            jelly.advance(FRAME);
            let out = jelly.value(Side::Top, 0.5);
            // Never faster than a kick sets it off.
            assert!(
                (out - previous).abs() <= speed * FRAME,
                "{previous} → {out}"
            );
            previous = out;
            furthest = furthest.max(out);
        }
        assert!(furthest < 0.9 * theme::NUDGE_BUMP_ROOM, "{furthest}");
        settle(&mut jelly, |_| {});
    }
}
