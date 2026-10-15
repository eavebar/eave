//! An island: the squircle behind a zone's items. It never snaps to a new shape. When items come
//! or go, its edges spring to the new size, the items that stay slide to their new places and new
//! ones fade in. It reacts to the pointer like something physical: a press squeezes it in a
//! little, dragging pulls it along with growing resistance, and letting go springs it back.
//!
//! An island can also become a popover. Given one, it morphs into the panel's shape, growing from
//! where it sits (the dormer's drops from it into the middle of the screen), while its items fade
//! out and the panel's content fades in. Closing runs the morph backwards and reports when it's
//! done. Its outline is a skin (D-111): as the edges set off it stretches the way they go, and as
//! they land it carries on, splatting into the top. The panel lives in an overlay, so it's laid
//! out against the whole surface, drawn over the other islands, and sees events first.
//!
//! An island can have a toggle, a control of the host's at one end, there only while the pointer
//! is over the island: hovering grows the island to make room for it, and it fades in.
//!
//! It only watches pointer events on the items and never captures them, so the items work exactly
//! as they would without it. Like the switcher, it steps one frame at a time when the compositor
//! asks, and stops asking once everything is at rest.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eave_api::{Nudge, NudgeRequest};

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::alignment::Horizontal;
use iced::keyboard::{self, key};
use iced::widget::canvas::{Cache, Frame, Path};
use iced::{
    Color, Element, Event, Length, Padding, Point, Rectangle, Size, Transformation, Vector, window,
};

use crate::bezel::Sag;
use crate::field::AnyFocused;
use crate::glass;
use crate::hand::{self, rubber_band};
use crate::jelly::{self, Jelly, Side};
use crate::pour;
use crate::spring::{FADE, HAND, LAG, LEAD, MAX_FRAME, MORPH, REACT, REST_DISTANCE, Spring};
use crate::squircle::{self, CornerShape, squircle_corners, warp};
use crate::squish::{self, Squish};
use crate::theme::{self, Palette};
use crate::wall::{self, Walls};

pub struct Island<'a, M> {
    /// Each item's key, which follows it when items before it come or go.
    keys: Vec<u64>,
    items: Vec<Element<'a, M>>,
    /// Which way the island grows into a popover: from its left edge, its right edge or its center.
    align: Horizontal,
    popover: Option<Popover<'a, M>>,
    /// The last nudge each item asked for, numbered so the island plays each one once.
    nudges: Vec<(u64, (u64, NudgeRequest))>,
    /// Each item's width group: items in the same one are as wide as the widest of them.
    groups: Vec<Option<&'a str>>,
    /// Where the toggle is among the items, if there is one.
    toggle: Option<usize>,
    /// Where the toggle is, and whether the collapsible items show.
    toggle_side: Horizontal,
    expanded: bool,
    leaving: Vec<u64>,
    on_gone: Option<M>,
    /// A live island: snug around its items.
    live: bool,
    /// How far its shape reaches past its layout, toward the edges it's attached to (D-70), and its
    /// popover's with it.
    bleed: Padding,
    /// The width of a notch centered at the top of the surface, which its popover stays clear of.
    notch: Option<f32>,
    /// How it's rounded outwards where it meets the top edge, if it's attached there (D-72).
    shoulders: Option<CornerShape>,
    /// The bottom of a solid bar it's in, in surface coordinates (D-89): its shoulders are there,
    /// and it flows out of it.
    band: Option<f32>,
    /// Its corners: top left, top right, bottom right, bottom left (D-73).
    corners: [CornerShape; 4],
    /// How far past its layout it can be seen each way, less where it can't: its items are
    /// centered in that.
    shows: Padding,
    /// How tall it is at rest: the theme's, or down to where the notch ends (D-76).
    height: f32,
    /// Where it puts its shape as it's drawn, if given: for another island to come out of it.
    shape: Option<Shape>,
    /// Whether it may turn to glass, with the glass plugin ([`glass`]).
    glass: bool,
    /// Whether its ink goes darker over the glass: over something bright.
    darker: bool,
    /// The screen's edges it keeps clear of, if any (D-104).
    walls: Option<Walls>,
}

/// An island's shape as it was last drawn, in surface coordinates: a popover it's turning into
/// included. Shared with what comes out of it and goes back into it ([`Popover::home`]).
pub type Shape = Rc<Cell<Rectangle>>;

/// The control at one end of an island that shows and hides its collapsible items: there while
/// the pointer is over the island, or always while they show.
pub struct Toggle<'a, M> {
    pub element: Element<'a, M>,
    /// Before the first item (`Left`), or after the last: where the items come out.
    pub side: Horizontal,
    /// Whether the collapsible items show. When it changes, the island squashes the way they
    /// went, like a nudge.
    pub expanded: bool,
    /// The collapsible items going away, by key: they take no room, so the island closes at
    /// once, and each stays where it was until the toggle or the edge reaches it, then goes back
    /// into the toggle (D-116).
    pub leaving: Vec<u64>,
    /// Sent once the last of them is gone, to take them out.
    pub on_gone: Option<M>,
}

/// The toggle's key among the items' keys: no item has it.
const TOGGLE: u64 = u64::MAX;

/// How far two items, or an item and the shape's edge, may overlap and still count as clear of
/// each other: a rounded pixel.
const OVERLAP: f32 = 0.5;

/// What an island turns into while it's a popover.
pub struct Popover<'a, M> {
    /// Which popover it is. When it changes while the island is one, the island morphs straight
    /// to the new one, and the new content fades in.
    pub key: u64,
    pub content: Element<'a, M>,
    /// False while closing: the island morphs back, then sends `on_closed`.
    pub open: bool,
    /// The room the panel needs, in surface coordinates, whenever that changes: where it goes,
    /// and while the shape is still on its way there, where it was before. So a panel that
    /// shrinks keeps its room until the shape has landed, and the surface never cuts it off.
    pub on_placed: Box<dyn Fn(Rectangle) -> M + 'a>,
    pub on_closed: M,
    /// Sent when the pointer comes onto the popover (true) or leaves it (false), if given.
    pub on_hover: Option<Box<dyn Fn(bool) -> M + 'a>>,
    /// Its width, padding included: [`theme::PANEL_WIDTH`], or wider for reading.
    pub width: f32,
    /// Where it goes: under the island, or across the middle of the surface.
    pub placement: Placement,
    /// Turns a key the panel's content left alone into a message, if given: the dormer's arrows
    /// pick a row while its field keeps the rest, a widget's shortcuts press its rows (D-91).
    pub on_key: Option<OnKey<'a, M>>,
    /// Comes out as a drop instead of growing out of the island, if given: from this far down the
    /// surface, under the island's middle (D-86). At rest it's a dot there. It swells into a ball
    /// that falls out on a neck, which pinches off, and the ball unfolds into the panel. Closing
    /// runs it back.
    pub drop: Option<f32>,
    /// The color of what a drop comes out of, if it isn't the island's: the notch's black, in the
    /// light scheme. The drop starts out that color and turns the island's as it unfolds, and
    /// back as it folds up again (D-92).
    pub drop_color: Option<Color>,
    /// Sent as a drop's neck lets go of it (false) or catches it back (true), if given: the moment
    /// it leaves where it comes out of, and touches it again.
    pub on_neck: Option<Box<dyn Fn(bool) -> M + 'a>>,
    /// How far the bottom of what a drop comes out of is from where it rests right now, as drawn
    /// by whatever draws it, if given: its neck flows from there, so they never come apart.
    pub sag: Option<Sag>,
    /// Comes out of another island's shape, and goes back into it, instead of its own, if given:
    /// wherever that is drawn right now (D-90).
    pub home: Option<Shape>,
}

/// Turns a key, with the modifiers held, into a message, or leaves it alone. The flag says whether
/// a text field in the panel has the focus.
pub type OnKey<'a, M> = Box<dyn Fn(&keyboard::Key, keyboard::Modifiers, bool) -> Option<M> + 'a>;

/// Where a popover goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Under its island, growing from its side ([`place`]).
    Below,
    /// Centered across the surface, its top this far down: the dormer, which drops out of an
    /// island at the top into the middle of the screen (D-84).
    Centered { top: f32 },
    /// In the middle of the screen both ways: `room` is how far it reaches down from the surface's
    /// top, and the top stays at least `min_top` down. A widget's dialog (D-90).
    Middle { room: f32, min_top: f32 },
}

impl<'a, M> Island<'a, M> {
    pub fn new(items: impl IntoIterator<Item = (u64, Element<'a, M>)>) -> Self {
        let (keys, items) = items.into_iter().unzip();
        Self {
            keys,
            items,
            align: Horizontal::Center,
            popover: None,
            nudges: Vec::new(),
            groups: Vec::new(),
            toggle: None,
            toggle_side: Horizontal::Right,
            expanded: false,
            leaving: Vec::new(),
            on_gone: None,
            live: false,
            bleed: Padding::ZERO,
            notch: None,
            shoulders: None,
            band: None,
            corners: [CornerShape::new(theme::ISLAND_RADIUS, theme::ISLAND_SMOOTHING); 4],
            shows: Padding::ZERO,
            height: theme::ISLAND_HEIGHT as f32,
            shape: None,
            walls: None,
            glass: false,
            darker: false,
        }
    }

    /// May turn to glass, with the glass plugin: where it's its fill, what's behind shows through.
    pub fn glass(mut self, glass: bool) -> Self {
        self.glass = glass;
        self
    }

    /// Its ink goes darker over the glass, rather than lighter: it turns as this changes.
    pub fn ink(mut self, darker: bool) -> Self {
        self.darker = darker;
        self
    }

    /// Keeps `WALL_GAP` from these edges of the screen, and bounces off them (D-104).
    pub fn walls(mut self, walls: Walls) -> Self {
        self.walls = Some(walls);
        self
    }

    /// Puts its shape here every time it's drawn.
    pub fn shape(mut self, shape: Shape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// This tall at rest, instead of the theme's.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Reaches past its layout by this much each way: to the screen edges it's attached to, and
    /// past them, so they're never seen to come apart while it moves. Its popover does too.
    pub fn bleed(mut self, bleed: Padding) -> Self {
        self.bleed = bleed;
        self
    }

    /// A notch this wide, centered at the top of the surface: its popover stays on its side.
    pub fn notch(mut self, notch: Option<f32>) -> Self {
        self.notch = notch;
        self
    }

    /// Rounded outwards like this where it meets the top of the surface, on each side that doesn't
    /// reach on past it, if it's attached to the top: it hangs from the edge as the notch does.
    pub fn shoulders(mut self, shoulders: Option<CornerShape>) -> Self {
        self.shoulders = shoulders;
        self
    }

    /// In a solid bar whose bottom is at `band`, from the surface's top (D-89): its shoulders sit
    /// there instead of at the top, and whatever of it reaches below flows out of the bar, the
    /// shoulders growing with it from nothing.
    pub fn band(mut self, band: Option<f32>) -> Self {
        self.band = band;
        self
    }

    /// Seen this far past its layout each way: up to the screen's edges it's attached to, and short
    /// of what the notch hides (a negative side). Its items sit in the middle of what's seen.
    pub fn shows(mut self, shows: Padding) -> Self {
        self.shows = shows;
        self
    }

    /// Rounded like this: top left, top right, bottom right, bottom left, live or not.
    pub fn corners(mut self, corners: [CornerShape; 4]) -> Self {
        self.corners = corners;
        self
    }

    /// Makes it a live island: as much room at its ends as above and below its items, so it's
    /// snug around them. Its corners are any island's where it sits (D-78).
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    pub fn align(mut self, align: Horizontal) -> Self {
        self.align = align;
        self
    }

    pub fn popover(mut self, popover: Option<Popover<'a, M>>) -> Self {
        self.popover = popover;
        self
    }

    /// Each item's latest nudge, by key, with its number. Only ones newer than the island has seen
    /// play.
    pub fn nudges(mut self, nudges: Vec<(u64, (u64, NudgeRequest))>) -> Self {
        self.nudges = nudges;
        self
    }

    /// Each item's width group, in the order of the items.
    pub fn width_groups(mut self, groups: Vec<Option<&'a str>>) -> Self {
        self.groups = groups;
        if self.toggle == Some(0) {
            self.groups.insert(0, None);
        }
        self
    }

    /// A control shown at one end, while the pointer is over the island or it's pinned, and the
    /// island isn't a popover.
    pub fn toggle(mut self, toggle: Option<Toggle<'a, M>>) -> Self {
        let Some(Toggle {
            element,
            side,
            expanded,
            leaving,
            on_gone,
        }) = toggle
        else {
            return self;
        };
        self.toggle_side = side;
        self.expanded = expanded;
        self.leaving = leaving;
        self.on_gone = on_gone;
        let index = if side == Horizontal::Left {
            if !self.groups.is_empty() {
                self.groups.insert(0, None);
            }
            0
        } else {
            self.items.len()
        };
        self.keys.insert(index, TOGGLE);
        self.items.insert(index, element);
        self.toggle = Some(index);
        self
    }
}

struct State {
    motion: Motion,
    /// The squircle, rebuilt only when its size changes, or while its jelly moves: at rest it's
    /// built once.
    shape: Cache,
    /// Whether `shape` was last built bent by the jelly.
    shape_bent: Cell<bool>,
    /// The corners `shape` was last built with: a drop's round off as it unfolds.
    shape_corners: Cell<[CornerShape; 4]>,
    /// Where a solid bar's bottom was on the shape it was drawn with, so it's drawn again when
    /// that moves.
    shape_band: Cell<Option<f32>>,
    /// The color `shape` was filled with, so a new theme, or a drop turning, fills it again.
    shape_fill: Cell<Color>,
    /// The popover panel's widget state, while there is one.
    panel: Option<Tree>,
    /// Whether the pointer is over the island, which shows the toggle.
    hovered: bool,
    /// Where the island itself is within its layout, from the layout's left, and how wide: the
    /// layout is wider while a popover is open, to keep the panel's room clear of its neighbors.
    own: (f32, f32),
    /// Whether the collapsible items showed last time, to squash when that changes. `None` until
    /// the island has had a look.
    expanded: Option<bool>,
    /// Whether the last layout made room for the toggle. A hidden one is still laid out, but
    /// takes no room and gets no events.
    toggle_shown: bool,
    /// Whether it's a live island, so `shape` is rebuilt when that changes.
    live: bool,
    /// Its shoulders and corners, so `shape` is rebuilt when they change.
    shoulders: Option<CornerShape>,
    corners: [CornerShape; 4],
}

/// Where everything is going and how it's moving there. Positions are in logical pixels, in
/// surface coordinates; fades go from 0 (hidden) to 1 (shown).
#[derive(Debug)]
struct Motion {
    /// Where the pointer went down, while the primary button is held.
    press: Option<Point>,
    /// Where the island was grabbed, from the shape's top left: the drag stretches the shape most
    /// there. Kept after letting go until it's back in place, so the spring back stays there too.
    grab: Option<Vector>,
    /// How far the drag pulls: the island under the pointer moves this far, the rest less.
    x: Spring,
    y: Spring,
    /// How far each side has moved in.
    inset: Spring,
    /// Which way the ink goes over glass: 0 lighter, 1 darker, and in between as it turns.
    ink: Spring,
    /// Where the island's layout puts it. Unknown until it's first laid out.
    island: Option<Rectangle>,
    /// The drawn shape's edges, chasing `island`, or the panel while a popover is open.
    edges: Edges,
    items: Vec<ItemMotion>,
    popover: PopoverMotion,
    /// How far a nudge moved the whole island, on top of any drag.
    push_x: Spring,
    push_y: Spring,
    /// The outline, which a nudge pokes and which smooths the poke out by itself.
    jelly: Jelly,
    /// The shelves an up or down nudge holds out of the top and the bottom (D-105).
    shelves: [ShelfMotion; 2],
    /// The screen's edges it keeps clear of, how wide the surface is, and its corners (D-104).
    walls: Option<(Walls, f32, [CornerShape; 4])>,
    /// How far it can go up, right, down and left from where it rests before it's too close to
    /// a wall, and what that was worked out for: where it rests and its walls.
    rooms: [f32; 4],
    rooms_for: Option<(Rectangle, Walls, f32, [CornerShape; 4])>,
    /// How much slower than usual the last nudge plays: the push springs and the jelly run on
    /// time divided by it. 1 unless the nudge said how long it should last.
    pace: f32,
    /// How far down the surface a popover that comes out as a drop does it ([`Popover::drop`]).
    drop: Option<f32>,
    /// The color of what it comes out of, if it isn't the island's ([`Popover::drop_color`]).
    drop_color: Option<Color>,
    /// Another island's shape it rests at instead of its own ([`Popover::home`]).
    home: Option<Shape>,
    /// The island's corners, which a drop rounds off into.
    corner: CornerShape,
    /// How far what a drop comes out of has moved its bottom ([`Popover::sag`]).
    sag: Option<Sag>,
    /// The newest nudge already played, once the island has been laid out.
    seen: Option<u64>,
    /// The fastest the island has slid as a whole in the push going on, signed (negative is
    /// left). 0 when it isn't being pushed.
    pushed: f32,
    /// How long the push going on has lasted, in seconds: the swell builds up over it.
    pushing: f32,
    /// The previous frame, while something is moving.
    last: Option<Instant>,
    /// Which end items come out of and go back into: the toggle's.
    from: Horizontal,
    /// Whether the collapsible items were just shown: they come out of the toggle even one alone.
    opening: bool,
    /// The items going away, by key.
    leaving: Vec<u64>,
    /// Whether the island said they're gone.
    told_gone: bool,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            press: None,
            grab: None,
            x: Spring::new(0.0, HAND),
            y: Spring::new(0.0, HAND),
            inset: Spring::new(0.0, HAND),
            ink: Spring::new(0.0, FADE),
            island: None,
            edges: Edges::at(Rectangle::default()),
            items: Vec::new(),
            popover: PopoverMotion::default(),
            push_x: Spring::new(0.0, REACT),
            push_y: Spring::new(0.0, REACT),
            jelly: Jelly::default(),
            shelves: [ShelfMotion::default(); 2],
            walls: None,
            rooms: [f32::INFINITY; 4],
            rooms_for: None,
            pace: 1.0,
            drop: None,
            drop_color: None,
            home: None,
            corner: CornerShape::SQUARE,
            sag: None,
            seen: None,
            pushed: 0.0,
            pushing: 0.0,
            last: None,
            from: Horizontal::Right,
            opening: false,
            leaving: Vec::new(),
            told_gone: false,
        }
    }
}

/// A shelf held out of the top or the bottom by up or down nudges (D-105): how far out it is,
/// where it starts and ends from the island's left, and until when it's held. Each nudge while
/// it's held only holds it longer, so a stream of them keeps it out, still, and it goes back once,
/// `NUDGE_HOLD` after the last.
#[derive(Debug, Clone, Copy)]
struct ShelfMotion {
    depth: Spring,
    from: Spring,
    to: Spring,
    until: Option<Instant>,
}

impl Default for ShelfMotion {
    fn default() -> Self {
        Self {
            depth: Spring::new(0.0, REACT),
            from: Spring::new(0.0, REACT),
            to: Spring::new(0.0, REACT),
            until: None,
        }
    }
}

impl ShelfMotion {
    /// Holds it out from `from` to `to` until `until`. Out of nothing, it starts there; already
    /// out, it slides over.
    fn hold(&mut self, from: f32, to: f32, until: Instant) {
        if self.depth.target == 0.0 && self.depth.value.abs() < REST_DISTANCE {
            self.from = Spring::new(from, REACT);
            self.to = Spring::new(to, REACT);
        } else {
            self.from.target = from;
            self.to.target = to;
        }
        self.depth.target = theme::NUDGE_SHELF * theme::REACT_SIZE;
        self.until = Some(until);
    }

    fn release(&mut self) {
        self.until = None;
        self.depth.target = 0.0;
    }

    fn shelf(&self) -> jelly::Shelf {
        jelly::Shelf {
            depth: self.depth.value,
            from: self.from.value,
            to: self.to.value,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Edges {
    left: Spring,
    top: Spring,
    right: Spring,
    bottom: Spring,
    /// Where the edges go (left, top, right, bottom). Their springs may aim past it.
    goal: [f32; 4],
    /// Whether they land on it, stopping dead, rather than ease into it.
    hits: bool,
    /// The tongue's edges (left, top, right, bottom), heading where the edges go (D-114). Only
    /// the two along the way the shape changes most are its own; across, it's the shape's.
    tongue: [Spring; 4],
    /// Whether that way is up and down.
    vertical: bool,
}

impl Edges {
    fn at(rect: Rectangle) -> Self {
        Self {
            left: Spring::new(rect.x, MORPH),
            top: Spring::new(rect.y, MORPH),
            right: Spring::new(rect.x + rect.width, MORPH),
            bottom: Spring::new(rect.y + rect.height, MORPH),
            goal: edges_of(rect),
            hits: false,
            tongue: edges_of(rect).map(|edge| Spring::new(edge, MORPH)),
            vertical: true,
        }
    }

    /// Heads for `rect`. One that `hits` it heads `LAND_PAST` of the way past each edge that
    /// changes the shape's size, and stops dead as it gets there ([`Edges::hit`]): it lands on
    /// it at speed, and what it carries goes into the skin. One that doesn't, as an opening
    /// popover, eases in. A left and right that only move the shape along ease in too.
    fn aim(&mut self, rect: Rectangle, hits: bool) {
        let goal = edges_of(rect);
        if goal == self.goal && hits == self.hits {
            return;
        }
        self.goal = goal;
        self.hits = hits;
        // The tongue runs along the way the shape changes most: its end going out runs on ahead,
        // its end coming in hangs on behind. A shape sliding along as a whole has none.
        let values = [self.left, self.top, self.right, self.bottom].map(|edge| edge.value);
        let travel =
            |a: usize, b: usize| (goal[a] - values[a]).abs().max((goal[b] - values[b]).abs());
        self.vertical = travel(1, 3) >= travel(0, 2);
        let along = if self.vertical { [1, 3] } else { [0, 2] };
        let sliding =
            (goal[along[0]] - values[along[0]]) * (goal[along[1]] - values[along[1]]) > 0.0;
        for (i, spring) in self.tongue.iter_mut().enumerate() {
            spring.target = goal[i];
            let outward = if i < 2 {
                goal[i] < values[i]
            } else {
                goal[i] > values[i]
            };
            spring.feel = match (along.contains(&i) && !sliding, outward) {
                (false, _) => MORPH,
                (true, true) => LEAD,
                (true, false) => LAG,
            };
        }
        let [left, top, right, bottom] = self.springs_mut();
        let [goal_left, goal_top, goal_right, goal_bottom] = goal;
        // Up and down, a shape always lands against the top, which hangs from the bar.
        for (a, b, goal_a, goal_b, across) in [
            (left, right, goal_left, goal_right, true),
            (top, bottom, goal_top, goal_bottom, false),
        ] {
            let sliding = across && (goal_a - a.value) * (goal_b - b.value) > 0.0;
            for (spring, goal) in [(a, goal_a), (b, goal_b)] {
                spring.target = if hits && !sliding {
                    goal + theme::LAND_PAST * (goal - spring.value)
                } else {
                    goal
                };
            }
        }
    }

    fn springs_mut(&mut self) -> [&mut Spring; 4] {
        [
            &mut self.left,
            &mut self.top,
            &mut self.right,
            &mut self.bottom,
        ]
    }

    /// Stops each edge dead that got to where it goes on its way past it.
    fn hit(&mut self) {
        let goal = self.goal;
        for (spring, goal) in self.springs_mut().into_iter().zip(goal) {
            if spring.target != goal && (spring.value - goal) * (spring.target - goal) >= 0.0 {
                *spring = Spring::new(goal, MORPH);
            }
        }
    }

    /// How fast each side's edge goes outward (left, top, right, bottom), less what it shares with
    /// the side across: a shape sliding as a whole doesn't deform, only one changing size does.
    fn outward(&self) -> [f32; 4] {
        let shared = |a: f32, b: f32| {
            if a * b > 0.0 {
                a.signum() * a.abs().min(b.abs())
            } else {
                0.0
            }
        };
        let (left, top) = (self.left.velocity, self.top.velocity);
        let (right, bottom) = (self.right.velocity, self.bottom.velocity);
        let (across, down) = (shared(left, right), shared(top, bottom));
        [
            -(left - across),
            -(top - down),
            right - across,
            bottom - down,
        ]
    }

    fn rect(&self) -> Rectangle {
        let (left, top) = (self.left.value, self.top.value);
        Rectangle::new(
            Point::new(left, top),
            Size::new(
                (self.right.value - left).max(0.0),
                (self.bottom.value - top).max(0.0),
            ),
        )
    }

    /// How far, from 0 to 1, the shape has come from `from` toward `to`.
    fn progress(&self, from: Rectangle, to: Rectangle) -> f32 {
        let distance = |a: Rectangle, b: [f32; 4]| {
            (a.x - b[0]).abs()
                + (a.y - b[1]).abs()
                + (a.x + a.width - b[2]).abs()
                + (a.y + a.height - b[3]).abs()
        };
        let total = distance(from, edges_of(to));
        if total < 1.0 {
            return 1.0;
        }
        let now = [
            self.left.value,
            self.top.value,
            self.right.value,
            self.bottom.value,
        ];
        (1.0 - distance(to, now) / total).clamp(0.0, 1.0)
    }
}

fn edges_of(rect: Rectangle) -> [f32; 4] {
    [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height]
}

/// One item's slide and fade, found again by its key after a change.
#[derive(Debug)]
struct ItemMotion {
    key: u64,
    x: Spring,
    shown: Spring,
    /// How far its own sideways nudge moves it, on top of the island's push.
    kick: Spring,
    /// How wide it's laid out.
    width: f32,
    /// Where it is in coming out with others, or going back with them (D-116).
    cascade: Option<Cascade>,
    /// How far it's slid toward the toggle, as it comes out of it or goes back into it.
    slide: Spring,
}

/// An item that comes out of the toggle with others, or goes back into it: it waits for the
/// edge to pass it, then catches up (D-116).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cascade {
    /// Waiting to be clear of the toggle and the items beside it, and inside the shape.
    Coming,
    /// Shown, until the toggle, an item or the shape's edge reaches it.
    Going,
    /// Reached, and fading out.
    Gone,
}

impl ItemMotion {
    fn new(key: u64, x: f32, width: f32) -> Self {
        Self {
            key,
            x: Spring::new(x, MORPH),
            shown: Spring {
                target: 1.0,
                ..Spring::new(0.0, MORPH)
            },
            kick: Spring::new(0.0, REACT),
            width,
            cascade: None,
            slide: Spring::new(0.0, FADE),
        }
    }

    /// Where it's drawn across, before the jelly and the stretch.
    fn drawn(&self) -> f32 {
        self.x.value + self.kick.value + self.slide.value
    }

    /// Waits for the edge to pass it before it comes out.
    fn wait(&mut self) {
        self.cascade = Some(Cascade::Coming);
        self.shown = Spring::new(0.0, FADE);
    }

    /// Comes out, from `toward` (the toggle's way, ±1) of where it rests.
    fn reveal(&mut self, toward: f32) {
        self.cascade = None;
        self.shown = Spring {
            target: 1.0,
            ..Spring::new(self.shown.value, FADE)
        };
        self.slide = Spring {
            target: 0.0,
            ..Spring::new(toward * theme::CASCADE_SLIDE, FADE)
        };
    }

    /// Goes back into the toggle, `toward` it (±1).
    fn hide(&mut self, toward: f32) {
        self.cascade = Some(Cascade::Gone);
        self.shown = Spring {
            target: 0.0,
            ..Spring::new(self.shown.value, FADE)
        };
        self.slide.target = toward * theme::CASCADE_SLIDE;
    }

    /// Called back on its way out: back from where it got to.
    fn call_back(&mut self) {
        self.cascade = None;
        self.shown.target = 1.0;
        self.slide.target = 0.0;
    }
}

#[derive(Debug)]
struct PopoverMotion {
    /// Whether the island has a popover at all, open or closing.
    present: bool,
    open: bool,
    /// Where the panel goes, once laid out.
    rect: Option<Rectangle>,
    /// Whether the surface is tall enough to show it. Until it is, the island waits.
    fits: bool,
    /// The last place that fit. An open panel that grows past the surface holds its
    /// shape here until the surface has grown, instead of heading back to the island.
    fitted: Option<Rectangle>,
    /// The last place reported to the host.
    reported: Option<Rectangle>,
    /// Whether the host has been told the island is back to itself.
    closed: bool,
    /// How many of a drop's stops ([`Motion::stops`]) it has been most of the way to this way of
    /// the morph: it heads for the next.
    via: usize,
    /// Whether a drop's neck just let go of it (false) or caught it (true), and nobody's been
    /// told yet.
    touched: Option<bool>,
    /// Whether a drop's neck joined it to where it comes out, last frame.
    joined: bool,
    /// Whether an opening drop has bounced as it arrived.
    arrived: bool,
    /// How much the island's items show: they make way for the panel.
    items: Spring,
    /// How much the panel's content shows.
    panel: Spring,
    /// Whether the content has shown in full since this way of the morph set off. Until then it
    /// waits for the shape to be halfway out of the island; after, the panel only changes size,
    /// and the content stays while it does.
    out: bool,
    /// Which popover it is.
    key: Option<u64>,
    /// Whether the pointer is on it, as last reported.
    hovered: bool,
}

impl Default for PopoverMotion {
    fn default() -> Self {
        Self {
            present: false,
            open: false,
            rect: None,
            fits: false,
            fitted: None,
            reported: None,
            closed: false,
            via: 0,
            touched: None,
            joined: false,
            arrived: false,
            items: Spring::new(1.0, FADE),
            panel: Spring::new(0.0, FADE),
            out: false,
            key: None,
            hovered: false,
        }
    }
}

impl PopoverMotion {
    /// Open, and with somewhere to open to: where it goes, or while the surface is still too
    /// short for that, the last place that fit.
    fn opening(&self) -> Option<Rectangle> {
        if !self.open {
            return None;
        }
        if self.fits { self.rect } else { self.fitted }
    }
}

impl Motion {
    fn press(&mut self, at: Point) {
        self.press = Some(at);
        self.grab = Some(at - self.edges.rect().position());
        self.inset.target = theme::PRESS_INSET;
    }

    /// Pulls toward the pointer, harder to move the further it goes, and never into a wall.
    fn drag(&mut self, to: Point) {
        let Some(from) = self.press else {
            return;
        };
        let [up, right, _, left] = self.room();
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let across = if dx > 0.0 { right } else { left };
        let down = if dy < 0.0 { up } else { f32::INFINITY };
        self.x.target = rubber_band(dx, theme::DRAG_LIMIT_X.min(across));
        self.y.target = rubber_band(dy, theme::DRAG_LIMIT_Y.min(down));
    }

    /// Takes in the screen's edges it keeps clear of, on a surface `width` wide, and its corners.
    /// How far it can go toward each is worked out again only when where it rests changes.
    fn wall(&mut self, walls: Option<Walls>, width: f32, corners: [CornerShape; 4]) {
        self.walls = walls.map(|walls| (walls, width, corners));
        let (Some(walls), Some(island)) = (walls, self.island) else {
            self.rooms = [f32::INFINITY; 4];
            self.rooms_for = None;
            return;
        };
        let key = (island, walls, width, corners);
        if self.rooms_for != Some(key) {
            self.rooms = wall::rooms(walls, width, island, corners);
            self.rooms_for = Some(key);
        }
        self.jelly.room = self.jelly_room();
    }

    /// How far the drawn shape can go up, right, down and left before it's too close to a wall.
    /// Round the screen's corners as it rests; a popover, which is somewhere else, only to the
    /// straight walls.
    fn room(&self) -> [f32; 4] {
        let Some((walls, width, corners)) = self.walls else {
            return [f32::INFINITY; 4];
        };
        let shape = self.shape();
        if self.popover.present {
            let walls = Walls {
                corners: [CornerShape::SQUARE; 2],
                ..walls
            };
            return wall::rooms(walls, width, shape, corners);
        }
        let Some(island) = self.island else {
            return self.rooms;
        };
        // Resting rooms, from where the shape is on its way to the island.
        let [up, right, down, left] = self.rooms;
        [
            up + (shape.y - island.y),
            right + (island.x + island.width - shape.x - shape.width),
            down,
            left + (shape.x - island.x),
        ]
    }

    /// How far the jelly can bend each side out: the frame's room, less what the walls take, less
    /// how far the island has gone toward them.
    fn jelly_room(&self) -> [f32; 4] {
        let [up, right, down, left] = self.room();
        let x = self.push_x.value + self.x.value;
        let y = self.push_y.value + self.y.value;
        [
            up - (-y).max(0.0),
            right - x.max(0.0),
            down,
            left - (-x).max(0.0),
        ]
        .map(|room| room.clamp(0.0, theme::SKIN_ROOM))
    }

    /// Keeps a push out of the walls: one that gets there bounces back, slower, and the side that
    /// hit squashes in, bulging the sides next to it.
    fn bounce_off_walls(&mut self) {
        let [up, right, _, left] = self.room();
        let drag = Vector::new(
            self.x.value * theme::DRAG_FOLLOW,
            self.y.value * theme::DRAG_FOLLOW,
        );
        // Ends of a side, eased: 0 in its first half, rising to 1 at its end.
        let end = |along: f32| {
            let t = (2.0 * along - 1.0).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        for (way, room, side) in [(1.0, right, Side::Right), (-1.0, left, Side::Left)] {
            let past = way * (self.push_x.value + drag.x) - room;
            if past <= 0.0 {
                continue;
            }
            self.push_x.value -= way * past;
            let speed = way * self.push_x.velocity;
            if speed > 0.0 {
                self.push_x.velocity = -way * speed * theme::WALL_BOUNCE;
                let squash = speed * theme::WALL_SQUASH;
                self.jelly.kick(side, |_| -squash);
                // Along the top and bottom, the end that hit is the one it went toward.
                let toward = move |along: f32| {
                    if way > 0.0 {
                        end(along)
                    } else {
                        end(1.0 - along)
                    }
                };
                for edge in [Side::Top, Side::Bottom] {
                    self.jelly
                        .kick(edge, |along| theme::BULGE * squash * toward(along));
                }
            }
        }
        let past = -(self.push_y.value + drag.y) - up;
        if past > 0.0 {
            self.push_y.value += past;
            let speed = -self.push_y.velocity;
            if speed > 0.0 {
                self.push_y.velocity = speed * theme::WALL_BOUNCE;
                let squash = speed * theme::WALL_SQUASH;
                self.jelly.kick(Side::Top, |_| -squash);
                // Down the sides runs top to bottom: their top ends bulge.
                for edge in [Side::Left, Side::Right] {
                    self.jelly
                        .kick(edge, |along| theme::BULGE * squash * end(1.0 - along));
                }
            }
        }
    }

    fn release(&mut self) {
        self.press = None;
        self.x.target = 0.0;
        self.y.target = 0.0;
        self.inset.target = 0.0;
    }

    /// Plays the nudges it hasn't seen yet, with where each item starts across the surface and
    /// how wide it is. A new island only notes them: they happened before it. A held island, or
    /// one with a popover, lets them pass.
    fn nudges<N: Into<NudgeRequest>>(
        &mut self,
        nudges: impl IntoIterator<Item = (u64, (u64, N))>,
        span: impl Fn(u64) -> Option<(f32, f32)>,
        now: Instant,
    ) {
        let seen = self.seen;
        let mut newest = seen.unwrap_or(0);
        for (key, (number, nudge)) in nudges {
            newest = newest.max(number);
            if seen.is_some_and(|seen| number > seen)
                && self.press.is_none()
                && !self.popover.present
            {
                let request = nudge.into();
                self.nudge(request, span(key), now);
                self.kick(key, request.nudge);
            }
        }
        self.seen = Some(newest);
    }

    /// Sets the island off: sideways as a whole, or up or down half a pixel. Its outline leans into
    /// it, swelling on the side it's going toward and flattening on the side it's leaving. Up or
    /// down, the edge over the item (at `span`, its left and width) holds a shelf out until the
    /// nudges stop: one that comes while it's held only holds it longer (D-105). The springs and
    /// the jelly bring it back. A nudge that says how long it lasts plays the same motion
    /// stretched out to that.
    fn nudge(&mut self, request: NudgeRequest, span: Option<(f32, f32)>, now: Instant) {
        let NudgeRequest { nudge, lasting } = request;
        // The springs run on time slowed by the pace, so the same kicks go just as far, slower.
        self.pace = lasting.map_or(1.0, |lasting| {
            (lasting.as_secs_f32() / REACT.settle_time().as_secs_f32()).clamp(0.5, 5.0)
        });
        let (push, lift) = (react(theme::NUDGE_PUSH), react(theme::NUDGE_LIFT));
        let lean = jelly::poke(theme::JELLY_LEAN);
        // From −1 at the start of a side to 1 at its end, eased at both.
        let lean_toward_end = move |along: f32| lean * (std::f32::consts::PI * (along - 0.5)).sin();
        match nudge {
            Nudge::Left | Nudge::Right => {
                let way = if nudge == Nudge::Left { -1.0 } else { 1.0 };
                self.push_x.velocity = way * push;
                for side in [Side::Top, Side::Bottom] {
                    self.jelly.kick(side, |along| way * lean_toward_end(along));
                }
            }
            Nudge::Up | Nudge::Down => {
                let (way, edge) = if nudge == Nudge::Up {
                    (-1.0, 0)
                } else {
                    (1.0, 1)
                };
                // The other edge lets go of its shelf: the change went the other way.
                self.shelves[1 - edge].release();
                let held = self.shelves[edge].until.is_some();
                if let (Some((x, width)), Some(island)) = (span, self.island) {
                    let from = x - island.x;
                    self.shelves[edge].hold(from, from + width, now + theme::NUDGE_HOLD);
                }
                // Held already: it stays out, and the island doesn't lurch again.
                if held {
                    return;
                }
                self.push_y.velocity = way * lift;
                // Down the sides runs top to bottom, so going up swells the top.
                for side in [Side::Left, Side::Right] {
                    self.jelly.kick(side, |along| way * lean_toward_end(along));
                }
            }
        }
    }

    /// While the island slides as a whole, pushed by a neighbor making room, the end it's pushed
    /// from swells taller, the more the longer and faster it goes, as if what it passed piled up
    /// there. When it stops, the swell is kicked across to the other end, and the jelly wiggles
    /// back flat. A resize moves its edges apart or together, not the same way, so it doesn't
    /// count, and neither does a popover's morph.
    fn lean_with_push(&mut self, elapsed: f32) {
        if self.popover.present || self.press.is_some() {
            self.pushed = 0.0;
            self.pushing = 0.0;
            return;
        }
        let (left, right) = (self.edges.left.velocity, self.edges.right.velocity);
        let speed = if left.signum() == right.signum() {
            left.signum() * left.abs().min(right.abs())
        } else {
            0.0
        };
        // From −1 at the start of a side to 1 at its end, eased at both.
        let toward_end = |along: f32| (std::f32::consts::PI * (along - 0.5)).sin();
        let strength =
            |speed: f32, most: f32| most * (speed.abs() / theme::PUSH_FULL_SPEED).min(1.0);
        if speed.abs() > theme::PUSH_MOVING {
            if speed.abs() > self.pushed.abs() {
                self.pushed = speed;
            }
            self.pushing += elapsed;
            // Going left, it's pushed from the right: its right end swells, gradually.
            let way = speed.signum();
            let building = (self.pushing / theme::PUSH_BUILD.as_secs_f32()).min(1.0);
            let lean = strength(speed, jelly::poke(theme::PUSH_LEAN)) * building;
            for side in [Side::Top, Side::Bottom] {
                self.jelly
                    .kick(side, |along| -way * lean * toward_end(along));
            }
        } else if self.pushed != 0.0 {
            let way = self.pushed.signum();
            let wiggle = strength(self.pushed, jelly::poke(theme::PUSH_WIGGLE));
            for side in [Side::Top, Side::Bottom] {
                self.jelly
                    .kick(side, |along| way * wiggle * toward_end(along));
            }
            self.pushed = 0.0;
            self.pushing = 0.0;
        }
    }

    /// Takes in where layout put the island and each item (by key, with its x), and whether it has
    /// a popover and if that's open. Everything that changed starts moving there.
    #[cfg(test)]
    fn arrange(
        &mut self,
        island: Rectangle,
        items: impl Iterator<Item = (u64, f32)>,
        popover: Option<bool>,
    ) {
        self.place(island, items.map(|(key, x)| (key, x, 0.0)), popover);
    }

    /// [`Motion::arrange`], with each item's width too.
    fn place(
        &mut self,
        island: Rectangle,
        items: impl Iterator<Item = (u64, f32, f32)>,
        popover: Option<bool>,
    ) {
        // A new island grows out of a sliver at its center. A drop starts as a dot, and one that
        // comes out of another island as that island.
        let existed = self.island.is_some();
        if self.island.is_none() {
            let sliver = match (self.drop, &self.home) {
                (Some(_), _) | (_, Some(_)) => self.home(island),
                (None, None) => Rectangle::new(
                    Point::new(island.center_x(), island.y),
                    Size::new(0.0, island.height),
                ),
            };
            self.edges = Edges::at(sliver);
        }
        self.island = Some(island);

        let mut kept = Vec::with_capacity(self.items.len());
        let mut coming = Vec::new();
        for (key, x, width) in items {
            let leaving = self.leaving.contains(&key);
            match self.items.iter().position(|item| item.key == key) {
                Some(index) => {
                    let mut item = self.items.swap_remove(index);
                    item.width = width;
                    match item.cascade {
                        // Going: it stays where it was, taking no room, while the toggle and the
                        // edge close over it.
                        _ if leaving => {
                            if item.cascade.is_none() {
                                item.cascade = Some(Cascade::Going);
                            } else if item.cascade == Some(Cascade::Coming) {
                                item.hide(0.0);
                            }
                        }
                        Some(Cascade::Going | Cascade::Gone) => {
                            item.x.target = x;
                            item.call_back();
                        }
                        _ => item.x.target = x,
                    }
                    kept.push(item);
                }
                None => {
                    let mut item = ItemMotion::new(key, x, width);
                    if leaving {
                        item.cascade = Some(Cascade::Gone);
                        item.shown = Spring::new(0.0, FADE);
                    } else if key != TOGGLE {
                        coming.push(kept.len());
                    }
                    kept.push(item);
                }
            }
        }
        // Whatever is left in `self.items` is gone.
        self.items = kept;
        // Collapsible items shown, or several coming at once, come out of the toggle as the edge
        // passes them. Not on a new island: it grows out with all its items. Anything else new
        // fades in in place.
        if existed && (self.opening || coming.len() > 1) {
            for index in coming {
                self.items[index].wait();
            }
        }
        self.opening = false;
        if self.leaving.is_empty() {
            self.told_gone = false;
        }

        let popover_state = &mut self.popover;
        match popover {
            Some(open) => {
                popover_state.present = true;
                if open && !popover_state.open {
                    popover_state.closed = false;
                }
                // Turning around, a drop goes by its ball again.
                if open != popover_state.open {
                    popover_state.out = false;
                    popover_state.via = 0;
                    popover_state.arrived = false;
                }
                popover_state.open = open;
            }
            None if popover_state.present => {
                // Taken away without closing: back to the island, wherever it got to.
                let (items, panel) = (popover_state.items, popover_state.panel);
                *popover_state = PopoverMotion {
                    items: Spring {
                        target: 1.0,
                        ..items
                    },
                    panel: Spring {
                        target: 0.0,
                        ..panel
                    },
                    ..PopoverMotion::default()
                };
            }
            None => {}
        }
        self.aim();
    }

    /// Lets out each item waiting to come out once it's clear, and takes each one going back
    /// once it isn't: clear of the toggle and every other item shown, and inside the shape
    /// (D-116). Whatever is still waiting once everything else is still goes at once.
    fn cascade(&mut self, settled: bool) {
        let pending =
            |item: &ItemMotion| matches!(item.cascade, Some(Cascade::Coming | Cascade::Going));
        if !self.items.iter().any(pending) {
            return;
        }
        let toward = if self.from == Horizontal::Left {
            -1.0
        } else {
            1.0
        };
        let (left, right) = (self.edges.left.value, self.edges.right.value);
        let spans: Vec<_> = self
            .items
            .iter()
            .filter(|item| item.shown.target > 0.5)
            .map(|item| (item.key, item.drawn(), item.drawn() + item.width))
            .collect();
        for item in &mut self.items {
            if !pending(item) {
                continue;
            }
            let (from, to) = (item.x.value, item.x.value + item.width);
            let inside = from >= left - OVERLAP && to <= right + OVERLAP;
            let covered = spans
                .iter()
                .any(|&(key, a, b)| key != item.key && a < to - OVERLAP && b > from + OVERLAP);
            let clear = inside && !covered;
            match item.cascade {
                Some(Cascade::Coming) if clear || settled => item.reveal(toward),
                Some(Cascade::Going) if !clear || settled => item.hide(toward),
                _ => {}
            }
        }
    }

    /// Whether the items going away are all gone, and nobody's been told yet.
    fn gone(&mut self) -> bool {
        if self.leaving.is_empty() || self.told_gone {
            return false;
        }
        let gone = self
            .items
            .iter()
            .filter(|item| self.leaving.contains(&item.key))
            .all(|item| item.cascade == Some(Cascade::Gone) && item.shown.value < REST_DISTANCE);
        self.told_gone = gone;
        gone
    }

    /// Notes which popover the island has. Another one than before, while it's still a popover,
    /// takes over the morph: the shape springs on from where it is to the new panel, with a
    /// squish, and the new content fades in once the shape is halfway.
    /// Returns whether it took over.
    fn swap(&mut self, key: Option<u64>) -> bool {
        let popover = &mut self.popover;
        let taken = if let (Some(new), Some(old)) = (key, popover.key)
            && new != old
            && popover.present
        {
            popover.panel = Spring::new(0.0, FADE);
            popover.out = false;
            popover.closed = false;
            true
        } else {
            false
        };
        popover.key = key;
        taken
    }

    /// One that takes over coming out of another island starts as that island, not as what this
    /// one was: a dialog out of a popover while the dormer still folds back comes out of the
    /// popover.
    fn rehome(&mut self) {
        if let (Some(island), Some(_)) = (self.island, &self.home) {
            self.edges = Edges::at(self.home(island));
            self.popover.via = 0;
        }
    }

    /// Points the shape and the fades where they should go now. During a popover's morph, each
    /// side's content only fades in once the other side's is gone and the shape is more than
    /// halfway its way. The two never show at once: each fades by covering itself with the
    /// island's color, which would cover the other side's content too where they overlap.
    fn aim(&mut self) {
        let Some(island) = self.island else {
            return;
        };
        let home = self.home(island);
        let ball = self.ball(island);
        let (out, back) = (self.stops(island, true), self.stops(island, false));
        let (stops_out, stops_back) = (slice(&out), slice(&back));
        let popover = &mut self.popover;
        let mut bounce = false;
        match (popover.opening(), popover.rect) {
            (Some(panel), _) => {
                // A drop swells where it comes out and falls as a ball first, heading on to each
                // next stop once it's most of the way to one.
                let heading = pass(&self.edges, &mut popover.via, home, stops_out).unwrap_or(panel);
                // Opening has nothing to hit (D-44): it eases in.
                self.edges.aim(heading, false);
                // A drop falls into place: it bounces as it gets there, like a workspace nudged.
                let mut ahead = self.edges.bottom;
                ahead.advance(REACT.peak_time());
                if ball.is_some()
                    && heading == panel
                    && !popover.arrived
                    && (panel.y + panel.height) - ahead.value < theme::DROP_LAND
                {
                    popover.arrived = true;
                    bounce = true;
                }
                popover.items.target = 0.0;
                // Out once, a change of size (another page, a row more) keeps the content: the
                // halfway rule measures from the island, and a big enough change of size would
                // cover the content with the island's color until the shape got there.
                popover.out |= popover.panel.value >= 1.0 - REST_DISTANCE;
                popover.panel.target = if popover.out
                    || popover.items.value < REST_DISTANCE
                        && self.edges.progress(ball.unwrap_or(island), panel) > 0.5
                {
                    1.0
                } else {
                    0.0
                };
            }
            (None, Some(panel)) if popover.present => {
                // A drop folds back into its ball and rises into where it came out first, the
                // same stops the other way. Not while it's open and waiting for room: that waits
                // at home.
                let heading = if popover.open {
                    home
                } else {
                    pass(&self.edges, &mut popover.via, panel, stops_back).unwrap_or(home)
                };
                // It lands on where it rests, and only goes by the stops.
                self.edges.aim(heading, !popover.open && heading == home);
                popover.panel.target = 0.0;
                popover.items.target = if popover.panel.value < REST_DISTANCE
                    && self.edges.progress(panel, home) > 0.5
                {
                    1.0
                } else {
                    0.0
                };
            }
            _ => {
                // Any other change of shape lands the same way.
                self.edges.aim(home, true);
                popover.items.target = 1.0;
                popover.panel.target = 0.0;
            }
        }
        if bounce {
            self.bounce(1.0);
        }
    }

    /// Lets the skin feel how its edges moved since they went `before` ([`Edges::outward`]): as
    /// an edge speeds up, its side stretches out, whichever way it goes (`SKIN_STRETCH`): ahead of
    /// an edge going out, behind one coming in. As it slows down and lands, its side carries on
    /// the way it went (`SKIN_CARRY`). Fullest in the side's middle (`SKIN_BOW`). The ring does the
    /// rest: it spreads it round the corners and keeps its area (D-111).
    fn feel_edges(&mut self, before: [f32; 4]) {
        let (stretch, mut carry) = skin_gains();
        if self.popover.opening().is_some() {
            // Opening has nothing to hit (D-44).
            carry *= theme::OPEN_CARRY;
        } else if self.drop.is_some() && self.popover.present {
            // A drop vanishes as it lands: it reacts as its neck catches it instead (D-86).
            carry = 0.0;
        }
        let bow = |along: f32| {
            1.0 - theme::SKIN_BOW + theme::SKIN_BOW * (std::f32::consts::PI * along).sin()
        };
        let after = self.edges.outward();
        for ((was, now), side) in
            before
                .into_iter()
                .zip(after)
                .zip([Side::Left, Side::Top, Side::Right, Side::Bottom])
        {
            let change = now - was;
            if change == 0.0 {
                continue;
            }
            let speeding = was * now >= 0.0 && now.abs() > was.abs();
            let speed = if speeding {
                stretch * (now.abs() - was.abs())
            } else {
                -carry * change
            };
            self.jelly.push(side, |along| speed * bow(along));
        }
    }

    /// Where the shape rests: the island, or for a drop, a dot where it comes out, or the other
    /// island it comes out of, wherever that's drawn right now.
    fn home(&self, island: Rectangle) -> Rectangle {
        match (self.drop, &self.home) {
            (Some(y), _) => {
                Rectangle::new(Point::new(island.center_x(), y + self.sag()), Size::ZERO)
            }
            (None, Some(home)) => home.get(),
            (None, None) => island,
        }
    }

    /// Where a drop goes by between home and the panel, in the order it goes out (`out`) or back:
    /// swelling into a ball at where it comes out, half in it, then that ball fallen out of it.
    /// None for anything else.
    fn stops(&self, island: Rectangle, out: bool) -> Option<[Rectangle; 2]> {
        let (Some(y), Some(ball)) = (self.drop, self.ball(island)) else {
            return None;
        };
        let swell = Rectangle {
            y: y - ball.height / 2.0,
            ..ball
        };
        Some(if out { [swell, ball] } else { [ball, swell] })
    }

    /// How far the bottom of what a drop comes out of is drawn from where it rests.
    fn sag(&self) -> f32 {
        self.sag.as_ref().map_or(0.0, |sag| sag.get())
    }

    /// The ball a drop goes by, on its way out and back: under where it comes out.
    fn ball(&self, island: Rectangle) -> Option<Rectangle> {
        let size = theme::DROP_BALL;
        self.drop.map(|y| {
            Rectangle::new(
                Point::new(island.center_x() - size / 2.0, y + theme::DROP_FALL),
                Size::new(size, size),
            )
        })
    }

    /// How round a drop is, from 0 to 1: a ball until it heads for the panel, then less and less
    /// the closer it gets. 0 for anything else.
    fn roundness(&self) -> f32 {
        let Some(island) = self.island else {
            return 0.0;
        };
        match (self.ball(island), self.popover.rect) {
            (Some(ball), Some(panel)) => 1.0 - self.edges.progress(ball, panel),
            (Some(_), None) => 1.0,
            (None, _) => 0.0,
        }
    }

    /// What the shape is filled with: the island's color, or a drop's, from that of what it comes
    /// out of while it's a ball to the island's as it unfolds into the panel.
    fn fill(&self, island: Color) -> Color {
        let Some(from) = self.drop_color.filter(|_| self.drop.is_some()) else {
            return island;
        };
        let t = 1.0 - self.roundness();
        let channel = |a: f32, b: f32| a + (b - a) * t;
        Color {
            r: channel(from.r, island.r),
            g: channel(from.g, island.g),
            b: channel(from.b, island.b),
            a: channel(from.a, island.a),
        }
    }

    /// A drop's corners, drawn `shape`: a ball's, rounding off into the island's as it unfolds
    /// into the panel.
    fn drop_corner(&self, shape: Rectangle) -> CornerShape {
        let (round, corner) = (self.roundness(), self.corner);
        let half = shape.width.min(shape.height) / 2.0;
        CornerShape::new(
            corner.radius + (half - corner.radius).max(0.0) * round,
            corner.smoothing * (1.0 - round),
        )
    }

    /// The neck between where a drop comes out and the drawn `shape` ([`pour::Pour`]), while
    /// they're close enough to flow together. It blends less while the drop is smaller than its
    /// ball, so a dot coming out or going in has hardly any.
    fn neck(&self, shape: Rectangle) -> Option<pour::Pour> {
        let (Some(y), Some(island)) = (self.drop, self.island) else {
            return None;
        };
        let size = (shape.width.min(shape.height) / theme::DROP_BALL).min(1.0);
        let from = pour::From::Line {
            y: y + self.sag(),
            x: island.center_x(),
            spread: theme::NECK_SPREAD,
        };
        let radius = self.drop_corner(shape).radius;
        pour::Pour::new(from, theme::NECK_BLEND * size, shape, radius)
    }

    /// The morph's tongue (D-114), drawn `shape`: where it runs on ahead of the shape or hangs on
    /// behind it, `TONGUE_WIDTH` of it across, about its middle. `None` while it's inside it.
    fn tongue(&self, shape: Rectangle) -> Option<Rectangle> {
        let edges = &self.edges;
        let [left, top, right, bottom] = edges.tongue.map(|spring| spring.value);
        let rect = if edges.vertical {
            let width = shape.width * theme::TONGUE_WIDTH;
            Rectangle::new(
                Point::new(shape.center_x() - width / 2.0, top),
                Size::new(width, bottom - top),
            )
        } else {
            let height = shape.height * theme::TONGUE_WIDTH;
            Rectangle::new(
                Point::new(left, shape.center_y() - height / 2.0),
                Size::new(right - left, height),
            )
        };
        let out = [
            shape.x - rect.x,
            shape.y - rect.y,
            rect.x + rect.width - (shape.x + shape.width),
            rect.y + rect.height - (shape.y + shape.height),
        ]
        .into_iter()
        .fold(0.0_f32, f32::max);
        (rect.width >= 1.0 && rect.height >= 1.0 && out >= 0.5).then_some(rect)
    }

    /// How round the tongue `rect` is at its corners: as the shape's, or all round if it's thinner.
    fn tongue_corner(&self, rect: Rectangle, shape: Rectangle) -> CornerShape {
        let corner = match self.drop {
            Some(_) => self.drop_corner(shape),
            None => self.corner,
        };
        CornerShape::new(
            corner.radius.min(rect.width.min(rect.height) / 2.0),
            corner.smoothing,
        )
    }

    /// The fillet joining the tongue `rect` to the drawn `shape` ([`pour::Pour`]): wider the
    /// further it's out of it, up to `POUR_BLEND`.
    fn tongue_pour(&self, rect: Rectangle, shape: Rectangle) -> Option<pour::Pour> {
        let out = [
            shape.x - rect.x,
            shape.y - rect.y,
            rect.x + rect.width - (shape.x + shape.width),
            rect.y + rect.height - (shape.y + shape.height),
        ]
        .into_iter()
        .fold(0.0_f32, f32::max);
        let blend = theme::POUR_BLEND * (out / theme::POUR_FULL).min(1.0);
        let radius = self.tongue_corner(rect, shape).radius;
        let body = match self.drop {
            Some(_) => self.drop_corner(shape).radius,
            None => self.corner.radius,
        };
        let from = pour::From::Box { rect, radius };
        pour::Pour::new(from, blend, shape, body)
    }

    /// Bounces a drop down (`way` 1) or up (−1): it lurches that way and back, its sides lean into
    /// it and the side it goes toward bulges, like an island nudged up or down, a little more and
    /// slower.
    fn bounce(&mut self, way: f32) {
        // Played slower, like a nudge that says how long it lasts: the same kicks, stretched out.
        self.pace = (theme::DROP_BOUNCE_LASTING.as_secs_f32() / REACT.settle_time().as_secs_f32())
            .clamp(0.5, 5.0);
        self.push_y.velocity = way * react(theme::DROP_BOUNCE);
        let (lean, bump) = (
            jelly::poke(theme::JELLY_LEAN),
            jelly::poke(theme::JELLY_BUMP),
        );
        // Down the sides runs top to bottom, so going down swells their bottom ends.
        for side in [Side::Left, Side::Right] {
            self.jelly.kick(side, |along| {
                way * lean * (std::f32::consts::PI * (along - 0.5)).sin()
            });
        }
        let edge = if way > 0.0 { Side::Bottom } else { Side::Top };
        self.jelly.kick(edge, |along| {
            bump * 0.5 * (1.0 - (std::f32::consts::TAU * along).cos())
        });
    }

    /// Bounces a drop as its neck lets go of it (down, opening) or catches it (up, closing), and
    /// notes it for the host, so what it comes out of reacts on the same frame.
    fn watch_neck(&mut self) {
        if self.drop.is_none() || !self.popover.present {
            return;
        }
        let joined = self
            .neck(self.shape() + self.offset())
            .is_some_and(|neck| neck.joined());
        let popover = &mut self.popover;
        let was = std::mem::replace(&mut popover.joined, joined);
        if was && !joined && popover.open {
            popover.touched = Some(false);
            self.bounce(1.0);
        } else if !was && joined && !popover.open {
            popover.touched = Some(true);
            self.bounce(-1.0);
        }
    }

    /// Takes in where the popover's panel goes, and whether the surface is tall enough for it.
    fn place_panel(&mut self, rect: Rectangle, fits: bool) {
        let popover = &mut self.popover;
        popover.rect = Some(rect);
        popover.fits = fits;
        if fits {
            popover.fitted = Some(rect);
        }
    }

    /// The room the panel needs on the surface: where it goes, and while the shape is still on
    /// its way, what was asked for before too. A shrinking shape keeps its room until it lands,
    /// then gives it back at once: the surface resizes twice, not every frame.
    ///
    /// Landed means at rest where the shape should go now, not just at rest: a plugin's page swap
    /// lays the panel out anew and is reported before [`Motion::aim`] points the edges at it.
    fn reach(&self) -> Option<Rectangle> {
        let rect = self.popover.rect?;
        let edges = &self.edges;
        let destination = self
            .popover
            .opening()
            .or(self.island.map(|island| self.home(island)));
        let there = destination.is_none_or(|to| {
            let at = edges.rect();
            [
                at.x - to.x,
                at.y - to.y,
                at.width - to.width,
                at.height - to.height,
            ]
            .iter()
            .all(|off| off.abs() < REST_DISTANCE)
        });
        let landed = there
            && [edges.left, edges.top, edges.right, edges.bottom]
                .iter()
                .all(Spring::at_rest);
        match self.popover.reported {
            Some(reported) if !landed => Some(rect.union(&reported)),
            _ => Some(rect),
        }
    }

    fn springs(&mut self) -> impl Iterator<Item = &mut Spring> {
        let edges = &mut self.edges;
        [
            &mut self.x,
            &mut self.y,
            &mut self.inset,
            &mut self.ink,
            &mut edges.left,
            &mut edges.top,
            &mut edges.right,
            &mut edges.bottom,
            &mut self.popover.items,
            &mut self.popover.panel,
        ]
        .into_iter()
        .chain(&mut edges.tongue)
        .chain(
            self.items
                .iter_mut()
                .flat_map(|item| [&mut item.x, &mut item.shown, &mut item.slide]),
        )
        .chain(
            self.shelves
                .iter_mut()
                .flat_map(|shelf| [&mut shelf.depth, &mut shelf.from, &mut shelf.to]),
        )
    }

    /// The springs a nudge moves, which run at its pace.
    fn paced(&mut self) -> impl Iterator<Item = &mut Spring> {
        [&mut self.push_x, &mut self.push_y]
            .into_iter()
            .chain(self.items.iter_mut().map(|item| &mut item.kick))
    }

    /// Sends the item that nudged sideways a little further than its island, so what changed
    /// (the cover of the next track, the active workspace) gets a push of its own.
    fn kick(&mut self, key: u64, nudge: Nudge) {
        let way = match nudge {
            Nudge::Left => -1.0,
            Nudge::Right => 1.0,
            Nudge::Up | Nudge::Down => return,
        };
        if let Some(item) = self.items.iter_mut().find(|item| item.key == key) {
            item.kick.velocity = way * react(theme::NUDGE_KICK);
        }
    }

    fn at_rest(&mut self) -> bool {
        self.items
            .iter()
            .all(|item| !matches!(item.cascade, Some(Cascade::Coming | Cascade::Going)))
            && self.jelly.at_rest()
            && self.springs().all(|spring| spring.at_rest())
            && self.paced().all(|spring| spring.at_rest())
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        self.aim();
        self.let_shelves_go(now);
        if self.at_rest() {
            self.let_go_of_grab();
            self.last = None;
            return false;
        }
        // The first frame after rest has no elapsed time to cover; a long stall is cut short so
        // the springs pick up where they were instead of jumping.
        let elapsed = self
            .last
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last))
            .min(MAX_FRAME)
            .as_secs_f32();
        self.last = Some(now);
        let before = self.edges.outward();
        for spring in self.springs() {
            spring.advance(elapsed);
        }
        self.edges.hit();
        self.feel_edges(before);
        self.jelly.size = self.edges.rect().size();
        let paced = elapsed / self.pace;
        for spring in self.paced() {
            spring.advance(paced);
        }
        self.bounce_off_walls();
        self.lean_with_push(paced);
        self.jelly.advance(paced);
        self.jelly.room = self.jelly_room();
        // The fades wait on how far the shape has come, and so do the items coming out of the
        // toggle or going back into it.
        self.aim();
        let settled = self.springs().all(|spring| spring.at_rest())
            && self.paced().all(|spring| spring.at_rest());
        self.cascade(settled);
        self.watch_neck();
        if self.at_rest() {
            self.springs().for_each(Spring::settle);
            self.paced().for_each(Spring::settle);
            self.jelly.settle();
            self.jelly.shelves = self.shelves.map(|shelf| shelf.shelf());
            self.let_go_of_grab();
            self.last = None;
            return false;
        }
        self.jelly.shelves = self.shelves.map(|shelf| shelf.shelf());
        true
    }

    /// Lets the shelves go whose hold is up, and all of them while a popover is out.
    fn let_shelves_go(&mut self, now: Instant) {
        let popover = self.popover.present;
        for shelf in &mut self.shelves {
            if popover || shelf.until.is_some_and(|until| now >= until) {
                shelf.release();
            }
        }
    }

    /// When a held shelf is up, if one is: nothing moves till then, so no frames are needed
    /// before it.
    fn wake(&self) -> Option<Instant> {
        self.shelves.iter().filter_map(|shelf| shelf.until).min()
    }

    /// Forgets where it was grabbed, once it's let go and back in place.
    fn let_go_of_grab(&mut self) {
        if self.press.is_none() {
            self.grab = None;
        }
    }

    /// Whether a closing popover is all the way back to the island, and nobody's been told yet.
    fn closing_done(&mut self) -> bool {
        let done = self.popover.present && !self.popover.open && !self.popover.closed;
        if done && self.at_rest() {
            self.popover.closed = true;
            return true;
        }
        false
    }

    /// The shape's frame: its edges. The skin bends what's drawn of it ([`Jelly::displace`]).
    fn shape(&self) -> Rectangle {
        self.edges.rect()
    }

    /// How squished something centered at `x` in the drawn `shape` is, and how far down the skin
    /// moves it: by how much taller the skin makes the shape there.
    fn squish_at(&self, shape: Rectangle, x: f32) -> (Squish, f32) {
        let (taller, down) = self.jelly.at(shape, x);
        (Squish { x: 1.0, y: taller }, down)
    }

    /// How squished an item is by its own kick ([`Motion::kick`]): narrower and a little taller the
    /// further out it is, as if it bumped into where it's going.
    fn kicked(&self, key: u64) -> Squish {
        let Some(item) = self.items.iter().find(|item| item.key == key) else {
            return Squish::NONE;
        };
        let out = item.kick.value.abs();
        Squish {
            x: 1.0 - theme::NUDGE_KICK_SQUISH * out,
            y: 1.0 + theme::BULGE * theme::NUDGE_KICK_SQUISH * out,
        }
    }

    /// How far the whole island has moved: its share of the drag, and any nudge.
    fn offset(&self) -> Vector {
        Vector::new(
            self.x.value * theme::DRAG_FOLLOW + self.push_x.value,
            self.y.value * theme::DRAG_FOLLOW + self.push_y.value,
        )
    }

    /// The rest of the drag: how far it stretches the point `at` of the drawn `shape`, all the
    /// way at the grab and less further from it, easing to nothing at `DRAG_REACH`. Flat at both
    /// ends, so neither the grab point nor where the stretch fades out shows as a bend.
    fn pull(&self, shape: Rectangle, at: Point) -> Vector {
        let Some(grab) = self.grab else {
            return Vector::ZERO;
        };
        let distance = at.distance(shape.position() + grab);
        let near = hand::near(distance) * (1.0 - theme::DRAG_FOLLOW);
        Vector::new(self.x.value * near, self.y.value * near)
    }

    /// Whether a drag stretches the shape right now.
    fn pulled(&self) -> bool {
        self.grab.is_some() && (self.x.value != 0.0 || self.y.value != 0.0)
    }

    /// How far an item laid out at `x` is from where it's drawn, and how much it shows.
    fn item(&self, key: u64, x: f32) -> (f32, f32) {
        let fade = self.popover.items.value;
        match self.items.iter().find(|item| item.key == key) {
            Some(item) => (item.drawn() - x, item.shown.value * fade),
            // Not arranged yet: new, so not shown yet.
            None => (0.0, 0.0),
        }
    }
}

/// The plain arrow, as something other than `None`, which would let the pointer through to
/// what's under a popover. Not `Idle`: iced_layershell turns that into the busy cursor. It turns
/// any variant it doesn't name, like this one, into the default arrow.
const ARROW: mouse::Interaction = mouse::Interaction::ContextMenu;

/// The island itself, without the room an open popover keeps beside it.
fn own_bounds(state: &State, layout: Layout<'_>) -> Rectangle {
    let bounds = layout.bounds();
    let (offset, width) = state.own;
    Rectangle::new(
        Point::new(bounds.x + offset, bounds.y),
        Size::new(width, bounds.height),
    )
}

/// Whether the item at `index` is the toggle, laid out hidden.
fn is_hidden(toggle: Option<usize>, index: usize, shown: bool) -> bool {
    toggle == Some(index) && !shown
}

/// How wide each item's slot is: its own width, or its width group's widest.
fn slots(widths: &[f32], groups: &[Option<&str>]) -> Vec<f32> {
    let group = |index: usize| groups.get(index).copied().flatten();
    (0..widths.len())
        .map(|index| match group(index) {
            Some(name) => (0..widths.len())
                .filter(|other| group(*other) == Some(name))
                .map(|other| widths[other])
                .fold(0.0, f32::max),
            None => widths[index],
        })
        .collect()
}

/// How fast to kick a REACT spring at rest so it goes out `peak` at its furthest, with every
/// reaction's size (`REACT_SIZE`).
fn react(peak: f32) -> f32 {
    REACT.kick(peak * theme::REACT_SIZE)
}

/// How hard to push the skin for each pixel per second its edge speeds up (stretch) and slows
/// down (carry), so a side that comes `SKIN_TRAVEL` goes `SKIN_STRETCH` out and `SKIN_CARRY` on
/// at its furthest. Found by playing one such side on a spring, as [`Feel::kick`](crate::spring) does.
fn skin_gains() -> (f32, f32) {
    static GAINS: OnceLock<(f32, f32)> = OnceLock::new();
    *GAINS.get_or_init(|| {
        // Stretching, it eases in as an opening popover does; carrying on, it lands as anything
        // else does, hitting where it goes ([`Edges::aim`]).
        let ahead = |lead: bool| {
            let mut edges = Edges::at(Rectangle::new(Point::ORIGIN, Size::new(0.0, 1.0)));
            edges.aim(
                Rectangle::new(Point::ORIGIN, Size::new(theme::SKIN_TRAVEL, 1.0)),
                !lead,
            );
            let mut skin = Spring::new(0.0, REACT);
            let mut furthest = 0.0f32;
            let frame = 1.0 / 120.0;
            for _ in 0..240 {
                let was = edges.right.velocity;
                edges.right.advance(frame);
                edges.hit();
                let edge = edges.right;
                let change = edge.velocity - was;
                if (edge.velocity.abs() > was.abs()) == lead {
                    skin.velocity += if lead { change } else { -change };
                }
                skin.advance(frame);
                furthest = furthest.max(skin.value.abs());
            }
            furthest
        };
        (
            theme::SKIN_STRETCH * theme::REACT_SIZE / ahead(true),
            theme::SKIN_CARRY * theme::REACT_SIZE / ahead(false),
        )
    })
}

/// Where a popover goes: as tall as the panel and never narrower than the island, with its top
/// where the island's is. It grows from the island's left edge, right edge or center, and stays
/// `EDGE_GAP` clear of the surface's sides. With a notch, it stays on the island's side of it,
/// and may reach right up to it.
fn place(
    island: Rectangle,
    panel: Size,
    align: Horizontal,
    surface_width: f32,
    notch: Option<f32>,
) -> Rectangle {
    let padding = 2.0 * f32::from(theme::PANEL_PADDING);
    let width = (panel.width + padding).max(island.width).ceil();
    let height = (panel.height + padding).max(island.height).ceil();
    let x = match align {
        Horizontal::Left => island.x,
        Horizontal::Right => island.x + island.width - width,
        Horizontal::Center => island.center_x() - width / 2.0,
    };
    let gap = f32::from(theme::EDGE_GAP);
    let (mut from, mut to) = (gap, surface_width - gap);
    if let Some(notch) = notch {
        let middle = surface_width / 2.0;
        if island.center_x() < middle {
            to = to.min(middle - notch / 2.0);
        } else {
            from = from.max(middle + notch / 2.0);
        }
    }
    let x = x.min(to - width).max(from).round();
    Rectangle::new(Point::new(x, island.y), Size::new(width, height))
}

/// What's in `stops`, if there are any.
fn slice(stops: &Option<[Rectangle; 2]>) -> &[Rectangle] {
    stops.as_ref().map_or(&[], |stops| stops.as_slice())
}

/// The stop to head for next among `stops`, after `from`: the one after each the shape has been
/// most of the way to, counted in `via`. `None` once past them all.
fn pass(edges: &Edges, via: &mut usize, from: Rectangle, stops: &[Rectangle]) -> Option<Rectangle> {
    while let Some(&stop) = stops.get(*via) {
        let before = if *via == 0 { from } else { stops[*via - 1] };
        if edges.progress(before, stop) <= theme::DROP_WAYPOINT {
            return Some(stop);
        }
        *via += 1;
    }
    None
}

/// Where a [`Placement::Centered`] popover goes: as tall as the panel, in the middle of the
/// surface's width, with its top at `top`.
fn centered(panel: Size, surface_width: f32, top: f32) -> Rectangle {
    let padding = 2.0 * f32::from(theme::PANEL_PADDING);
    let size = Size::new(
        (panel.width + padding).ceil(),
        (panel.height + padding).ceil(),
    );
    let x = ((surface_width - size.width) / 2.0).round();
    Rectangle::new(Point::new(x, top.round()), size)
}

/// Where a [`Placement::Middle`] popover goes: in the middle of the surface's width, and of the
/// `room` down from its top, but never above `min_top`.
fn middle(panel: Size, surface_width: f32, room: f32, min_top: f32) -> Rectangle {
    let rect = centered(panel, surface_width, 0.0);
    let top = ((room - rect.height) / 2.0).round().max(min_top.round());
    Rectangle { y: top, ..rect }
}

/// Tells the host the room the panel needs, when that changed.
fn report<M>(motion: &mut Motion, popover: &Popover<'_, M>, shell: &mut Shell<'_, M>) {
    if let Some(reach) = motion.reach()
        && motion.popover.reported != Some(reach)
    {
        motion.popover.reported = Some(reach);
        shell.publish((popover.on_placed)(reach));
    }
}

/// What a key press does to an open popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyAction {
    /// Left unhandled, so the host closes the popover.
    Close,
    /// Kept: it was typed into a field.
    Keep,
}

/// Any key closes a popover, unless a text field in it has the focus. Esc always closes it, even
/// while typing (D-41). A modifier alone keeps it, since it may start a shortcut like Ctrl+K
/// (D-91).
fn key_policy(key: &keyboard::Key, field_focused: bool) -> KeyAction {
    use key::Named::{Alt, Control, Escape, Shift, Super};
    match key {
        keyboard::Key::Named(Escape) => KeyAction::Close,
        keyboard::Key::Named(Shift | Control | Alt | Super) => KeyAction::Keep,
        _ if field_focused => KeyAction::Keep,
        _ => KeyAction::Close,
    }
}

/// The panel's content, centered in its place.
fn panel_offset(place: Size, content: Size) -> Vector {
    Vector::new(
        ((place.width - content.width) / 2.0).round(),
        ((place.height - content.height) / 2.0).round(),
    )
}

/// The ink spring's target: 1 for darker ink.
fn ink_target(darker: bool) -> f32 {
    if darker { 1.0 } else { 0.0 }
}

/// Draws the island as it is right now: the shape, the items that show, and the popover panel if
/// one is given (with its layout).
#[allow(clippy::too_many_arguments)]
fn draw_island<M>(
    state: &State,
    keys: &[u64],
    items: &[Element<'_, M>],
    trees: &[Tree],
    layout: Layout<'_>,
    panel: Option<(&Element<'_, M>, &Tree, Layout<'_>)>,
    bleed: Padding,
    shoulders: Option<CornerShape>,
    band: Option<f32>,
    own_corners: [CornerShape; 4],
    shape_out: Option<&Shape>,
    glass: bool,
    renderer: &mut iced::Renderer,
    theme: &iced::Theme,
    style: &renderer::Style,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
) {
    use iced::advanced::Renderer as _;

    let motion = &state.motion;
    let shape = if motion.island.is_some() {
        motion.shape()
    } else {
        own_bounds(state, layout)
    };
    if let Some(out) = shape_out {
        out.set(shape + motion.offset());
    }
    let fill = motion.fill(Palette::of(theme).island_bg);
    // The morph's tongue, under the shape, and the fillet joining them (D-114).
    if motion.island.is_some()
        && let Some(tongue) = motion.tongue(shape)
    {
        let corner = motion.tongue_corner(tongue, shape);
        let pour = motion.tongue_pour(tongue, shape);
        let reach = pour.as_ref().map_or(Size::ZERO, pour::Pour::reach);
        let far = Size::new(
            (tongue.x + tongue.width + 1.0).max(reach.width),
            (tongue.y + tongue.height + 1.0).max(reach.height),
        );
        renderer.with_translation(motion.offset(), |renderer| {
            let mut frame = Frame::new(renderer, far);
            frame.fill(
                &squircle::squircle(tongue, corner.radius, corner.smoothing),
                fill,
            );
            if let Some(path) = pour.and_then(|pour| pour.path()) {
                frame.fill(&path, fill);
            }
            renderer.draw_geometry(frame.into_geometry());
        });
        // Glass where the shape is: the fillet is inside what the two reach together.
        let reach = tongue.union(&shape) + motion.offset();
        if glass
            && !motion
                .neck(shape + motion.offset())
                .is_some_and(|neck| neck.joined())
        {
            glass::frame::island(reach, fill, motion.ink.value);
        } else {
            glass::frame::opaque(reach);
        }
    }
    if shape.width < 1.0 || shape.height < 1.0 {
        return;
    }
    // Squeezed by the same few pixels whatever the island's width, about its center.
    let scale = (1.0 - 2.0 * motion.inset.value / shape.width).max(0.5);
    let center = shape.center();
    let transformation = Transformation::translate(center.x, center.y)
        * Transformation::scale(scale)
        * Transformation::translate(-center.x, -center.y);

    // The frame clips to its size: it has room all round for the jelly and the stretch, and for
    // the shoulders flowing out of a solid bar, which are as round as the island.
    let flare = band
        .and(shoulders)
        .map_or(0.0, |shoulder| shoulder.radius * (1.0 + shoulder.smoothing));
    let room = Vector::new(
        (theme::SKIN_ROOM + theme::DRAG_LIMIT_X).max(flare.ceil()),
        theme::SKIN_ROOM + theme::DRAG_LIMIT_Y,
    );
    let pulled = motion.pulled();
    let bent = !motion.jelly.is_flat() || pulled;
    if state.shape_bent.replace(bent) || bent {
        state.shape.clear();
    }
    // A drop is a ball, and rounds off into the panel's corners as it unfolds (D-86).
    let corners = match motion.drop {
        Some(_) => [motion.drop_corner(shape); 4],
        None => own_corners,
    };
    if state.shape_corners.replace(corners) != corners {
        state.shape.clear();
    }
    // Where the bar's bottom is on the shape moves as the shape does: a popover's grows down.
    let band_at = band.map(|band| band - shape.y);
    if state.shape_band.replace(band_at) != band_at {
        state.shape.clear();
    }
    if state.shape_fill.replace(fill) != fill {
        state.shape.clear();
    }
    // What the glass plugin hears of it: where it's its fill, glass, or never. A drop still
    // hanging from the notch is the notch's, black, until it lets go.
    let moved = shape + motion.offset();
    let hanging = motion.neck(moved).is_some_and(|neck| neck.joined());
    if glass && !hanging {
        glass::frame::island(moved, fill, motion.ink.value);
    } else {
        glass::frame::opaque(
            Rectangle::new(
                Point::new(shape.x - bleed.left, shape.y - bleed.top),
                Size::new(
                    shape.width + bleed.left + bleed.right,
                    shape.height + bleed.top + bleed.bottom,
                ),
            ) + motion.offset(),
        );
    }

    // What's drawn: the shape, reaching on to the edges it's attached to.
    let body = Rectangle::new(
        Point::new(shape.x - bleed.left, shape.y - bleed.top),
        Size::new(
            shape.width + bleed.left + bleed.right,
            shape.height + bleed.top + bleed.bottom,
        ),
    );
    // The neck, under the drop, flowing from what's above while they're close. Where the drop is
    // drawn, moved, but not from where it flows: that stays put.
    if let Some(neck) = motion.neck(moved)
        && let Some(path) = neck.path()
    {
        // From the surface's origin, as far as the neck reaches.
        let mut frame = Frame::new(renderer, neck.reach());
        frame.fill(&path, fill);
        renderer.draw_geometry(frame.into_geometry());
    }
    let offset = motion.offset();
    renderer.with_translation(offset, |renderer| {
        glass::frame::translated(offset, || {
            renderer.with_transformation(transformation, |renderer| {
                let frame_size = Size::new(body.width + 2.0 * room.x, body.height + 2.0 * room.y);
                let geometry = state.shape.draw(renderer, frame_size, |frame| {
                    let size = body.size();
                    let outline = Rectangle::new(Point::ORIGIN + room, size);
                    // The jelly bends the shape itself: its sides are where they'd be without the
                    // reach, and what reaches past one moves with it.
                    let jellied = Rectangle::new(
                        Point::ORIGIN + room + Vector::new(bleed.left, bleed.top),
                        shape.size(),
                    );
                    // Bent by the jelly and stretched by the drag while they move, both paths the
                    // same way.
                    let bend = |path: Path| {
                        if bent {
                            warp(&path, |at| {
                                let at = motion.jelly.displace(jellied, at);
                                if pulled {
                                    at + motion.pull(jellied, at)
                                } else {
                                    at
                                }
                            })
                        } else {
                            path
                        }
                    };
                    // Hanging from the top edge, where the surface starts, if it's attached there and
                    // has shoulders: on the sides that don't reach on, and no further than the frame.
                    // In a solid bar, flowing out of its bottom instead (D-89).
                    let hanging = shoulders.filter(|_| bleed.top > 0.0).map(|shoulder| {
                        let most = room.x / (1.0 + shoulder.smoothing);
                        let shoulder =
                            CornerShape::new(shoulder.radius.min(most), shoulder.smoothing);
                        let sides = [bleed.left <= 0.0, bleed.right <= 0.0];
                        (room.y + band.unwrap_or(0.0) - body.y, shoulder, sides)
                    });
                    // Black on black, with no border: what touches the notch or the bezel reads as
                    // one shape with it (D-74).
                    let outline = match hanging {
                        Some((edge, shoulder, sides)) => Path::new(|builder| {
                            if band.is_some() {
                                squircle::flowing(builder, outline, corners, edge, shoulder, sides);
                            } else {
                                squircle::hanging(builder, outline, corners, edge, shoulder, sides);
                            }
                        }),
                        None => squircle_corners(outline, corners),
                    };
                    frame.fill(&bend(outline), fill);
                });
                renderer.with_translation(Vector::new(body.x, body.y) - room, |renderer| {
                    renderer.draw_geometry(geometry);
                });

                // Nothing pokes out of the shape while it changes, but where the drag stretched it.
                let grab = shape.position() + motion.grab.unwrap_or_default();
                let clip = stretched(shape, motion.pull(shape, grab));
                renderer.with_layer(clip, |renderer| {
                    for ((key, item), (tree, child)) in keys
                        .iter()
                        .zip(items)
                        .zip(trees.iter().zip(layout.children()))
                    {
                        let bounds = child.bounds();
                        let (dx, shown) = motion.item(*key, bounds.x);
                        if shown < REST_DISTANCE {
                            continue;
                        }
                        let (squished, down) = motion.squish_at(shape, bounds.center_x() + dx);
                        let kicked = motion.kicked(*key);
                        let squished = Squish {
                            x: squished.x * kicked.x,
                            y: squished.y * kicked.y,
                        };
                        let moved = Vector::new(dx, down);
                        // It rides along with the skin where it is, and squishes with it.
                        // And it goes with the stretch where it ends up.
                        let moved = moved + motion.pull(shape, bounds.center() + moved);
                        renderer.with_translation(moved, |renderer| {
                            with_clip(clip - moved, || {
                                squish::with(squished, || {
                                    glass::frame::translated(moved, || {
                                        item.as_widget().draw(
                                            tree, renderer, theme, style, child, cursor, viewport,
                                        );
                                    });
                                });
                            });
                            // In the item's moved coordinates, the shape is the other way.
                            fade(renderer, theme, bounds, shape - moved, shown);
                        });
                    }
                    if let Some((panel, tree, layout)) = panel {
                        let shown = motion.popover.panel.value;
                        // The panel's viewport is what of it the shape shows, so a scrolling list or a
                        // text field clips to the shape while it grows.
                        if shown >= REST_DISTANCE
                            && let Some(visible) = layout.bounds().intersection(&shape)
                        {
                            let moved = motion.pull(shape, layout.bounds().center());
                            renderer.with_translation(moved, |renderer| {
                                glass::frame::translated(moved, || {
                                    panel.as_widget().draw(
                                        tree,
                                        renderer,
                                        theme,
                                        style,
                                        layout,
                                        cursor,
                                        &(visible - moved),
                                    );
                                });
                                fade(renderer, theme, layout.bounds(), shape - moved, shown);
                            });
                        }
                    }
                });
            });
        })
    });
}

thread_local! {
    static CLIP: Cell<Option<Rectangle>> = const { Cell::new(None) };
}

/// What the island being drawn shows of its items, in the coordinates they're drawn in. Something
/// that draws in a layer of its own (a count badge, over its content) keeps to it: a nested layer
/// isn't clipped by the one around it. Ambient like [`squish::current`], for the same reason.
pub(crate) fn clip() -> Option<Rectangle> {
    CLIP.with(Cell::get)
}

fn with_clip<R>(clip: Rectangle, draw: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rectangle>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CLIP.with(|current| current.set(self.0));
        }
    }
    let _restore = Restore(CLIP.with(|current| current.replace(Some(clip))));
    draw()
}

/// `shape`, grown by `pull` the way it points, to take in what the drag stretched out of it.
fn stretched(shape: Rectangle, pull: Vector) -> Rectangle {
    Rectangle::new(
        Point::new(shape.x + pull.x.min(0.0), shape.y + pull.y.min(0.0)),
        Size::new(shape.width + pull.x.abs(), shape.height + pull.y.abs()),
    )
}

/// Fades what was just drawn in `bounds` toward the island's background. iced has no group
/// opacity, but the island is opaque, so covering content with its own color is the same thing.
/// Kept inside `shape`: a nested layer isn't clipped by the one around it.
pub(crate) fn fade(
    renderer: &mut iced::Renderer,
    theme: &iced::Theme,
    bounds: Rectangle,
    shape: Rectangle,
    shown: f32,
) {
    use iced::advanced::Renderer as _;

    let Some(bounds) = bounds
        .intersection(&shape)
        .filter(|_| shown <= 1.0 - REST_DISTANCE)
    else {
        return;
    };
    // A layer of its own, so it lands over the text, which draws after quads within a layer.
    renderer.with_layer(bounds, |renderer| {
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            Color {
                a: 1.0 - shown,
                ..Palette::of(theme).island_bg
            },
        );
    });
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Island<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            motion: Motion {
                ink: Spring::new(ink_target(self.darker), FADE),
                ..Motion::default()
            },
            shape: Cache::new(),
            shape_bent: Cell::new(false),
            shape_corners: Cell::new([CornerShape::SQUARE; 4]),
            shape_band: Cell::new(None),
            shape_fill: Cell::new(Color::TRANSPARENT),
            panel: None,
            hovered: false,
            toggle_shown: false,
            expanded: None,
            own: (0.0, 0.0),
            live: false,
            shoulders: None,
            corners: [CornerShape::SQUARE; 4],
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Fixed(self.height))
    }

    /// A row of items, centered vertically, with padding at the ends. Items in a width group get
    /// a slot as wide as the widest of them, and sit in its middle.
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let height = self.height;
        let spacing = theme::ITEM_SPACING as f32;
        let limits = layout::Limits::new(Size::new(0.0, height), Size::new(f32::INFINITY, height));
        let state = tree.state.downcast_mut::<State>();
        if state.live != self.live
            || state.shoulders != self.shoulders
            || state.corners != self.corners
        {
            state.live = self.live;
            state.shoulders = self.shoulders;
            state.corners = self.corners;
            state.shape.clear();
        }
        state.toggle_shown = (state.hovered || self.expanded) && self.popover.is_none();
        // A hidden toggle takes no room.
        let hidden = (!state.toggle_shown).then_some(self.toggle).flatten();
        let nodes: Vec<_> = self
            .items
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(item, tree)| item.as_widget_mut().layout(tree, renderer, &limits))
            .collect();
        let widths: Vec<_> = nodes.iter().map(|node| node.size().width).collect();
        // A hidden toggle and the items going back into it take no room (D-116).
        let roomless: Vec<_> = self
            .keys
            .iter()
            .enumerate()
            .map(|(index, key)| Some(index) == hidden || self.leaving.contains(key))
            .collect();
        let laid_out = roomless.iter().filter(|roomless| !**roomless).count();
        // A live island has as much room at its ends as above and below its tallest item, in
        // what's seen of it.
        let tallest = nodes
            .iter()
            .enumerate()
            .filter(|&(index, _)| !roomless[index])
            .map(|(_, node)| node.size().height)
            .reduce(f32::max);
        let padding = match (self.live, tallest) {
            (false, _) => f32::from(theme::ISLAND_PADDING),
            (true, Some(tallest)) => {
                let shown = height + self.shows.top + self.shows.bottom;
                ((shown - tallest) / 2.0).round().max(0.0)
            }
            (true, None) => f32::from(theme::LIVE_ISLAND_PADDING),
        };
        let groups: Vec<_> = self
            .groups
            .iter()
            .zip(&roomless)
            .map(|(group, roomless)| group.filter(|_| !roomless))
            .collect();
        let slots = slots(&widths, &groups);
        let mut x = padding;
        let children: Vec<_> = nodes
            .into_iter()
            .zip(slots)
            .enumerate()
            .map(|(index, (node, slot))| {
                let size = node.size();
                if roomless[index] {
                    return node.move_to(Point::new(x, ((height - size.height) / 2.0).round()));
                }
                let at = Point::new(
                    (x + (slot - size.width) / 2.0).round(),
                    ((height - size.height) / 2.0).round(),
                );
                x += slot + spacing;
                node.move_to(at)
            })
            .collect();
        let width = if laid_out == 0 {
            2.0 * padding
        } else {
            x - spacing + padding
        };
        // A live island is never narrower than round.
        let width = if self.live { width.max(height) } else { width };
        // An open popover takes the panel's width in the row, so it pushes the islands beside it
        // away instead of covering them. They glide there and back on their own springs. The
        // island itself stays where its popover grows from: at the zone's side, or centered. One
        // in the middle of the surface, a dialog, is clear of them.
        let panel = self
            .popover
            .as_ref()
            .filter(|popover| popover.open && popover.placement == Placement::Below)
            .map(|popover| popover.width);
        let room = match panel {
            Some(panel) => width.max(panel),
            None => width,
        };
        let offset = match self.align {
            Horizontal::Left => 0.0,
            Horizontal::Center => ((room - width) / 2.0).round(),
            Horizontal::Right => room - width,
        };
        tree.state.downcast_mut::<State>().own = (offset, width);
        // In the middle of what's seen of it, which an edge or the notch moves off its layout's.
        let centered = Vector::new(
            ((self.shows.right - self.shows.left) / 2.0).round(),
            ((self.shows.bottom - self.shows.top) / 2.0).round(),
        );
        let children = children
            .into_iter()
            .map(|child| child.translate(Vector::new(offset, 0.0) + centered))
            .collect();
        layout::Node::with_children(Size::new(room, height), children)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // A popover's overlay draws the whole island, over everything else.
        if self.popover.is_some() {
            return;
        }
        draw_island(
            tree.state.downcast_ref::<State>(),
            &self.keys,
            &self.items,
            &tree.children,
            layout,
            None,
            self.bleed,
            self.shoulders,
            self.band,
            self.corners,
            self.shape.as_ref(),
            self.glass,
            renderer,
            theme,
            style,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        self.items.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.items);
        let state = tree.state.downcast_mut::<State>();
        state.motion.ink.target = ink_target(self.darker);
        let panel = &mut state.panel;
        match (&self.popover, panel.as_mut()) {
            (Some(popover), Some(tree)) => tree.diff(&popover.content),
            (Some(popover), None) => *panel = Some(Tree::new(&popover.content)),
            (None, _) => *panel = None,
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((item, tree), layout) in self
            .items
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            item.as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let shown = tree.state.downcast_ref::<State>().toggle_shown;
        // Items under a popover are hidden, and stay out of it.
        if self.popover.is_none() {
            for (index, ((item, tree), layout)) in self
                .items
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .enumerate()
            {
                if is_hidden(self.toggle, index, shown) || self.leaving.contains(&self.keys[index])
                {
                    continue;
                }
                item.as_widget_mut().update(
                    tree, event, layout, cursor, renderer, clipboard, shell, viewport,
                );
            }
        }

        let state = tree.state.downcast_mut::<State>();
        // Hovering shows the toggle, and leaving hides it: the island makes room for it, or gives
        // the room back. No message: it's the island's own.
        if self.toggle.is_some()
            && let Event::Mouse(
                mouse::Event::CursorMoved { .. }
                | mouse::Event::CursorEntered
                | mouse::Event::CursorLeft,
            ) = event
        {
            let hovered = !matches!(event, Event::Mouse(mouse::Event::CursorLeft))
                && cursor.is_over(own_bounds(state, layout));
            if hovered != state.hovered {
                state.hovered = hovered;
                shell.invalidate_layout();
                shell.request_redraw();
            }
        }
        let own = own_bounds(state, layout);
        // Showing or hiding the collapsible items squashes the island the way they went: out
        // toward the toggle, or back.
        let expanded = self.toggle.map(|_| self.expanded);
        let squash = state.expanded.is_some() && expanded != state.expanded;
        state.expanded = expanded;
        let motion = &mut state.motion;
        motion.opening |= squash && self.expanded;
        if squash && motion.press.is_none() && !motion.popover.present {
            let out = if self.toggle_side == Horizontal::Left {
                Nudge::Left
            } else {
                Nudge::Right
            };
            let back = if out == Nudge::Left {
                Nudge::Right
            } else {
                Nudge::Left
            };
            let nudge = if self.expanded { out } else { back };
            let request = NudgeRequest {
                nudge,
                lasting: Some(theme::COLLAPSE_SQUASH),
            };
            motion.nudge(request, None, Instant::now());
        }
        let taken = motion.swap(self.popover.as_ref().map(|popover| popover.key));
        motion.drop = self.popover.as_ref().and_then(|popover| popover.drop);
        motion.drop_color = self.popover.as_ref().and_then(|popover| popover.drop_color);
        motion.home = self
            .popover
            .as_ref()
            .and_then(|popover| popover.home.clone());
        motion.corner = self.corners[0];
        motion.sag = self
            .popover
            .as_ref()
            .and_then(|popover| popover.sag.clone());
        if taken {
            motion.rehome();
        }
        motion.from = self.toggle_side;
        motion.leaving.clone_from(&self.leaving);
        // A hidden toggle isn't arranged, so it isn't drawn, and fades in when it shows.
        let positions = self
            .keys
            .iter()
            .zip(layout.children())
            .enumerate()
            .filter(|(index, _)| !is_hidden(self.toggle, *index, shown))
            .map(|(_, (key, child))| (*key, child.bounds().x, child.bounds().width));
        motion.place(
            own,
            positions,
            self.popover.as_ref().map(|popover| popover.open),
        );
        motion.wall(self.walls, viewport.width, self.corners);
        motion.nudges(
            self.nudges.iter().copied(),
            |key| {
                self.keys
                    .iter()
                    .zip(layout.children())
                    .find(|(k, _)| **k == key)
                    .map(|(_, child)| (child.bounds().x, child.bounds().width))
            },
            Instant::now(),
        );

        match event {
            // Watched whether or not an item handled it: the island reacts to every press on it.
            Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left | mouse::Button::Right,
            )) if self.popover.is_none() => {
                if let Some(at) = cursor.position_over(own) {
                    motion.press(at);
                }
            }
            // While the button is held, the compositor keeps sending motion even off the surface.
            Event::Mouse(mouse::Event::CursorMoved { position }) if motion.press.is_some() => {
                motion.drag(*position);
            }
            Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left | mouse::Button::Right,
            ))
            | Event::Mouse(mouse::Event::CursorLeft)
                if motion.press.is_some() =>
            {
                motion.release();
            }
            // Every frame starts with this event.
            Event::Window(window::Event::RedrawRequested(now)) => {
                motion.step(*now);
            }
            _ => {}
        }
        if let Some(popover) = &self.popover {
            // After stepping: the frame the shape lands on is the last, and gives back its room.
            report(motion, popover, shell);
            if let Some(joined) = motion.popover.touched.take()
                && let Some(on_neck) = &popover.on_neck
            {
                shell.publish(on_neck(joined));
            }
            if motion.closing_done() {
                shell.publish(popover.on_closed.clone());
            }
        }
        if motion.gone()
            && let Some(on_gone) = &self.on_gone
        {
            shell.publish(on_gone.clone());
        }
        if !motion.at_rest() {
            shell.request_redraw();
        } else if let Some(wake) = motion.wake() {
            shell.request_redraw_at(wake);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.popover.is_some() {
            return mouse::Interaction::None;
        }
        let shown = tree.state.downcast_ref::<State>().toggle_shown;
        self.items
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
            .filter(|(index, _)| {
                !is_hidden(self.toggle, *index, shown) && !self.leaving.contains(&self.keys[*index])
            })
            .map(|(_, ((item, tree), layout))| {
                item.as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        if self.popover.is_none() {
            return overlay::from_children(
                &mut self.items,
                tree,
                layout,
                renderer,
                viewport,
                translation,
            );
        }
        Some(overlay::Element::new(Box::new(Morph {
            island: self,
            tree,
            layout,
            translation,
        })))
    }
}

impl<'a, M: Clone + 'a> From<Island<'a, M>> for Element<'a, M> {
    fn from(island: Island<'a, M>) -> Self {
        Element::new(island)
    }
}

/// The island while it has a popover: drawn whole, panel included, above everything else.
struct Morph<'a, 'b, M> {
    island: &'b mut Island<'a, M>,
    tree: &'b mut Tree,
    /// The island's own layout in the bar.
    layout: Layout<'b>,
    translation: Vector,
}

impl<M: Clone> overlay::Overlay<M, iced::Theme, iced::Renderer> for Morph<'_, '_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let popover = self.island.popover.as_mut().expect("a morph has a popover");
        let state = self.tree.state.downcast_mut::<State>();
        let panel = state
            .panel
            .get_or_insert_with(|| Tree::new(&popover.content));
        // Every panel is as wide as `PANEL_WIDTH` (or `PANEL_WIDE_WIDTH`, if it asked), padding
        // included, so rows can stretch to its edge (D-40). Its height is its own: unbounded and
        // compressed, so `Fill` children don't stretch to infinity.
        let width = popover.width - 2.0 * f32::from(theme::PANEL_PADDING);
        let limits = layout::Limits::with_compression(
            Size::new(width, 0.0),
            Size::new(width, f32::INFINITY),
            Size::new(false, true),
        );
        let content = popover
            .content
            .as_widget_mut()
            .layout(panel, renderer, &limits);

        let island = own_bounds(state, self.layout) + self.translation;
        let rect = match popover.placement {
            Placement::Below => place(
                island,
                content.size(),
                self.island.align,
                bounds.width,
                self.island.notch,
            ),
            Placement::Centered { top } => centered(content.size(), bounds.width, top),
            Placement::Middle { room, min_top } => {
                middle(content.size(), bounds.width, room, min_top)
            }
        };
        state
            .motion
            .place_panel(rect, rect.y + rect.height <= bounds.height);

        let offset = panel_offset(rect.size(), content.size());
        let content = content.move_to(Point::ORIGIN + offset);
        layout::Node::with_children(rect.size(), vec![content]).move_to(rect.position())
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let island = &*self.island;
        let state = self.tree.state.downcast_ref::<State>();
        let (Some(popover), Some(panel)) = (&island.popover, &state.panel) else {
            return;
        };
        let content = layout.children().next().expect("a morph has its panel");
        draw_island(
            state,
            &island.keys,
            &island.items,
            &self.tree.children,
            self.layout,
            Some((&popover.content, panel, content)),
            island.bleed,
            island.shoulders,
            island.band,
            island.corners,
            island.shape.as_ref(),
            island.glass,
            renderer,
            theme,
            style,
            cursor,
            &layout.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let popover = self.island.popover.as_mut().expect("a morph has a popover");
        let state = self.tree.state.downcast_mut::<State>();
        let rect = layout.bounds();
        report(&mut state.motion, popover, shell);
        let Some(panel) = &mut state.panel else {
            return;
        };
        let content = layout.children().next().expect("a morph has its panel");
        // Keys go to the panel even while it fades in, so typing isn't lost; Esc never does, so
        // it always closes the popover (D-41).
        let (key, modifiers) = match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                (Some(key), *modifiers)
            }
            _ => (None, keyboard::Modifiers::empty()),
        };
        let shown = state.motion.popover.panel.target > 0.0;
        let escape = key.is_some_and(|key| *key == keyboard::Key::Named(key::Named::Escape));
        let forward = popover.open && !escape && (shown || matches!(event, Event::Keyboard(_)));
        if forward {
            popover.content.as_widget_mut().update(
                panel, event, content, cursor, renderer, clipboard, shell, &rect,
            );
        }
        if let Some(key) = key
            && popover.open
        {
            let mut focused = AnyFocused::default();
            popover
                .content
                .as_widget_mut()
                .operate(panel, content, renderer, &mut focused);
            if let Some(message) = popover
                .on_key
                .as_ref()
                .and_then(|on_key| on_key(key, modifiers, focused.found))
            {
                shell.publish(message);
                shell.capture_event();
            } else if key_policy(key, focused.found) == KeyAction::Keep {
                shell.capture_event();
            }
        }
        // A press the panel's content left alone grabs the popover, like a press on an island
        // does. One on a control doesn't, so a slider doesn't stretch the panel too. The drag is
        // followed here as well: what the overlay captures never reaches the island.
        let motion = &mut state.motion;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if popover.open && !shell.is_event_captured() =>
            {
                if let Some(at) = cursor.position_over(motion.edges.rect()) {
                    motion.press(at);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if motion.press.is_some() => {
                motion.drag(*position);
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Mouse(mouse::Event::CursorLeft)
                if motion.press.is_some() =>
            {
                motion.release();
                shell.request_redraw();
            }
            _ => {}
        }
        if let Some(on_hover) = &popover.on_hover {
            let hovered = !matches!(event, Event::Mouse(mouse::Event::CursorLeft))
                && cursor.is_over(motion.edges.rect());
            if hovered != motion.popover.hovered {
                motion.popover.hovered = hovered;
                shell.publish(on_hover(hovered));
            }
        }
        // A press anywhere on the shape is the popover's, handled or not: it isn't a click on the
        // bar, which would close it.
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && cursor.is_over(state.motion.edges.rect())
        {
            shell.capture_event();
        }
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = self.tree.state.downcast_mut::<State>();
        if let (Some(popover), Some(panel), Some(content)) = (
            self.island.popover.as_mut(),
            state.panel.as_mut(),
            layout.children().next(),
        ) {
            popover
                .content
                .as_widget_mut()
                .operate(panel, content, renderer, operation);
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = self.tree.state.downcast_ref::<State>();
        if !cursor.is_over(state.motion.edges.rect()) {
            return mouse::Interaction::None;
        }
        let inner = match (&self.island.popover, &state.panel) {
            (Some(popover), Some(panel)) if popover.open => {
                let content = layout.children().next().expect("a morph has its panel");
                popover.content.as_widget().mouse_interaction(
                    panel,
                    content,
                    cursor,
                    &layout.bounds(),
                    renderer,
                )
            }
            _ => mouse::Interaction::None,
        };
        // Anything but `None`, so whatever is under the shape doesn't see the pointer.
        if inner == mouse::Interaction::None {
            ARROW
        } else {
            inner
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    /// Steps frame by frame until the motion asks for no more, waking when it asks to be woken;
    /// returns the frames taken.
    fn run(motion: &mut Motion, start: Instant, mut each: impl FnMut(&Motion)) -> usize {
        let mut now = start;
        for frame in 0..1000 {
            if !motion.step(now) {
                match motion.wake() {
                    Some(wake) => {
                        now = now.max(wake);
                        continue;
                    }
                    None => return frame,
                }
            }
            each(motion);
            now += FRAME;
        }
        panic!("never came to rest");
    }

    fn rect(x: f32, width: f32) -> Rectangle {
        Rectangle::new(Point::new(x, 6.0), Size::new(width, 32.0))
    }

    /// An island already laid out and at rest at `island`, with items `keys` at 10, 50, 90…
    fn settled(island: Rectangle, keys: &[u64]) -> Motion {
        let mut motion = Motion::default();
        let positions = keys
            .iter()
            .zip(0..)
            .map(|(key, i)| (*key, 10.0 + 40.0 * i as f32));
        motion.arrange(island, positions, None);
        run(&mut motion, Instant::now(), |_| {});
        motion
    }

    fn find(motion: &Motion, key: u64) -> &ItemMotion {
        motion.items.iter().find(|item| item.key == key).unwrap()
    }

    #[test]
    fn items_shown_together_come_out_as_the_toggle_passes_them() {
        let mut motion = Motion::default();
        let before = [(1, 10.0, 30.0), (TOGGLE, 50.0, 20.0)];
        motion.place(rect(0.0, 80.0), before.into_iter(), None);
        run(&mut motion, Instant::now(), |_| {});

        // Shown: the toggle on the right slides out, and the island grows with it.
        motion.opening = true;
        let after = [
            (1, 10.0, 30.0),
            (2, 50.0, 30.0),
            (3, 90.0, 30.0),
            (TOGGLE, 130.0, 20.0),
        ];
        motion.place(rect(0.0, 160.0), after.into_iter(), None);
        assert!(!motion.at_rest(), "waiting is moving");
        let mut shown_at = [None; 2];
        let mut frame = 0;
        run(&mut motion, Instant::now(), |motion| {
            frame += 1;
            for (index, key) in [2, 3].into_iter().enumerate() {
                let item = find(motion, key);
                if shown_at[index].is_none() && item.shown.target > 0.0 {
                    shown_at[index] = Some(frame);
                    let toggle = find(motion, TOGGLE).drawn();
                    assert!(
                        toggle >= item.x.value + item.width - OVERLAP,
                        "{key} waits for the toggle to pass it: {toggle}"
                    );
                }
            }
        });
        let [Some(near), Some(far)] = shown_at else {
            panic!("both came out: {shown_at:?}");
        };
        assert!(near < far, "the nearer first: {near} then {far}");
        assert_eq!(motion.item(3, 90.0), (0.0, 1.0));
    }

    #[test]
    fn items_going_close_with_the_island_and_say_when_they_are_gone() {
        let mut motion = Motion::default();
        let all = [
            (1, 10.0, 30.0),
            (2, 50.0, 30.0),
            (3, 90.0, 30.0),
            (TOGGLE, 130.0, 20.0),
        ];
        motion.place(rect(0.0, 160.0), all.into_iter(), None);
        run(&mut motion, Instant::now(), |_| {});

        // Hidden: they take no room, so the island closes at once, and they stay put.
        motion.leaving = vec![2, 3];
        let closed = [
            (1, 10.0, 30.0),
            (2, 50.0, 30.0),
            (3, 50.0, 30.0),
            (TOGGLE, 50.0, 20.0),
        ];
        motion.place(rect(0.0, 80.0), closed.into_iter(), None);
        assert_eq!(motion.edges.goal[2], 80.0);
        assert_eq!(find(&motion, 3).x.target, 90.0, "it stays where it was");
        assert!(!motion.gone());
        let mut hidden = Vec::new();
        run(&mut motion, Instant::now(), |motion| {
            for key in [2, 3] {
                if find(motion, key).cascade == Some(Cascade::Gone) && !hidden.contains(&key) {
                    hidden.push(key);
                }
            }
        });
        assert_eq!(hidden, [3, 2], "the toggle reaches the nearer first");
        assert_eq!(motion.item(2, 50.0).1, 0.0);
        assert_eq!(motion.item(1, 10.0), (0.0, 1.0), "the rest stay");
        assert!(motion.gone());
        assert!(!motion.gone(), "said once");

        // Called back on their way out: they come back.
        motion.leaving.clear();
        motion.place(rect(0.0, 160.0), all.into_iter(), None);
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.item(3, 90.0), (0.0, 1.0));
    }

    #[test]
    fn at_rest_asks_for_no_frames() {
        let mut motion = Motion::default();
        assert!(!motion.step(Instant::now()));
        assert_eq!(motion.last, None);
    }

    #[test]
    fn a_new_island_grows_from_its_center() {
        let mut motion = Motion::default();
        motion.arrange(rect(100.0, 200.0), [(1, 110.0)].into_iter(), None);
        let first = motion.edges.rect();
        assert_eq!(first.width, 0.0);
        assert_eq!(first.center_x(), 200.0);
        let mut wider = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            let width = motion.edges.rect().width;
            assert!(width >= wider - 1.0, "only grows: {width} after {wider}");
            wider = width;
        });
        assert_eq!(motion.edges.rect(), rect(100.0, 200.0));
        assert_eq!(motion.item(1, 110.0), (0.0, 1.0));
    }

    #[test]
    fn a_change_of_size_morphs_without_wobbling() {
        let mut motion = settled(rect(100.0, 200.0), &[1]);
        motion.arrange(rect(100.0, 300.0), [(1, 110.0)].into_iter(), None);
        assert_eq!(motion.edges.rect(), rect(100.0, 200.0), "doesn't snap");
        let mut widest: f32 = 0.0;
        let frames = run(&mut motion, Instant::now(), |motion| {
            widest = widest.max(motion.edges.rect().width);
        });
        assert!(frames > 5, "takes a moment: {frames} frames");
        assert!(widest < 301.0, "no overshoot past a pixel: {widest}");
        assert_eq!(motion.edges.rect(), rect(100.0, 300.0));
    }

    #[test]
    fn items_are_followed_by_key() {
        let mut motion = settled(rect(0.0, 200.0), &[1, 2, 3]);
        // 1 goes, 4 comes in between 2 and 3, which both move.
        motion.arrange(
            rect(0.0, 200.0),
            [(2, 10.0), (4, 50.0), (3, 90.0)].into_iter(),
            None,
        );
        assert_eq!(motion.item(2, 10.0), (40.0, 1.0), "2 slides from 50");
        assert_eq!(
            motion.item(4, 50.0),
            (0.0, 0.0),
            "4 starts hidden, in place"
        );
        assert_eq!(motion.item(3, 90.0), (0.0, 1.0), "3 stays");
        assert!(motion.items.iter().all(|item| item.key != 1));
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.item(2, 10.0), (0.0, 1.0));
        assert_eq!(motion.item(4, 50.0), (0.0, 1.0));
    }

    fn popover(motion: &mut Motion, open: bool, panel: Rectangle, fits: bool) {
        let island = motion.island.unwrap();
        motion.arrange(island, [(1, 10.0)].into_iter(), Some(open));
        motion.place_panel(panel, fits);
        motion.aim();
    }

    #[test]
    fn a_drop_falls_as_a_ball_on_a_neck_then_unfolds_and_comes_back_the_same_way() {
        let island = rect(100.0, 120.0);
        let mut motion = Motion {
            drop: Some(40.0),
            ..Motion::default()
        };
        motion.arrange(island, [(1, 110.0)].into_iter(), None);
        run(&mut motion, Instant::now(), |_| {});
        let home = Rectangle::new(Point::new(160.0, 40.0), Size::ZERO);
        assert_eq!(
            motion.edges.rect(),
            home,
            "at rest, a dot where it comes out"
        );

        let panel = Rectangle::new(Point::new(10.0, 200.0), Size::new(300.0, 200.0));
        popover(&mut motion, true, panel, true);
        let (mut joined, mut let_go, mut content_early) = (false, false, false);
        let mut bounced: f32 = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            let neck = motion.neck(motion.shape() + motion.offset());
            let now = neck.as_ref().is_some_and(|neck| neck.joined());
            let_go |= joined && !now;
            joined |= now;
            bounced = bounced.max(motion.push_y.value);
            content_early |= motion.popover.via < 2 && motion.popover.panel.value > 0.01;
        });
        assert!(joined, "it hangs on a neck on its way out");
        assert!(let_go, "the neck lets go of it");
        assert!(bounced > 1.0, "it lurches down: {bounced}");
        assert!(!content_early, "the content waits for the ball to unfold");
        assert_eq!(motion.edges.rect(), panel);
        assert_eq!(
            motion.roundness(),
            0.0,
            "unfolded, it has the panel's corners"
        );
        assert!(motion.neck(motion.shape()).is_none(), "far from the notch");

        popover(&mut motion, false, panel, true);
        let (mut round_again, mut caught) = (false, false);
        let mut lifted: f32 = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            round_again |= motion.roundness() > 0.99 && motion.edges.rect().width > 20.0;
            caught |= motion.popover.joined;
            lifted = lifted.min(motion.push_y.value);
        });
        assert!(round_again, "it folds back into a ball");
        assert!(caught, "the neck catches it again");
        assert!(lifted < -1.0, "and pulls it up: {lifted}");
        assert_eq!(
            motion.popover.touched,
            Some(true),
            "it says when it's caught"
        );
        assert_eq!(motion.edges.rect(), home);
        assert!(motion.closing_done());
    }

    #[test]
    fn a_middle_popover_is_centered_on_the_screen_but_never_over_the_bar() {
        let panel = Size::new(600.0, 400.0);
        let padding = 2.0 * f32::from(theme::PANEL_PADDING);
        let rect = middle(panel, 1000.0, 1000.0, 40.0);
        assert_eq!(rect.x, ((1000.0 - 600.0 - padding) / 2.0).round());
        assert_eq!(rect.y, ((1000.0 - 400.0 - padding) / 2.0).round());
        assert_eq!(middle(panel, 1000.0, 300.0, 40.0).y, 40.0);
    }

    #[test]
    fn a_popover_with_a_home_comes_out_of_it_and_rests_there() {
        let home = Shape::default();
        home.set(rect(300.0, 80.0));
        let mut motion = Motion {
            home: Some(home.clone()),
            ..Motion::default()
        };
        motion.arrange(rect(0.0, 20.0), [(1, 10.0)].into_iter(), None);
        assert_eq!(motion.edges.rect(), rect(300.0, 80.0));
        // It moved: home follows it.
        home.set(rect(500.0, 90.0));
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.edges.rect(), rect(500.0, 90.0));
    }

    #[test]
    fn a_drop_follows_the_bottom_it_comes_out_of_as_that_squishes() {
        let sag = Sag::default();
        let mut motion = Motion {
            drop: Some(40.0),
            sag: Some(sag.clone()),
            ..Motion::default()
        };
        motion.arrange(rect(100.0, 120.0), [(1, 110.0)].into_iter(), None);
        assert_eq!(motion.home(rect(100.0, 120.0)).y, 40.0);
        // The notch flattens 3 px: home, and where the neck flows from, go up with it.
        sag.set(-3.0);
        assert_eq!(motion.home(rect(100.0, 120.0)).y, 37.0);
        let ball = Rectangle::new(Point::new(147.0, 38.0), Size::new(26.0, 26.0));
        assert!(motion.neck(ball).is_some_and(|neck| neck.joined()));
    }

    /// Plays the motion to rest, and returns how far the tongue got out of the shape each way
    /// (left, top, right, bottom) at the most, checking its fillet is somewhere.
    fn tongue_range(motion: &mut Motion) -> [f32; 4] {
        let mut most = [0.0f32; 4];
        run(motion, Instant::now(), |motion| {
            let shape = motion.shape();
            let Some(tongue) = motion.tongue(shape) else {
                return;
            };
            let out = [
                shape.x - tongue.x,
                shape.y - tongue.y,
                tongue.x + tongue.width - (shape.x + shape.width),
                tongue.y + tongue.height - (shape.y + shape.height),
            ];
            for side in 0..4 {
                most[side] = most[side].max(out[side]);
            }
            if let Some(path) = motion
                .tongue_pour(tongue, shape)
                .and_then(|pour| pour.path())
            {
                assert!(
                    path.raw()
                        .iter()
                        .flat_map(|event| [event.from(), event.to()])
                        .all(|at| at.x.is_finite() && at.y.is_finite()),
                    "a finite fillet"
                );
            }
        });
        assert!(motion.tongue(motion.shape()).is_none(), "none at rest");
        most
    }

    #[test]
    fn a_popover_pours_down_ahead_and_drains_back_up_behind() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 300.0));
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        let out = tongue_range(&mut motion);
        assert!(out[3] > 20.0, "runs on ahead, down: {out:?}");
        assert!(
            out[0] < 0.5 && out[1] < 0.5 && out[2] < 0.5,
            "only down: {out:?}"
        );
        assert_eq!(motion.shape(), panel);

        popover(&mut motion, false, panel, true);
        let out = tongue_range(&mut motion);
        assert!(out[3] > 20.0, "hangs on behind, under: {out:?}");
        assert_eq!(motion.shape(), island);
    }

    #[test]
    fn an_island_growing_sideways_runs_on_that_way_and_one_sliding_doesnt() {
        let island = rect(100.0, 120.0);
        let mut motion = settled(island, &[1]);
        motion.arrange(rect(100.0, 260.0), [(1, 110.0)].into_iter(), None);
        let out = tongue_range(&mut motion);
        assert!(out[2] > 5.0, "runs on ahead, right: {out:?}");
        motion.arrange(rect(400.0, 260.0), [(1, 410.0)].into_iter(), None);
        let out = tongue_range(&mut motion);
        assert!(out.iter().all(|out| *out < 0.5), "sliding: {out:?}");
    }

    #[test]
    fn a_popover_waits_for_room_then_morphs_and_closes_back() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let mut motion = settled(island, &[1]);

        // The surface hasn't grown yet: nothing moves.
        popover(&mut motion, true, panel, false);
        assert!(!motion.step(Instant::now()));
        assert_eq!(motion.edges.rect(), island);

        popover(&mut motion, true, panel, true);
        let mut panel_before_halfway = false;
        let mut panel_while_items = false;
        let frames = run(&mut motion, Instant::now(), |motion| {
            if motion.edges.progress(island, panel) < 0.4 && motion.popover.panel.value > 0.01 {
                panel_before_halfway = true;
            }
            panel_while_items |= both_show(motion);
        });
        assert!(!panel_before_halfway, "the panel waits for the shape");
        assert!(!panel_while_items, "the panel waits for the items to go");
        // The squash swings on a little after the shape arrives.
        assert!(frames < 75, "settles in {frames} frames");
        assert_eq!(motion.edges.rect(), panel);
        assert_eq!(motion.item(1, 10.0), (0.0, 0.0), "the items made way");
        assert_eq!(motion.popover.panel.value, 1.0);
        assert!(!motion.closing_done(), "open, not closing");

        popover(&mut motion, false, panel, true);
        let mut items_while_panel = false;
        run(&mut motion, Instant::now(), |motion| {
            if both_show(motion) {
                items_while_panel = true;
            }
        });
        assert!(!items_while_panel, "the items wait for the panel to go");
        assert_eq!(motion.edges.rect(), island);
        assert_eq!(motion.item(1, 10.0), (0.0, 1.0));
        assert_eq!(motion.popover.panel.value, 0.0);
        assert!(motion.closing_done());
        assert!(!motion.closing_done(), "said only once");
    }

    #[test]
    fn a_panel_changing_size_keeps_its_content() {
        let island = rect(100.0, 120.0);
        let tall = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 480.0));
        let short = Rectangle {
            height: 150.0,
            ..tall
        };
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, tall, true);
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.popover.panel.value, 1.0);

        for to in [short, tall] {
            popover(&mut motion, true, to, true);
            let mut faded = false;
            run(&mut motion, Instant::now(), |motion| {
                faded |= motion.popover.panel.value < 1.0;
            });
            assert!(!faded, "no flash of the island's color");
            assert_eq!(motion.edges.rect(), to);
        }
    }

    #[test]
    fn a_growing_panel_holds_its_shape_until_there_is_room() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let taller = Rectangle {
            height: 320.0,
            ..panel
        };
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        run(&mut motion, Instant::now(), |_| {});

        // Too tall for the surface so far: it stays where it is, content and all.
        popover(&mut motion, true, taller, false);
        assert!(!motion.step(Instant::now()), "nothing moves");
        assert_eq!(motion.edges.rect(), panel);
        assert_eq!(motion.popover.panel.value, 1.0);
        assert_eq!(motion.reach(), Some(taller), "asks for the room at once");

        popover(&mut motion, true, taller, true);
        let mut faded = false;
        run(&mut motion, Instant::now(), |motion| {
            faded |= motion.popover.panel.value < 1.0;
        });
        assert!(!faded, "the content stays while it grows");
        assert_eq!(motion.edges.rect(), taller);
    }

    #[test]
    fn a_page_swap_keeps_its_room_before_the_shape_sets_off() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let shorter = Rectangle {
            height: 90.0,
            ..panel
        };
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        run(&mut motion, Instant::now(), |_| {});
        motion.popover.reported = motion.reach();

        // A plugin swapped its page: laid out and reported before anything aims the edges.
        motion.place_panel(shorter, true);
        assert_eq!(motion.reach(), Some(panel), "still has the room it had");

        motion.aim();
        assert_eq!(motion.reach(), Some(panel), "and keeps it on the way");
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.reach(), Some(shorter), "gives it back once it lands");
    }

    #[test]
    fn a_shrinking_panel_keeps_its_room_until_it_lands() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let shorter = Rectangle {
            height: 90.0,
            ..panel
        };
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        run(&mut motion, Instant::now(), |_| {});
        motion.popover.reported = motion.reach();
        assert_eq!(motion.popover.reported, Some(panel));

        popover(&mut motion, true, shorter, true);
        let mut kept = true;
        let mut between = false;
        run(&mut motion, Instant::now(), |motion| {
            let height = motion.edges.rect().height;
            between |= height < panel.height - 1.0 && height > shorter.height + 1.0;
            if !motion.edges.bottom.at_rest() {
                kept &= motion.reach() == Some(panel);
            }
        });
        assert!(between, "the shape eases down");
        assert!(kept, "the room stays while it moves");
        assert_eq!(motion.reach(), Some(shorter), "and goes once it lands");
    }

    /// Whether the island's items and the panel's content are both drawn.
    fn both_show(motion: &Motion) -> bool {
        motion.popover.items.value >= REST_DISTANCE && motion.popover.panel.value >= REST_DISTANCE
    }

    #[test]
    fn reopening_while_closing_turns_around() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        run(&mut motion, Instant::now(), |_| {});
        popover(&mut motion, false, panel, true);
        let start = Instant::now();
        for frame in 0..5 {
            motion.step(start + FRAME * frame);
        }
        popover(&mut motion, true, panel, true);
        run(&mut motion, start + FRAME * 5, |_| {});
        assert_eq!(motion.edges.rect(), panel);
        assert_eq!(motion.shape(), panel);
        assert!(!motion.closing_done());
    }

    /// How far the skin is out of the middle of each side, past its edge: left, top, right,
    /// bottom.
    fn skin(motion: &Motion) -> [f32; 4] {
        [Side::Left, Side::Top, Side::Right, Side::Bottom].map(|side| motion.jelly.value(side, 0.5))
    }

    /// Plays the motion to rest, and returns the furthest each side's skin went out (`max`) and in
    /// (`min`), and the frame's rectangle when the bottom went furthest in.
    fn skin_range(motion: &mut Motion) -> ([f32; 4], [f32; 4], Rectangle) {
        let (mut out, mut inward) = ([0.0f32; 4], [0.0f32; 4]);
        let mut flattest = motion.edges.rect();
        run(motion, Instant::now(), |motion| {
            let now = skin(motion);
            if now[3] < inward[3] {
                flattest = motion.edges.rect();
            }
            for side in 0..4 {
                out[side] = out[side].max(now[side]);
                inward[side] = inward[side].min(now[side]);
            }
            assert!(
                motion.shape() == motion.edges.rect(),
                "the frame is the edges: only the skin gives"
            );
        });
        assert!(motion.jelly.is_flat(), "the skin settles flat");
        (out, inward, flattest)
    }

    /// How far a side stretched the way its edge went out, opening: out if the skin stretches,
    /// in if it squashes instead (a negative `SKIN_STRETCH`).
    fn stretched(out: [f32; 4], inward: [f32; 4], side: usize) -> f32 {
        if theme::SKIN_STRETCH >= 0.0 {
            out[side]
        } else {
            -inward[side]
        }
    }

    #[test]
    fn opening_stretches_the_way_it_grows_and_closing_splats_against_the_top() {
        // Only down: the bottom leads, and the sides draw in to keep its area.
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(120.0, 300.0));
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        let (out, inward, _) = skin_range(&mut motion);
        assert!(
            stretched(out, inward, 3) > 1.0,
            "the bottom stretches: {out:?} {inward:?}"
        );
        assert!(
            stretched(inward, out, 0) < -0.1 && stretched(inward, out, 2) < -0.1,
            "the sides draw in: {out:?} {inward:?}"
        );
        assert!(
            out[1].abs() < 0.5 && inward[1].abs() < 0.5,
            "the top stays on the bar"
        );
        assert_eq!(motion.shape(), panel);

        // Closing: the bottom carries on up into the top as it lands, and the sides bulge.
        popover(&mut motion, false, panel, true);
        let (out, inward, flattest) = skin_range(&mut motion);
        assert!(inward[3] < -1.0, "lands flatter: {inward:?}");
        assert!(out[0] > 0.2 && out[2] > 0.2, "and wider: {out:?}");
        assert!(
            flattest.height < island.height + 12.0,
            "flattest as it lands, not on the way: {flattest:?}"
        );
        assert_eq!(motion.shape(), island);
        assert!(motion.closing_done());
    }

    #[test]
    fn a_wide_short_popover_opens_wider_and_flatter() {
        let island = rect(300.0, 100.0);
        let panel = Rectangle::new(Point::new(150.0, 6.0), Size::new(400.0, 60.0));
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        let (out, inward, _) = skin_range(&mut motion);
        let (left, right) = (stretched(out, inward, 0), stretched(out, inward, 2));
        assert!(
            left > 1.0 && right > 1.0,
            "both sides stretch: {out:?} {inward:?}"
        );
        assert!(stretched(out, inward, 3) < right, "more across than down");
        assert_eq!(motion.shape(), panel);
    }

    #[test]
    fn a_popover_growing_right_leans_right_and_lands_from_the_right() {
        // As wide as the island going down, and 100 px wider to the right: the left side stays.
        let island = rect(100.0, 200.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(300.0, 40.0));
        let mut motion = settled(island, &[1]);
        popover(&mut motion, true, panel, true);
        let (out, inward, _) = skin_range(&mut motion);
        let (left, right) = (stretched(out, inward, 0), stretched(out, inward, 2));
        assert!(right > 0.5, "leans right: {out:?} {inward:?}");
        assert!(left < right / 3.0, "the left barely: {out:?} {inward:?}");

        // Back right to left: the right side carries on in, past the island's.
        popover(&mut motion, false, panel, true);
        let (out, inward, _) = skin_range(&mut motion);
        assert!(inward[2] < -1.0, "the right side carries on in: {inward:?}");
        assert!(
            out[0].max(-inward[0]) < -inward[2] / 2.0,
            "the left moves less: {out:?} {inward:?}"
        );
        assert_eq!(motion.shape(), island);
    }

    #[test]
    fn an_island_that_changes_size_lands_by_how_far_it_went() {
        let island = rect(100.0, 200.0);
        let carried = |motion: &mut Motion, to: Rectangle| {
            motion.arrange(to, [(1, 110.0)].into_iter(), None);
            let (out, _, _) = skin_range(motion);
            assert_eq!(motion.shape(), to);
            out[2]
        };
        let mut motion = settled(island, &[1]);
        let small = carried(&mut motion, rect(100.0, 205.0));
        assert!(small < 0.2, "a small change hardly reacts: {small}");
        let big = carried(&mut motion, rect(100.0, 285.0));
        assert!(big > 0.5, "the side that grew goes on out: {big}");
        assert!(big > 5.0 * small);
    }

    #[test]
    fn a_shape_sliding_as_a_whole_keeps_its_skin() {
        let mut motion = settled(rect(100.0, 200.0), &[1]);
        motion.arrange(rect(300.0, 200.0), [(1, 310.0)].into_iter(), None);
        let (out, inward, _) = skin_range(&mut motion);
        // The push's own lean pokes the top and bottom ends; the sides aren't pushed.
        assert!(
            out[0].abs() < 0.3 && out[2].abs() < 0.3,
            "{out:?} {inward:?}"
        );
    }

    #[test]
    fn the_skin_goes_as_far_as_the_language_says() {
        // A side that comes SKIN_TRAVEL with nothing round it: one REACT spring per side, so the
        // ring, sharing with its neighbors and keeping its area, goes about as far.
        let island = rect(100.0, 120.0);
        let mut motion = settled(island, &[1]);
        motion.arrange(
            Rectangle {
                width: 120.0 + theme::SKIN_TRAVEL,
                ..island
            },
            [(1, 110.0)].into_iter(),
            None,
        );
        let (out, _, _) = skin_range(&mut motion);
        let most = (theme::SKIN_STRETCH.max(theme::SKIN_CARRY) * theme::REACT_SIZE).abs();
        assert!(
            out[2] > 0.4 * most && out[2] < 1.6 * most,
            "about {most}: {}",
            out[2]
        );
    }

    #[test]
    fn the_dormer_drops_from_the_top_and_splats_back_into_it() {
        // A dot at the top center, and the dormer well down the screen.
        let island = rect(490.0, 20.0);
        let panel = centered(Size::new(480.0, 360.0), 1000.0, 200.0);
        let mut motion = settled(island, &[1]);

        // Every edge rides MORPH, so the frame glides down whole and never passes where it goes.
        popover(&mut motion, true, panel, true);
        let frames = run(&mut motion, Instant::now(), |motion| {
            let edges = motion.edges.rect();
            assert!(
                edges.y >= island.y - 0.5 && edges.y <= panel.y + 0.5,
                "the top glides"
            );
            assert!(edges.y + edges.height <= panel.y + panel.height + 0.5);
            assert!(edges.width <= panel.width + 0.5);
        });
        assert!(frames < 90, "settles in {frames} frames");
        assert_eq!(motion.shape(), panel);

        // Back up: it lands flatter, against the top, and nowhere on the way.
        popover(&mut motion, false, panel, true);
        let (_, inward, flattest) = skin_range(&mut motion);
        assert!(inward[3] < -0.5, "splats: {inward:?}");
        assert!(
            flattest.y - island.y < 2.0 && flattest.height < island.height * 1.5,
            "splats as it lands, not on the way: {flattest:?}"
        );
        assert_eq!(motion.shape(), island);
        assert!(motion.closing_done());
    }

    #[test]
    fn popovers_grow_from_their_island_and_stay_on_screen() {
        let panel = Size::new(200.0, 100.0);
        let pad = 2.0 * f32::from(theme::PANEL_PADDING);
        let gap = f32::from(theme::EDGE_GAP);
        let width = 200.0 + pad;

        let left = place(rect(6.0, 100.0), panel, Horizontal::Left, 1000.0, None);
        assert_eq!((left.x, left.y), (6.0, 6.0));
        assert_eq!(left.size(), Size::new(width, 100.0 + pad));

        let right = place(rect(894.0, 100.0), panel, Horizontal::Right, 1000.0, None);
        assert_eq!(right.x + right.width, 994.0);

        let center = place(rect(450.0, 100.0), panel, Horizontal::Center, 1000.0, None);
        assert_eq!(center.center_x(), 500.0);

        // Never narrower than the island.
        let wide = place(rect(300.0, 400.0), panel, Horizontal::Center, 1000.0, None);
        assert_eq!(wide.width, 400.0);

        // Kept on screen.
        let clamped = place(rect(950.0, 44.0), panel, Horizontal::Left, 1000.0, None);
        assert_eq!(clamped.x + clamped.width, 1000.0 - gap);

        // With a 200-wide notch, from 400 to 600, each stays on its island's side, and may reach
        // right up to the notch.
        let notch = Some(200.0);
        let before = place(rect(330.0, 70.0), panel, Horizontal::Center, 1000.0, notch);
        assert_eq!(before.x + before.width, 400.0);
        let after = place(rect(600.0, 70.0), panel, Horizontal::Center, 1000.0, notch);
        assert_eq!(after.x, 600.0);
        let far = place(rect(6.0, 100.0), panel, Horizontal::Left, 1000.0, notch);
        assert_eq!(far.x, 6.0);
    }

    #[test]
    fn a_centered_popover_sits_across_the_middle_at_its_top() {
        let pad = 2.0 * f32::from(theme::PANEL_PADDING);
        let rect = centered(Size::new(400.0, 300.0), 1000.0, 180.0);
        assert_eq!(rect.center_x(), 500.0);
        assert_eq!(rect.y, 180.0);
        assert_eq!(rect.size(), Size::new(400.0 + pad, 300.0 + pad));
    }

    #[test]
    fn keys_close_a_popover_unless_a_field_has_the_focus() {
        let escape = keyboard::Key::Named(key::Named::Escape);
        let shift = keyboard::Key::Named(key::Named::Shift);
        let control = keyboard::Key::Named(key::Named::Control);
        let letter = keyboard::Key::Character("a".into());
        assert_eq!(key_policy(&escape, true), KeyAction::Close);
        assert_eq!(key_policy(&escape, false), KeyAction::Close);
        assert_eq!(key_policy(&letter, true), KeyAction::Keep);
        assert_eq!(key_policy(&shift, true), KeyAction::Keep);
        assert_eq!(key_policy(&letter, false), KeyAction::Close);
        // A modifier alone may start a shortcut (D-91).
        assert_eq!(key_policy(&shift, false), KeyAction::Keep);
        assert_eq!(key_policy(&control, false), KeyAction::Keep);
    }

    #[test]
    fn a_press_squeezes_then_holds_still() {
        let mut motion = Motion::default();
        motion.press(Point::new(10.0, 10.0));
        let frames = run(&mut motion, Instant::now(), |_| {});
        assert!(frames > 1 && frames < 60, "settled in {frames} frames");
        assert_eq!(motion.inset.value, theme::PRESS_INSET);
        assert_eq!(motion.last, None);
    }

    #[test]
    fn letting_go_overshoots_once_then_settles_in_place() {
        let mut motion = Motion::default();
        motion.press(Point::new(0.0, 0.0));
        motion.drag(Point::new(200.0, 0.0));
        run(&mut motion, Instant::now(), |_| {});
        let pulled = motion.x.value;
        assert!(pulled > 0.0 && pulled < theme::DRAG_LIMIT_X);

        motion.release();
        let mut lowest = f32::MAX;
        run(&mut motion, Instant::now(), |motion| {
            lowest = lowest.min(motion.x.value)
        });
        assert!(lowest < 0.0, "springs past its place: {lowest}");
        assert!(lowest > -pulled / 2.0, "but only a little: {lowest}");
        assert_eq!(motion.offset(), Vector::ZERO);
        assert_eq!(motion.inset.value, 0.0);
    }

    #[test]
    fn a_drag_stretches_most_where_it_was_grabbed() {
        let island = rect(100.0, 300.0);
        let mut motion = settled(island, &[1]);
        motion.press(Point::new(120.0, 20.0));
        motion.drag(Point::new(320.0, 20.0));
        run(&mut motion, Instant::now(), |_| {});
        assert!(motion.pulled());

        let shape = motion.shape();
        let at_grab = motion.pull(shape, Point::new(120.0, 20.0)).x;
        let nearby = motion.pull(shape, Point::new(150.0, 20.0)).x;
        let far = motion.pull(shape, Point::new(380.0, 20.0)).x;
        assert!(at_grab > nearby && nearby > 0.0, "{at_grab} {nearby}");
        assert_eq!(far, 0.0);
        // Under the pointer, the island goes the whole way: its share plus the stretch.
        let whole = motion.offset().x + at_grab;
        assert!((whole - motion.x.value).abs() < 0.01, "{whole}");
        assert!(whole > motion.offset().x * 1.5);
    }

    #[test]
    fn letting_go_of_a_stretch_forgets_where_it_was_grabbed() {
        let mut motion = settled(rect(100.0, 300.0), &[1]);
        motion.press(Point::new(120.0, 20.0));
        motion.drag(Point::new(200.0, 40.0));
        run(&mut motion, Instant::now(), |_| {});
        motion.release();
        let mut stretched = false;
        run(&mut motion, Instant::now(), |motion| {
            stretched |= motion.pulled();
        });
        assert!(stretched, "springs back where it was grabbed");
        assert_eq!(motion.grab, None);
        assert!(!motion.pulled());
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn a_long_stall_does_not_throw_the_island() {
        let mut motion = Motion::default();
        motion.press(Point::ORIGIN);
        motion.drag(Point::new(100.0, 100.0));
        let start = Instant::now();
        motion.step(start);
        motion.step(start + Duration::from_secs(30));
        for spring in [motion.x, motion.y, motion.inset] {
            assert!(spring.value.is_finite() && spring.value.abs() <= theme::DRAG_LIMIT_X * 2.0);
        }
    }

    #[test]
    fn rubber_band_gives_less_and_less() {
        assert_eq!(rubber_band(0.0, 10.0), 0.0);
        assert_eq!(rubber_band(-30.0, 10.0), -rubber_band(30.0, 10.0));
        let near = rubber_band(1.0, 10.0);
        assert!(
            (near - theme::DRAG_RESISTANCE).abs() < 0.05,
            "about 1:1 near zero: {near}"
        );
        let mut previous = 0.0;
        for distance in [1.0, 10.0, 100.0, 10_000.0] {
            let moved = rubber_band(distance, 10.0);
            assert!(moved > previous && moved < 10.0);
            previous = moved;
        }
    }

    /// Where `settled` puts each item: 30 px wide, at 10, 50, 90…
    fn span(key: u64) -> Option<(f32, f32)> {
        Some((10.0 + 40.0 * (key - 1) as f32, 30.0))
    }

    /// How far a reaction going out `peak` goes, with every reaction's size.
    fn size(peak: f32) -> f32 {
        peak * theme::REACT_SIZE
    }

    /// Whether `value` is within `share` of how far a reaction going out `peak` goes.
    fn near(value: f32, peak: f32, share: f32) -> bool {
        (value - size(peak)).abs() <= share * size(peak)
    }

    /// An island at rest that has already noted the nudges so far.
    fn nudged(nudges: &[(u64, (u64, Nudge))]) -> Motion {
        let mut motion = settled(rect(0.0, 200.0), &[1, 2]);
        motion.nudges([(1, (1, Nudge::Up))], span, Instant::now());
        assert!(motion.at_rest(), "a new island doesn't replay old nudges");
        motion.nudges(nudges.iter().copied(), span, Instant::now());
        motion
    }

    #[test]
    fn a_push_goes_that_way_then_back() {
        let mut motion = nudged(&[(2, (2, Nudge::Right))]);
        assert!(!motion.at_rest());
        let mut furthest: f32 = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            let x = motion.offset().x;
            furthest = furthest.max(x);
            let rebound = 1.3 * REACT.overshoot() * size(theme::NUDGE_PUSH);
            assert!(
                x > -rebound - 0.2,
                "swings back past no more than its feel: {x}"
            );
        });
        assert!(near(furthest, theme::NUDGE_PUSH, 0.2), "{furthest}");
        assert_eq!(motion.offset(), Vector::ZERO);
        // Already played.
        motion.nudges([(2, (2, Nudge::Right))], span, Instant::now());
        assert!(motion.at_rest());
    }

    /// A nudge that lasts longer plays the same push, just as far, stretched out in time.
    #[test]
    fn a_lasting_push_is_slower_but_as_far() {
        let push = |lasting: Option<Duration>| {
            let request = NudgeRequest {
                nudge: Nudge::Right,
                lasting,
            };
            let mut motion = nudged(&[]);
            motion.nudges([(2, (2, request))], span, Instant::now());
            let (mut furthest, mut peak_at, mut frame): (f32, usize, usize) = (0.0, 0, 0);
            let frames = run(&mut motion, Instant::now(), |motion| {
                frame += 1;
                if motion.offset().x > furthest {
                    (furthest, peak_at) = (motion.offset().x, frame);
                }
            });
            (furthest, peak_at, frames)
        };
        let (far, peak, frames) = push(None);
        let (slow_far, slow_peak, slow_frames) = push(Some(REACT.settle_time() * 3));
        assert!((far - slow_far).abs() < 0.2, "{far} {slow_far}");
        assert!(slow_peak >= peak * 2, "{peak} {slow_peak}");
        assert!(slow_frames >= frames * 2, "{frames} {slow_frames}");
    }

    #[test]
    fn a_nudge_down_holds_a_flat_shelf_under_its_item_then_lets_it_go() {
        // Wide enough that the far end is well clear of the shelf.
        let mut motion = settled(rect(0.0, 400.0), &[1, 2]);
        motion.nudges([(1, (1, Nudge::Up))], span, Instant::now());
        let start = Instant::now();
        motion.nudges([(2, (2, Nudge::Down))], span, start);
        let shape = motion.shape();
        let depth = size(theme::NUDGE_SHELF);
        let (mut furthest, mut lowest, mut wider, mut item): (f32, f32, f32, f32) =
            (0.0, 0.0, 0.0, 0.0);
        let mut held = false;
        run(&mut motion, start, |motion| {
            let shelf = motion.jelly.shelves[1];
            let under = shelf.at(65.0);
            furthest = furthest.max(under);
            // Flat across the item, from 50 to 80.
            assert!((shelf.at(52.0) - under).abs() < 1e-4, "flat: {shelf:?}");
            assert!((shelf.at(78.0) - under).abs() < 1e-4, "flat: {shelf:?}");
            assert_eq!(shelf.at(300.0), 0.0, "and nothing far from it");
            assert_eq!(motion.jelly.shelves[0].depth, 0.0, "only the bottom");
            held |= motion.jelly.at_rest() && (under - depth).abs() < 0.01;
            lowest = lowest.max(motion.offset().y);
            wider = wider.max(motion.jelly.value(Side::Right, 1.0));
            item = item.max(motion.squish_at(shape, 65.0).1);
        });
        assert!(held, "it rests out while it's held");
        assert!(
            (furthest - depth).abs() < 0.2 * depth,
            "out as far as a shelf: {furthest}"
        );
        assert!(
            near(lowest, theme::NUDGE_LIFT, 0.25),
            "the island moves as far as a lift: {lowest}"
        );
        assert!(
            wider > size(theme::JELLY_LEAN) / 3.0,
            "the bottom widens: {wider}"
        );
        assert!(
            (item - depth / 2.0).abs() < 0.25 * depth,
            "the item rides half the shelf: {item}"
        );
        assert!(motion.jelly.is_flat());
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn a_shelf_eases_into_the_edge_without_a_crease() {
        let shelf = jelly::Shelf {
            depth: 2.0,
            from: 100.0,
            to: 130.0,
        };
        let slope = |x: f32| (shelf.at(x + 0.01) - shelf.at(x - 0.01)) / 0.02;
        let end = 100.0 - theme::NUDGE_SHOULDER;
        assert_eq!(shelf.at(end - 1.0), 0.0);
        assert!(slope(end + 0.02).abs() < 0.01, "it leaves the edge flat");
        assert!(slope(99.98).abs() < 0.01, "and meets its top flat");
        assert!(
            slope(100.0 - theme::NUDGE_SHOULDER / 2.0) > 0.05,
            "rising between"
        );
        assert_eq!(shelf.at(115.0), 2.0);
    }

    #[test]
    fn a_stream_of_nudges_keeps_the_shelf_out_and_lets_it_go_once() {
        let mut motion = nudged(&[]);
        let start = Instant::now();
        let depth = size(theme::NUDGE_SHELF);
        let mut now = start;
        let mut lurches = 0;
        let mut out_since: Option<Instant> = None;
        // A nudge every 90 ms for a second, like scrolling the volume.
        for frame in 0..62 {
            if frame % 6 == 0 && frame < 60 {
                let before = motion.push_y.velocity;
                let number = 2 + frame / 6;
                motion.nudges([(2, (number, Nudge::Down))], span, now);
                if motion.push_y.velocity != before {
                    lurches += 1;
                }
            }
            motion.step(now);
            let out = motion.jelly.shelves[1].at(65.0);
            if out > 0.95 * depth {
                out_since.get_or_insert(now);
            }
            if let Some(since) = out_since
                && now < start + Duration::from_millis(900)
            {
                assert!(
                    out > 0.9 * depth,
                    "no dip between nudges, {:?} in: {out}",
                    now - since
                );
            }
            now += FRAME;
        }
        assert_eq!(lurches, 1, "only the first nudge lurches the island");
        // It goes back once the nudges stop.
        let mut last = depth;
        run(&mut motion, now, |motion| {
            let out = motion.jelly.shelves[1].at(65.0);
            assert!(out <= last + 0.05 * depth, "straight back: {last} → {out}");
            last = out;
        });
        assert!(motion.jelly.is_flat());
    }

    #[test]
    fn a_nudge_the_other_way_swaps_the_shelf_over() {
        let mut motion = nudged(&[]);
        let now = Instant::now();
        motion.nudges([(2, (2, Nudge::Up))], span, now);
        assert!(motion.shelves[0].until.is_some());
        motion.nudges([(2, (3, Nudge::Down))], span, now);
        assert!(motion.shelves[0].until.is_none(), "the top lets go");
        assert_eq!(motion.shelves[0].depth.target, 0.0);
        assert!(motion.shelves[1].until.is_some(), "the bottom holds");
    }

    #[test]
    fn a_push_leans_into_where_its_going_smoothly() {
        let mut motion = nudged(&[(2, (2, Nudge::Right))]);
        let shape = motion.shape();
        let (mut swell, mut flat, mut taller, mut flatter): (f32, f32, f32, f32) =
            (0.0, 0.0, 1.0, 1.0);
        run(&mut motion, Instant::now(), |motion| {
            let jelly = &motion.jelly;
            swell = swell.max(jelly.value(Side::Top, 0.95));
            flat = flat.min(jelly.value(Side::Top, 0.05));
            // Top and bottom lean the same way.
            assert!((jelly.value(Side::Top, 0.9) - jelly.value(Side::Bottom, 0.9)).abs() < 0.01);
            // No creases: along the top, nothing jumps between neighbors.
            for step in 0..100 {
                let (a, b) = (step as f32 / 100.0, (step + 1) as f32 / 100.0);
                assert!((jelly.value(Side::Top, a) - jelly.value(Side::Top, b)).abs() < 0.2);
            }
            taller = taller.max(motion.squish_at(shape, 185.0).0.y);
            flatter = flatter.min(motion.squish_at(shape, 15.0).0.y);
        });
        assert!(
            near(swell, theme::JELLY_LEAN, 0.33),
            "the right end swells: {swell}"
        );
        assert!(
            near(-flat, theme::JELLY_LEAN, 0.33),
            "the left end flattens: {flat}"
        );
        let lean = size(theme::JELLY_LEAN) / shape.height;
        assert!(
            taller > 1.0 + lean && flatter < 1.0 - lean,
            "{taller} {flatter}"
        );
        assert!(motion.jelly.is_flat());

        let mut motion = nudged(&[(2, (2, Nudge::Left))]);
        let mut leaned: f32 = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            leaned = leaned.max(motion.jelly.value(Side::Top, 0.05));
        });
        assert!(
            near(leaned, theme::JELLY_LEAN, 0.33),
            "the left end swells: {leaned}"
        );

        let mut motion = nudged(&[(2, (2, Nudge::Up))]);
        let (mut top, mut shelf): (f32, f32) = (0.0, 0.0);
        run(&mut motion, Instant::now(), |motion| {
            top = top.max(motion.jelly.value(Side::Left, 0.0));
            shelf = shelf.max(motion.jelly.shelves[0].at(65.0));
        });
        assert!(
            top > size(theme::JELLY_LEAN) / 3.0 && shelf > 0.75 * size(theme::NUDGE_SHELF),
            "the top widens and holds a shelf out: {top} {shelf}"
        );
    }

    #[test]
    fn a_sideways_nudge_kicks_its_item_a_little_further_and_back() {
        let mut motion = nudged(&[(2, (2, Nudge::Left))]);
        let (mut one, mut two) = (0.0f32, 0.0f32);
        let mut narrowest: f32 = 1.0;
        run(&mut motion, Instant::now(), |motion| {
            one = one.max(motion.item(1, 10.0).0.abs());
            two = two.min(motion.item(2, 50.0).0);
            narrowest = narrowest.min(motion.kicked(2).x);
        });
        assert!(
            near(
                1.0 - narrowest,
                theme::NUDGE_KICK_SQUISH * theme::NUDGE_KICK,
                0.6
            ),
            "squished a little: {narrowest}"
        );
        assert_eq!(motion.kicked(1), Squish::NONE);
        assert_eq!(one, 0.0, "only the item that nudged");
        assert!(near(-two, theme::NUDGE_KICK, 0.6), "a few px left: {two}");
        assert_eq!(motion.item(2, 50.0), (0.0, 1.0), "and back");
        assert_eq!(motion.kicked(2), Squish::NONE, "unsquished");

        // Up and down hold a shelf out instead.
        let motion = nudged(&[(2, (2, Nudge::Up))]);
        assert_eq!(motion.items[1].kick.velocity, 0.0);
    }

    fn walls() -> Walls {
        Walls {
            top: Some(0.0),
            left: true,
            right: true,
            corners: [CornerShape::SQUARE; 2],
        }
    }

    #[test]
    fn a_push_into_a_wall_bounces_off_it_and_keeps_the_gap() {
        // 4 px from the right edge of a 1000 px surface: the push would take it 2 px from it.
        let mut motion = settled(rect(796.0, 200.0), &[1, 2]);
        motion.wall(Some(walls()), 1000.0, [CornerShape::SQUARE; 4]);
        let room = 4.0 - theme::WALL_GAP;
        motion.nudges([(1, (1, Nudge::Up))], span, Instant::now());
        motion.nudges([(2, (2, Nudge::Right))], span, Instant::now());
        let (mut furthest, mut squashed): (f32, f32) = (0.0, 0.0);
        run(&mut motion, Instant::now(), |motion| {
            furthest = furthest.max(motion.offset().x);
            squashed = squashed.min(motion.jelly.value(Side::Right, 0.5));
            // Bulging out of the side never reaches past the wall either.
            assert!(motion.jelly.room[1] <= room - motion.offset().x.max(0.0) + 1e-3);
        });
        assert!(furthest <= room + 1e-3, "kept the gap: {furthest}");
        assert!(furthest > room - 0.1, "got there: {furthest}");
        assert!(squashed < -0.1, "and squashed against it: {squashed}");
        assert_eq!(motion.offset(), Vector::ZERO);
    }

    #[test]
    fn a_drag_into_a_wall_stops_short_of_it() {
        let mut motion = settled(rect(6.0, 200.0), &[1]);
        motion.wall(Some(walls()), 1000.0, [CornerShape::SQUARE; 4]);
        motion.press(Point::new(20.0, 20.0));
        motion.drag(Point::new(-200.0, -200.0));
        run(&mut motion, Instant::now(), |motion| {
            let pulled = motion.offset() + motion.pull(motion.shape(), Point::new(20.0, 20.0));
            assert!(pulled.x >= -(6.0 - theme::WALL_GAP), "{pulled:?}");
            assert!(pulled.y >= -(6.0 - theme::WALL_GAP), "{pulled:?}");
        });
        // Away from the walls, as far as ever.
        motion.drag(Point::new(220.0, 20.0));
        assert!(motion.x.target > 0.9 * theme::DRAG_LIMIT_X);
    }

    #[test]
    fn a_held_island_lets_nudges_pass() {
        let mut motion = nudged(&[]);
        motion.press(Point::new(10.0, 10.0));
        motion.nudges(
            [(1, (2, Nudge::Left)), (2, (3, Nudge::Up))],
            span,
            Instant::now(),
        );
        assert_eq!(motion.push_x.velocity, 0.0);
        assert!(motion.jelly.at_rest());
        assert_eq!(motion.seen, Some(3));
    }

    #[test]
    fn a_pushed_island_piles_up_where_its_pushed_from_then_wiggles_across() {
        let mut motion = settled(rect(400.0, 120.0), &[1]);
        // Pushed 200 px left, by a neighbor on its right.
        motion.arrange(rect(200.0, 120.0), [(1, 210.0)].into_iter(), None);
        let (mut right_most, mut left_after) = (0.0f32, 0.0f32);
        let mut stopped = false;
        let frames = run(&mut motion, Instant::now(), |motion| {
            let top = |along| motion.jelly.value(Side::Top, along);
            let sliding = motion.edges.left.velocity.abs() > theme::PUSH_MOVING;
            // The first frame has no time to move in.
            if sliding && !stopped {
                right_most = right_most.max(top(0.9));
                assert!(top(0.9) >= top(0.1) - 0.01, "the right end leads the swell");
            } else if right_most > 0.0 {
                stopped = true;
                left_after = left_after.max(top(0.1));
            }
        });
        assert!(right_most > 0.5, "the right end swelled: {right_most}");
        assert!(left_after > 0.3, "then the left end: {left_after}");
        assert!(frames < 200, "and it settles");
        assert!(motion.jelly.is_flat());
    }

    #[test]
    fn a_resize_is_not_a_push() {
        let mut motion = settled(rect(400.0, 120.0), &[1]);
        motion.arrange(rect(200.0, 320.0), [(1, 210.0)].into_iter(), None);
        // Only the left edge moves: its side stretches and lands, but the far end of the top
        // doesn't pile up as if pushed.
        run(&mut motion, Instant::now(), |motion| {
            let far = motion.jelly.value(Side::Top, 0.9);
            assert!(far.abs() < 0.1, "no lean: {far}");
        });
    }

    #[test]
    fn width_groups_take_their_widest() {
        let groups = [Some("status"), None, Some("status"), Some("other")];
        assert_eq!(
            slots(&[30.0, 50.0, 20.0, 10.0], &groups),
            [30.0, 50.0, 30.0, 10.0]
        );
        assert_eq!(slots(&[30.0, 20.0], &[]), [30.0, 20.0]);
    }

    #[test]
    fn another_popover_takes_over_the_morph_and_fades_its_content_in() {
        let island = rect(100.0, 120.0);
        let first = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let second = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 120.0));
        let mut motion = settled(island, &[1]);
        motion.swap(Some(1));
        popover(&mut motion, true, first, true);
        run(&mut motion, Instant::now(), |_| {});
        assert_eq!(motion.popover.panel.value, 1.0);

        // The same popover again changes nothing.
        motion.swap(Some(1));
        assert_eq!(motion.popover.panel.value, 1.0);

        motion.swap(Some(2));
        popover(&mut motion, true, second, true);
        assert_eq!(
            motion.popover.panel.value, 0.0,
            "the new content starts hidden"
        );
        let mut items_shown = false;
        run(&mut motion, Instant::now(), |motion| {
            items_shown |= motion.popover.items.value > REST_DISTANCE;
        });
        assert!(!items_shown, "never back to the island on the way");
        assert_eq!(motion.edges.rect(), second);
        assert_eq!(motion.popover.panel.value, 1.0);
    }
}
