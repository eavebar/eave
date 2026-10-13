//! An island: the squircle behind a zone's items. It never snaps to a new shape. When items come
//! or go, its edges spring to the new size, the items that stay slide to their new places and new
//! ones fade in. It reacts to the pointer like something physical: a press squeezes it in a
//! little, dragging pulls it along with growing resistance, and letting go springs it back.
//!
//! An island can also become a popover. Given one, it morphs into the panel's shape, growing from
//! where it sits, while its items fade out and the panel's content fades in. It squishes as it
//! sets off, squeezed in from the sides, and eases back as it grows. Closing runs the morph
//! backwards, lands like it hit the top (it splats into it, flatter and wider, then eases back),
//! and reports when it's done. The panel lives in an overlay, so it's laid out against the whole
//! surface, drawn over the other islands, and sees events first.
//!
//! An island can have a toggle, a control of the host's at one end, there only while the pointer
//! is over the island: hovering grows the island to make room for it, and it fades in.
//!
//! It only watches pointer events on the items and never captures them, so the items work exactly
//! as they would without it. Like the switcher, it steps one frame at a time when the compositor
//! asks, and stops asking once everything is at rest.

use std::cell::Cell;
use std::time::{Duration, Instant};

use eave_api::{Nudge, NudgeRequest};

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::alignment::Horizontal;
use iced::keyboard::{self, key};
use iced::widget::canvas::{Cache, Path};
use iced::{
    Color, Element, Event, Length, Padding, Point, Rectangle, Size, Transformation, Vector, window,
};

use crate::field::AnyFocused;
use crate::jelly::{Jelly, Side};
use crate::spring::{DRAG, FADE, MAX_FRAME, MORPH, NUDGE, PLAYFUL, REST_DISTANCE, SQUISH, Spring};
use crate::squircle::{self, CornerShape, squircle_corners, warp};
use crate::squish::{self, Squish};
use crate::theme;

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
    /// A live island: snug around its items.
    live: bool,
    /// How far its shape reaches past its layout, toward the edges it's attached to (D-70), and its
    /// popover's with it.
    bleed: Padding,
    /// The width of a notch centered at the top of the surface, which its popover stays clear of.
    notch: Option<f32>,
    /// How it's rounded outwards where it meets the top edge, if it's attached there (D-72).
    shoulders: Option<CornerShape>,
    /// Its corners: top left, top right, bottom right, bottom left (D-73).
    corners: [CornerShape; 4],
    /// How far past its layout it can be seen each way, less where it can't: its items are
    /// centered in that.
    shows: Padding,
    /// How tall it is at rest: the theme's, or down to where the notch ends (D-76).
    height: f32,
}

/// The control at one end of an island that shows and hides its collapsible items: there while
/// the pointer is over the island, or always while they show.
pub struct Toggle<'a, M> {
    pub element: Element<'a, M>,
    /// Before the first item (`Left`), or after the last: where the items come out.
    pub side: Horizontal,
    /// Whether the collapsible items show. When it changes, the island squashes the way they
    /// went, like a nudge.
    pub expanded: bool,
}

/// The toggle's key among the items' keys: no item has it.
const TOGGLE: u64 = u64::MAX;

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
            live: false,
            bleed: Padding::ZERO,
            notch: None,
            shoulders: None,
            corners: [CornerShape::new(theme::ISLAND_RADIUS, theme::ISLAND_SMOOTHING); 4],
            shows: Padding::ZERO,
            height: theme::ISLAND_HEIGHT as f32,
        }
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
        }) = toggle
        else {
            return self;
        };
        self.toggle_side = side;
        self.expanded = expanded;
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
    /// How much slower than usual the last nudge plays: the push springs and the jelly run on
    /// time divided by it. 1 unless the nudge said how long it should last.
    pace: f32,
    /// How many pixels a popover's overshoot adds to the shape's width and height (negative is
    /// narrower or shorter), on top of its edges.
    stretch_x: Spring,
    stretch_y: Spring,
    /// The newest nudge already played, once the island has been laid out.
    seen: Option<u64>,
    /// The fastest the island has slid as a whole in the push going on, signed (negative is
    /// left). 0 when it isn't being pushed.
    pushed: f32,
    /// How long the push going on has lasted, in seconds: the swell builds up over it.
    pushing: f32,
    /// The previous frame, while something is moving.
    last: Option<Instant>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            press: None,
            grab: None,
            x: Spring::new(0.0, DRAG),
            y: Spring::new(0.0, DRAG),
            inset: Spring::new(0.0, PLAYFUL),
            island: None,
            edges: Edges::at(Rectangle::default()),
            items: Vec::new(),
            popover: PopoverMotion::default(),
            push_x: Spring::new(0.0, NUDGE),
            push_y: Spring::new(0.0, NUDGE),
            jelly: Jelly::default(),
            pace: 1.0,
            stretch_x: Spring::new(0.0, SQUISH),
            stretch_y: Spring::new(0.0, SQUISH),
            seen: None,
            pushed: 0.0,
            pushing: 0.0,
            last: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Edges {
    left: Spring,
    top: Spring,
    right: Spring,
    bottom: Spring,
}

impl Edges {
    fn at(rect: Rectangle) -> Self {
        Self {
            left: Spring::new(rect.x, MORPH),
            top: Spring::new(rect.y, MORPH),
            right: Spring::new(rect.x + rect.width, MORPH),
            bottom: Spring::new(rect.y + rect.height, MORPH),
        }
    }

    fn aim(&mut self, rect: Rectangle) {
        self.left.target = rect.x;
        self.top.target = rect.y;
        self.right.target = rect.x + rect.width;
        self.bottom.target = rect.y + rect.height;
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
    /// Whether this way of the morph has had its squish (opening) or splat (closing).
    squished: bool,
    /// How much the island's items show: they make way for the panel.
    items: Spring,
    /// How much the panel's content shows.
    panel: Spring,
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
            squished: false,
            items: Spring::new(1.0, FADE),
            panel: Spring::new(0.0, FADE),
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

    /// Pulls toward the pointer, harder to move the further it goes.
    fn drag(&mut self, to: Point) {
        let Some(from) = self.press else {
            return;
        };
        self.x.target = rubber_band(to.x - from.x, theme::DRAG_LIMIT_X);
        self.y.target = rubber_band(to.y - from.y, theme::DRAG_LIMIT_Y);
    }

    fn release(&mut self) {
        self.press = None;
        self.x.target = 0.0;
        self.y.target = 0.0;
        self.inset.target = 0.0;
    }

    /// Plays the nudges it hasn't seen yet, with where each item's center is across the island.
    /// A new island only notes them: they happened before it. A held island, or one with a
    /// popover, lets them pass.
    fn nudges<N: Into<NudgeRequest>>(
        &mut self,
        nudges: impl IntoIterator<Item = (u64, (u64, N))>,
        center: impl Fn(u64) -> Option<f32>,
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
                self.nudge(request, center(key));
                self.kick(key, request.nudge);
            }
        }
        self.seen = Some(newest);
    }

    /// Sets the island off: sideways as a whole, or up or down half a pixel. Its outline leans into
    /// it, swelling on the side it's going toward and flattening on the side it's leaving, and up
    /// or down the edge over the item (at `center`) bulges. The springs and the jelly bring it
    /// back. A nudge that says how long it lasts plays the same motion stretched out to that.
    fn nudge(&mut self, request: NudgeRequest, center: Option<f32>) {
        let NudgeRequest { nudge, lasting } = request;
        // The springs run on time slowed by the pace, so the same kicks go just as far, slower.
        self.pace = lasting.map_or(1.0, |lasting| {
            (lasting.as_secs_f32() / theme::NUDGE_LASTING.as_secs_f32()).clamp(0.5, 5.0)
        });
        let (push, lift) = (theme::NUDGE_PUSH_SPEED, theme::NUDGE_LIFT_SPEED);
        let lean = theme::JELLY_LEAN;
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
                    (-1.0, Side::Top)
                } else {
                    (1.0, Side::Bottom)
                };
                self.push_y.velocity = way * lift;
                // Down the sides runs top to bottom, so going up swells the top.
                for side in [Side::Left, Side::Right] {
                    self.jelly.kick(side, |along| way * lean_toward_end(along));
                }
                let (Some(center), Some(island)) = (center, self.island) else {
                    return;
                };
                if island.width < 1.0 {
                    return;
                }
                let at = (center - island.x) / island.width;
                let half = theme::NUDGE_BUMP_WIDTH / 2.0 / island.width;
                self.jelly.kick(edge, |along| {
                    let from = ((along - at) / half).clamp(-1.0, 1.0);
                    theme::JELLY_BUMP * 0.5 * (1.0 + (std::f32::consts::PI * from).cos())
                });
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
            let lean = strength(speed, theme::PUSH_LEAN) * building;
            for side in [Side::Top, Side::Bottom] {
                self.jelly
                    .kick(side, |along| -way * lean * toward_end(along));
            }
        } else if self.pushed != 0.0 {
            let way = self.pushed.signum();
            let wiggle = strength(self.pushed, theme::PUSH_WIGGLE);
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
    fn arrange(
        &mut self,
        island: Rectangle,
        items: impl Iterator<Item = (u64, f32)>,
        popover: Option<bool>,
    ) {
        // A new island grows out of a sliver at its center.
        if self.island.is_none() {
            let sliver = Rectangle::new(
                Point::new(island.center_x(), island.y),
                Size::new(0.0, island.height),
            );
            self.edges = Edges::at(sliver);
        }
        self.island = Some(island);

        let mut kept = Vec::with_capacity(self.items.len());
        for (key, x) in items {
            match self.items.iter().position(|item| item.key == key) {
                Some(index) => {
                    let mut item = self.items.swap_remove(index);
                    item.x.target = x;
                    kept.push(item);
                }
                // New: in place at once, fading in.
                None => kept.push(ItemMotion {
                    key,
                    x: Spring::new(x, MORPH),
                    shown: Spring {
                        target: 1.0,
                        ..Spring::new(0.0, MORPH)
                    },
                    kick: Spring::new(0.0, NUDGE),
                }),
            }
        }
        // Whatever is left in `self.items` is gone.
        self.items = kept;

        let popover_state = &mut self.popover;
        match popover {
            Some(open) => {
                popover_state.present = true;
                if open && !popover_state.open {
                    popover_state.closed = false;
                }
                // Turning around squishes again.
                if open != popover_state.open {
                    popover_state.squished = false;
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

    /// Notes which popover the island has. Another one than before, while it's still a popover,
    /// takes over the morph: the shape springs on from where it is to the new panel, with a
    /// squish, and the new content fades in once the shape is halfway.
    fn swap(&mut self, key: Option<u64>) {
        let popover = &mut self.popover;
        if let (Some(new), Some(old)) = (key, popover.key)
            && new != old
            && popover.present
        {
            popover.panel = Spring::new(0.0, FADE);
            popover.squished = false;
            popover.closed = false;
        }
        popover.key = key;
    }

    /// Points the shape and the fades where they should go now. During a popover's morph, each
    /// side's content only fades in once the other side's is gone and the shape is more than
    /// halfway its way. The two never show at once: each fades by covering itself with the
    /// island's color, which would cover the other side's content too where they overlap.
    fn aim(&mut self) {
        let Some(island) = self.island else {
            return;
        };
        let popover = &mut self.popover;
        match (popover.opening(), popover.rect) {
            (Some(panel), _) => {
                self.edges.aim(panel);
                // Squeezed in from the sides as it sets off, then eases back: a squish on the
                // way, nothing to hit when it gets there.
                if !popover.squished {
                    popover.squished = true;
                    self.stretch_x.velocity = -theme::SQUISH_OPEN_SQUEEZE * panel.width;
                }
                popover.items.target = 0.0;
                popover.panel.target = if popover.items.value < REST_DISTANCE
                    && self.edges.progress(island, panel) > 0.5
                {
                    1.0
                } else {
                    0.0
                };
            }
            (None, Some(panel)) if popover.present => {
                self.edges.aim(island);
                // Splats against the top as it lands. Not while it's open and waiting for room.
                let landing = self.edges.bottom.value - (island.y + island.height);
                if !popover.open && !popover.squished && landing < theme::SQUISH_LAND {
                    popover.squished = true;
                    self.stretch_y.velocity = -theme::SQUISH_CLOSE_FLATTEN;
                    self.stretch_x.velocity = theme::SQUISH_CLOSE_WIDEN;
                }
                popover.panel.target = 0.0;
                popover.items.target = if popover.panel.value < REST_DISTANCE
                    && self.edges.progress(panel, island) > 0.5
                {
                    1.0
                } else {
                    0.0
                };
            }
            _ => {
                self.edges.aim(island);
                popover.items.target = 1.0;
                popover.panel.target = 0.0;
            }
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
        let destination = self.popover.opening().or(self.island);
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
            &mut edges.left,
            &mut edges.top,
            &mut edges.right,
            &mut edges.bottom,
            &mut self.popover.items,
            &mut self.popover.panel,
            &mut self.stretch_x,
            &mut self.stretch_y,
        ]
        .into_iter()
        .chain(
            self.items
                .iter_mut()
                .flat_map(|item| [&mut item.x, &mut item.shown]),
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
            item.kick.velocity = way * theme::NUDGE_KICK_SPEED;
        }
    }

    fn at_rest(&mut self) -> bool {
        self.jelly.at_rest()
            && self.springs().all(|spring| spring.at_rest())
            && self.paced().all(|spring| spring.at_rest())
    }

    /// Advances to `now`. Returns whether another frame is needed.
    fn step(&mut self, now: Instant) -> bool {
        self.aim();
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
        for spring in self.springs() {
            spring.advance(elapsed);
        }
        let paced = elapsed / self.pace;
        for spring in self.paced() {
            spring.advance(paced);
        }
        self.lean_with_push(paced);
        self.jelly.advance(paced);
        // The fades wait on how far the shape has come.
        self.aim();
        if self.at_rest() {
            self.springs().for_each(Spring::settle);
            self.paced().for_each(Spring::settle);
            self.jelly.settle();
            self.let_go_of_grab();
            self.last = None;
            return false;
        }
        true
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

    /// The shape as it's drawn: its edges, splatting. Stretching along one axis pulls the other
    /// in. It stretches about its center across, and down from its top, which it hit.
    fn shape(&self) -> Rectangle {
        let edges = self.edges.rect();
        let (stretch_x, stretch_y) = (self.stretch_x.value, self.stretch_y.value);
        if stretch_x == 0.0 && stretch_y == 0.0 {
            return edges;
        }
        let width = (edges.width + stretch_x - theme::SQUISH_BULGE * stretch_y).max(0.0);
        let height = (edges.height + stretch_y - theme::SQUISH_BULGE * stretch_x).max(0.0);
        Rectangle::new(
            Point::new(edges.center_x() - width / 2.0, edges.y),
            Size::new(width, height),
        )
    }

    /// How much the overshoot stretches the shape, across and up and down.
    fn squish(&self) -> Squish {
        let (edges, shape) = (self.edges.rect(), self.shape());
        if edges.width < 1.0 || edges.height < 1.0 || shape == edges {
            return Squish::NONE;
        }
        Squish {
            x: shape.width / edges.width,
            y: shape.height / edges.height,
        }
    }

    /// How far the overshoot moves something centered at `at`: to where the stretch takes that
    /// point of the edges. What's there squishes itself, about its own center ([`squish`]).
    fn follow(&self, at: Point) -> Vector {
        let squish = self.squish();
        if squish == Squish::NONE {
            return Vector::ZERO;
        }
        let (edges, shape) = (self.edges.rect(), self.shape());
        let to = Point::new(
            shape.x + (at.x - edges.x) * squish.x,
            shape.y + (at.y - edges.y) * squish.y,
        );
        to - at
    }

    /// How squished something centered at `x` in the drawn `shape` is, and how far down the jelly
    /// moves it: by the shape's stretch, and by how much taller the jelly makes the shape there.
    fn squish_at(&self, shape: Rectangle, x: f32) -> (Squish, f32) {
        let squish = self.squish();
        let (taller, down) = self.jelly.at(shape, x);
        (
            Squish {
                x: squish.x,
                y: squish.y * taller,
            },
            down,
        )
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
            y: 1.0 + theme::NUDGE_KICK_SQUISH / 2.0 * out,
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
        let near = (1.0 - distance / theme::DRAG_REACH).clamp(0.0, 1.0);
        let near = near * near * near * (near * (6.0 * near - 15.0) + 10.0);
        let near = near * (1.0 - theme::DRAG_FOLLOW);
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
            Some(item) => (item.x.value + item.kick.value - x, item.shown.value * fade),
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

/// Moves almost one to one at first, then gives less and less, approaching `limit` without ever
/// reaching it.
fn rubber_band(distance: f32, limit: f32) -> f32 {
    let pulled = distance.abs() * theme::DRAG_RESISTANCE;
    (limit * (1.0 - 1.0 / (1.0 + pulled / limit))).copysign(distance)
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
/// while typing (D-41).
fn key_policy(key: &keyboard::Key, field_focused: bool) -> KeyAction {
    match key {
        keyboard::Key::Named(key::Named::Escape) => KeyAction::Close,
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
    own_corners: [CornerShape; 4],
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
    if shape.width < 1.0 || shape.height < 1.0 {
        return;
    }
    // Squeezed by the same few pixels whatever the island's width, about its center.
    let scale = (1.0 - 2.0 * motion.inset.value / shape.width).max(0.5);
    let center = shape.center();
    let transformation = Transformation::translate(center.x, center.y)
        * Transformation::scale(scale)
        * Transformation::translate(-center.x, -center.y);

    // The frame clips to its size: it has room all round for the jelly and the stretch.
    let room = Vector::new(
        theme::NUDGE_BUMP_ROOM + theme::DRAG_LIMIT_X,
        theme::NUDGE_BUMP_ROOM + theme::DRAG_LIMIT_Y,
    );
    let pulled = motion.pulled();
    let bent = !motion.jelly.is_flat() || pulled;
    if state.shape_bent.replace(bent) || bent {
        state.shape.clear();
    }

    // What's drawn: the shape, reaching on to the edges it's attached to.
    let body = Rectangle::new(
        Point::new(shape.x - bleed.left, shape.y - bleed.top),
        Size::new(
            shape.width + bleed.left + bleed.right,
            shape.height + bleed.top + bleed.bottom,
        ),
    );
    renderer.with_translation(motion.offset(), |renderer| {
        renderer.with_transformation(transformation, |renderer| {
            let frame_size = Size::new(body.width + 2.0 * room.x, body.height + 2.0 * room.y);
            let geometry = state.shape.draw(renderer, frame_size, |frame| {
                let size = body.size();
                let corners = own_corners;
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
                let hanging = shoulders.filter(|_| bleed.top > 0.0).map(|shoulder| {
                    let most = room.x / (1.0 + shoulder.smoothing);
                    let shoulder = CornerShape::new(shoulder.radius.min(most), shoulder.smoothing);
                    let sides = [bleed.left <= 0.0, bleed.right <= 0.0];
                    (room.y - body.y, shoulder, sides)
                });
                // Black on black, with no border: what touches the notch or the bezel reads as
                // one shape with it (D-74).
                let outline = match hanging {
                    Some((edge, shoulder, sides)) => Path::new(|builder| {
                        squircle::hanging(builder, outline, corners, edge, shoulder, sides);
                    }),
                    None => squircle_corners(outline, corners),
                };
                frame.fill(&bend(outline), theme::ISLAND_BG);
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
                    // It rides along with the jelly where it is, and with the overshoot, to where
                    // the stretch takes its center. It squishes with both.
                    let moved = moved + motion.follow(bounds.center() + moved);
                    // And it goes with the stretch where it ends up.
                    let moved = moved + motion.pull(shape, bounds.center() + moved);
                    renderer.with_translation(moved, |renderer| {
                        with_clip(clip - moved, || {
                            squish::with(squished, || {
                                item.as_widget()
                                    .draw(tree, renderer, theme, style, child, cursor, viewport);
                            });
                        });
                        // In the item's moved coordinates, the shape is the other way.
                        fade(renderer, bounds, shape - moved, shown);
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
                            panel.as_widget().draw(
                                tree,
                                renderer,
                                theme,
                                style,
                                layout,
                                cursor,
                                &(visible - moved),
                            );
                            fade(renderer, layout.bounds(), shape - moved, shown);
                        });
                    }
                }
            });
        });
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
pub(crate) fn fade(renderer: &mut iced::Renderer, bounds: Rectangle, shape: Rectangle, shown: f32) {
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
                ..theme::ISLAND_BG
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
            motion: Motion::default(),
            shape: Cache::new(),
            shape_bent: Cell::new(false),
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
        let laid_out = nodes.len() - usize::from(hidden.is_some());
        // A live island has as much room at its ends as above and below its tallest item, in
        // what's seen of it.
        let tallest = nodes
            .iter()
            .enumerate()
            .filter(|&(index, _)| Some(index) != hidden)
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
        let slots = slots(&widths, &self.groups);
        let mut x = padding;
        let children: Vec<_> = nodes
            .into_iter()
            .zip(slots)
            .enumerate()
            .map(|(index, (node, slot))| {
                let size = node.size();
                if Some(index) == hidden {
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
        // island itself stays where its popover grows from: at the zone's side, or centered.
        let panel = self
            .popover
            .as_ref()
            .filter(|popover| popover.open)
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
            self.corners,
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
        let panel = &mut tree.state.downcast_mut::<State>().panel;
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
                if is_hidden(self.toggle, index, shown) {
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
            motion.nudge(request, None);
        }
        motion.swap(self.popover.as_ref().map(|popover| popover.key));
        // A hidden toggle isn't arranged, so it isn't drawn, and fades in when it shows.
        let positions = self
            .keys
            .iter()
            .zip(layout.children())
            .enumerate()
            .filter(|(index, _)| !is_hidden(self.toggle, *index, shown))
            .map(|(_, (key, child))| (*key, child.bounds().x));
        motion.arrange(
            own,
            positions,
            self.popover.as_ref().map(|popover| popover.open),
        );
        motion.nudges(self.nudges.iter().copied(), |key| {
            self.keys
                .iter()
                .zip(layout.children())
                .find(|(k, _)| **k == key)
                .map(|(_, child)| child.bounds().center_x())
        });

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
            if motion.closing_done() {
                shell.publish(popover.on_closed.clone());
            }
        }
        if !motion.at_rest() {
            shell.request_redraw();
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
            .filter(|(index, _)| !is_hidden(self.toggle, *index, shown))
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
        let rect = place(
            island,
            content.size(),
            self.island.align,
            bounds.width,
            self.island.notch,
        );
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
            island.corners,
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
        let key = match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => Some(key),
            _ => None,
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
            if key_policy(key, focused.found) == KeyAction::Keep {
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

    /// Steps frame by frame until the motion asks for no more; returns the frames taken.
    fn run(motion: &mut Motion, start: Instant, mut each: impl FnMut(&Motion)) -> usize {
        let mut now = start;
        for frame in 0..1000 {
            if !motion.step(now) {
                return frame;
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

    #[test]
    fn opening_squishes_and_closing_splats_against_the_top() {
        let island = rect(100.0, 120.0);
        let panel = Rectangle::new(Point::new(100.0, 6.0), Size::new(260.0, 200.0));
        let mut motion = settled(island, &[1]);
        let gap = f32::from(theme::EDGE_GAP);

        // Opening: squeezed in from the sides on the way, from its top, and no bump at the end.
        popover(&mut motion, true, panel, true);
        let mut squeezed = false;
        run(&mut motion, Instant::now(), |motion| {
            let (shape, edges) = (motion.shape(), motion.edges.rect());
            squeezed |= shape.width < edges.width - 4.0 && shape.height > edges.height;
            assert_eq!(shape.y, edges.y, "the top stays on the bar");
            assert!(
                shape.width < edges.width + 1.0,
                "never swings visibly wider"
            );
        });
        assert!(squeezed, "squished on the way");
        assert_eq!(motion.shape(), panel);

        // Closing: the top stays on the bar; the bottom gives.
        popover(&mut motion, false, panel, true);
        let (mut splat, mut spread) = (false, false);
        run(&mut motion, Instant::now(), |motion| {
            let (shape, edges) = (motion.shape(), motion.edges.rect());
            splat |= shape.height < island.height - 1.0 && shape.width > island.width + 1.0;
            assert_eq!(shape.y, edges.y, "the top stays put");
            assert!(
                shape.width - edges.width < 2.0 * gap,
                "stays on the surface"
            );
            // An item right of center moves right, and squishes wide and short.
            let at = Point::new(island.x + island.width * 0.75, island.center_y());
            let squish = motion.squish();
            spread |= motion.follow(at).x > 0.5 && squish.x > 1.01 && squish.y < 0.99;
        });
        assert!(splat, "lands flatter and wider");
        assert!(spread, "the items spread and squish with it");
        assert_eq!(motion.squish(), Squish::NONE);
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
    fn keys_close_a_popover_unless_a_field_has_the_focus() {
        let escape = keyboard::Key::Named(key::Named::Escape);
        let shift = keyboard::Key::Named(key::Named::Shift);
        let letter = keyboard::Key::Character("a".into());
        assert_eq!(key_policy(&escape, true), KeyAction::Close);
        assert_eq!(key_policy(&escape, false), KeyAction::Close);
        assert_eq!(key_policy(&letter, true), KeyAction::Keep);
        assert_eq!(key_policy(&shift, true), KeyAction::Keep);
        assert_eq!(key_policy(&shift, false), KeyAction::Close);
        assert_eq!(key_policy(&letter, false), KeyAction::Close);
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

    /// Where `settled` puts each item's center: 30 px wide, at 10, 50, 90…
    fn center(key: u64) -> Option<f32> {
        Some(25.0 + 40.0 * (key - 1) as f32)
    }

    /// An island at rest that has already noted the nudges so far.
    fn nudged(nudges: &[(u64, (u64, Nudge))]) -> Motion {
        let mut motion = settled(rect(0.0, 200.0), &[1, 2]);
        motion.nudges([(1, (1, Nudge::Up))], center);
        assert!(motion.at_rest(), "a new island doesn't replay old nudges");
        motion.nudges(nudges.iter().copied(), center);
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
            assert!(x > -0.2, "doesn't swing back past: {x}");
        });
        assert!((4.0..6.0).contains(&furthest), "{furthest}");
        assert_eq!(motion.offset(), Vector::ZERO);
        // Already played.
        motion.nudges([(2, (2, Nudge::Right))], center);
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
            motion.nudges([(2, (2, request))], center);
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
        let (slow_far, slow_peak, slow_frames) = push(Some(theme::NUDGE_LASTING * 3));
        assert!((far - slow_far).abs() < 0.2, "{far} {slow_far}");
        assert!(slow_peak >= peak * 2, "{peak} {slow_peak}");
        assert!(slow_frames >= frames * 2, "{frames} {slow_frames}");
    }

    #[test]
    fn a_nudge_down_bulges_the_bottom_under_its_item_then_flattens() {
        // Wide enough that the far end is well clear of the bump.
        let mut motion = settled(rect(0.0, 400.0), &[1, 2]);
        motion.nudges([(1, (1, Nudge::Up))], center);
        motion.nudges([(2, (2, Nudge::Down))], center);
        let shape = motion.shape();
        let (mut furthest, mut lowest, mut wider): (f32, f32, f32) = (0.0, 0.0, 0.0);
        let mut item: f32 = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            let jelly = &motion.jelly;
            let under = jelly.value(Side::Bottom, 65.0 / 400.0);
            furthest = furthest.max(under);
            assert!(
                under >= jelly.value(Side::Bottom, 0.9) - 0.05,
                "it's under the item: {under} {}",
                jelly.value(Side::Bottom, 0.9)
            );
            lowest = lowest.max(motion.offset().y);
            wider = wider.max(jelly.value(Side::Right, 1.0));
            item = item.max(motion.squish_at(shape, 65.0).1);
        });
        assert!(
            (0.4..0.65).contains(&lowest),
            "the island moves half a pixel: {lowest}"
        );
        assert!((1.5..2.5).contains(&furthest), "{furthest}");
        assert!(wider > 0.5, "the bottom widens: {wider}");
        assert!(
            (0.5..1.5).contains(&item),
            "the item rides half the bump: {item}"
        );
        assert!(motion.jelly.is_flat());
        assert_eq!(motion.offset(), Vector::ZERO);
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
        assert!((1.0..2.0).contains(&swell), "the right end swells: {swell}");
        assert!(
            (-2.0..-1.0).contains(&flat),
            "the left end flattens: {flat}"
        );
        assert!(taller > 1.05 && flatter < 0.95, "{taller} {flatter}");
        assert!(motion.jelly.is_flat());

        let mut motion = nudged(&[(2, (2, Nudge::Left))]);
        let mut leaned: f32 = 0.0;
        run(&mut motion, Instant::now(), |motion| {
            leaned = leaned.max(motion.jelly.value(Side::Top, 0.05));
        });
        assert!(leaned > 1.0, "the left end swells: {leaned}");

        let mut motion = nudged(&[(2, (2, Nudge::Up))]);
        let (mut top, mut bulge): (f32, f32) = (0.0, 0.0);
        run(&mut motion, Instant::now(), |motion| {
            top = top.max(motion.jelly.value(Side::Left, 0.0));
            bulge = bulge.max(motion.jelly.value(Side::Top, 65.0 / 200.0));
        });
        assert!(
            top > 0.5 && bulge > 1.5,
            "the top widens and bulges: {top} {bulge}"
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
            (0.85..0.98).contains(&narrowest),
            "squished a little: {narrowest}"
        );
        assert_eq!(motion.kicked(1), Squish::NONE);
        assert_eq!(one, 0.0, "only the item that nudged");
        assert!((-4.0..-1.0).contains(&two), "a few px left: {two}");
        assert_eq!(motion.item(2, 50.0), (0.0, 1.0), "and back");
        assert_eq!(motion.kicked(2), Squish::NONE, "unsquished");

        // Up and down bump the edge instead.
        let motion = nudged(&[(2, (2, Nudge::Up))]);
        assert_eq!(motion.items[1].kick.velocity, 0.0);
    }

    #[test]
    fn a_held_island_lets_nudges_pass() {
        let mut motion = nudged(&[]);
        motion.press(Point::new(10.0, 10.0));
        motion.nudges([(1, (2, Nudge::Left)), (2, (3, Nudge::Up))], center);
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
        run(&mut motion, Instant::now(), |motion| {
            assert!(motion.jelly.is_flat(), "only one edge moves");
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
