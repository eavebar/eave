//! The screen's edges as walls (D-104). An island pushed by a nudge, dragged or jellied keeps
//! `WALL_GAP` from the screen's sides and top, round its top corners too, and a push that gets
//! there bounces off. Only the sides it isn't attached to are walls: an attached side reaches past
//! the edge on purpose (D-70).

use iced::{Point, Rectangle, Size};

use crate::squircle::{self, CornerShape, Polygons};
use crate::theme;

/// The screen's edges an island keeps clear of, in the surface's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walls {
    /// Where the screen's top edge is, as the bar draws it (grown), if it's a wall.
    pub top: Option<f32>,
    /// Whether the screen's left and right sides are walls.
    pub left: bool,
    pub right: bool,
    /// The screen's top left and top right corners, which round the walls where they meet.
    pub corners: [CornerShape; 2],
}

/// How far the outlines may stray from their curves here: plenty, next to `WALL_GAP`, and far
/// fewer pieces to measure than the squircle's own tolerance (D-119).
const CUT: f32 = 0.05;

/// Which way an island moves toward a wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    Up,
    Right,
    Left,
}

/// How far `shape`, with `corners` (top left, top right, bottom right, bottom left), can move up,
/// right, down and left before it comes within `WALL_GAP` of a wall, on a surface `width` wide.
/// Infinite where there's no wall, and down, where there never is one.
pub fn rooms(walls: Walls, width: f32, shape: Rectangle, corners: [CornerShape; 4]) -> [f32; 4] {
    let room = |toward| room(walls, width, shape, corners, toward);
    [
        room(Toward::Up),
        room(Toward::Right),
        f32::INFINITY,
        room(Toward::Left),
    ]
}

fn room(
    walls: Walls,
    width: f32,
    shape: Rectangle,
    corners: [CornerShape; 4],
    toward: Toward,
) -> f32 {
    let gap = theme::WALL_GAP;
    // As far as it could go if the walls met square.
    let straight = match toward {
        Toward::Up => walls.top.map(|top| shape.y - top - gap),
        Toward::Right => walls.right.then_some(width - gap - (shape.x + shape.width)),
        Toward::Left => walls.left.then_some(shape.x - gap),
    };
    let Some(straight) = straight else {
        return f32::INFINITY;
    };
    let straight = straight.max(0.0);
    // Without a top, or a rounded corner, nothing comes nearer than the straight walls.
    let Some(top) = walls.top else {
        return straight;
    };
    if walls.corners.iter().all(|corner| corner.radius <= 0.0) {
        return straight;
    }
    // The screen, from its top edge on down, far enough that only its top corners count.
    let far = 1.0e4;
    let mut screen = Polygons::within(CUT);
    squircle::rectangle(
        &mut screen,
        Rectangle::new(Point::new(0.0, top), Size::new(width, far)),
        [
            walls.corners[0],
            walls.corners[1],
            CornerShape::SQUARE,
            CornerShape::SQUARE,
        ],
    );
    let way = match toward {
        Toward::Up => (0.0, -1.0),
        Toward::Right => (1.0, 0.0),
        Toward::Left => (-1.0, 0.0),
    };
    // How much further than the gap it is from the screen, moved `by` that way.
    let room = |by: f32| {
        let mut island = Polygons::within(CUT);
        let moved = Rectangle::new(
            Point::new(shape.x + way.0 * by, shape.y + way.1 * by),
            shape.size(),
        );
        squircle::rectangle(&mut island, moved, corners);
        squircle::clearance(&screen, &island) - gap
    };
    // Only the last stretch, where the island would be in a corner, can come too close: start
    // there, unless that's already too close.
    let reach = walls
        .corners
        .iter()
        .map(|corner| corner.radius * (1.0 + corner.smoothing))
        .fold(0.0, f32::max);
    let mut near = (straight - reach - gap).max(0.0);
    let mut left = room(near);
    if left < 0.0 {
        if near == 0.0 {
            return 0.0;
        }
        near = 0.0;
        left = room(0.0);
        if left < 0.0 {
            return 0.0;
        }
    }
    // Moved no further than the room it has left, nothing of it can come within the gap, so it
    // never steps past a corner it would cross: step by that until it's as good as there (D-119).
    // Square on, it gets there in a step; slanting into a curve, in a few.
    const THERE: f32 = 0.01;
    for _ in 0..64 {
        if near >= straight || left < THERE {
            break;
        }
        near = (near + left).min(straight);
        left = room(near);
    }
    near.min(straight)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walls(radius: f32) -> Walls {
        Walls {
            top: Some(0.0),
            left: true,
            right: true,
            corners: [CornerShape::new(radius, 0.0); 2],
        }
    }

    const SQUARE: [CornerShape; 4] = [CornerShape::SQUARE; 4];

    #[test]
    fn square_walls_leave_the_gap() {
        let shape = Rectangle::new(Point::new(6.0, 6.0), Size::new(100.0, 30.0));
        let [up, right, down, left] = rooms(walls(0.0), 1000.0, shape, SQUARE);
        assert_eq!(up, 6.0 - theme::WALL_GAP);
        assert_eq!(left, 6.0 - theme::WALL_GAP);
        assert_eq!(right, 1000.0 - 106.0 - theme::WALL_GAP);
        assert_eq!(down, f32::INFINITY);
    }

    #[test]
    fn a_round_corner_takes_room_away() {
        // A square island in a round corner: its top left gets close long before its side does.
        let shape = Rectangle::new(Point::new(20.0, 6.0), Size::new(100.0, 30.0));
        let square = rooms(walls(0.0), 1000.0, shape, SQUARE)[3];
        let round = rooms(walls(30.0), 1000.0, shape, SQUARE)[3];
        assert!(round < square, "{round} {square}");
        // And it's right: moved that far, it's a gap away.
        let mut screen = Polygons::default();
        squircle::rectangle(
            &mut screen,
            Rectangle::new(Point::ORIGIN, Size::new(1000.0, 1.0e4)),
            [
                CornerShape::new(30.0, 0.0),
                CornerShape::new(30.0, 0.0),
                CornerShape::SQUARE,
                CornerShape::SQUARE,
            ],
        );
        let mut island = Polygons::default();
        squircle::rectangle(&mut island, shape - iced::Vector::new(round, 0.0), SQUARE);
        let clearance = squircle::clearance(&screen, &island);
        assert!((clearance - theme::WALL_GAP).abs() < 0.05, "{clearance}");
    }

    #[test]
    fn no_wall_where_it_is_attached() {
        let shape = Rectangle::new(Point::new(-20.0, -20.0), Size::new(100.0, 30.0));
        let walls = Walls {
            top: None,
            left: false,
            right: true,
            corners: [CornerShape::new(30.0, 0.0); 2],
        };
        let [up, _, _, left] = rooms(walls, 1000.0, shape, SQUARE);
        assert_eq!(up, f32::INFINITY);
        assert_eq!(left, f32::INFINITY);
    }
}
