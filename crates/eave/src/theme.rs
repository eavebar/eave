//! Every visual constant in one place. Plugins can't reach any of this: they name a [`Tone`], the
//! host picks the color.
//!
//! The palette is black islands and white ink, or in the light scheme its mirror, white islands
//! and black ink, with opacity for hierarchy. Color only appears as a signal (good, warning,
//! critical) or when it comes from the content, like a cover's (D-61, D-92,
//! [docs/design.md](../../../docs/design.md)).

use std::time::Duration;

use eave_api::ui::Tone;
use iced::{Color, Font, Theme, font};

/// Height of an island, the visible part of the bar, where there's no notch. With one, the islands
/// end where it does (D-76).
pub const ISLAND_HEIGHT: u32 = 30;
/// No notch makes an island shorter than this, or its items wouldn't fit.
pub const MIN_ISLAND_HEIGHT: f32 = 20.0;
/// Space between the islands and the top, left and right screen edges. A popover keeps the same
/// distance from the sides.
pub const EDGE_GAP: u16 = 6;
/// Transparent space below the islands, so a dragged island can stretch down without being cut
/// off.
pub const DRAG_ROOM: u16 = 14;
/// How much of `DRAG_ROOM` the bar keeps windows out of. The rest of the surface hangs over the
/// top of the windows below it, outside its input region, so it never takes their clicks (D-45).
pub const RESERVED_ROOM: u16 = 6;
/// The bar's exclusive zone, and the strip that takes the pointer, with islands `island` tall
/// below an edge grown by `grow` (D-81): the grown edge, the gap, an island and the reserved room
/// below it.
pub fn bar_height(grow: f32, island: f32) -> u32 {
    (grow + f32::from(EDGE_GAP) + island + f32::from(RESERVED_ROOM)).ceil() as u32
}

/// The bar surface, with islands `island` tall below an edge grown by `grow`: the grown edge, the
/// gap, an island and all the room below it.
pub fn surface_height(grow: f32, island: f32) -> u32 {
    (grow + f32::from(EDGE_GAP) + island + f32::from(DRAG_ROOM)).ceil() as u32
}

/// The notch, and the top edge the bar grows (D-75, D-81): the screen's own black, whatever the
/// theme, since they stand for the hardware around the picture (D-92).
pub const BEZEL: Color = Color::BLACK;

/// With glass (hypr-eave), an island's ink turns dark once what's behind it is brighter than this,
/// and light again once it's dimmer than the lower one: apart, so one over something in between
/// doesn't flicker. Brightness is the luma of the frosted backdrop, 0 to 1.
pub const GLASS_TO_DARK_INK: f32 = 0.6;
pub const GLASS_TO_LIGHT_INK: f32 = 0.5;
/// The most records a frame tells the glass: as many as hypr-eave reads (`MAX_REGIONS`), well
/// inside one Wayland message.
pub const GLASS_MAX_REGIONS: usize = 64;
/// How far past an island's shape its pixels may reach while it moves, for hypr-eave
/// (`ISLAND_MARGIN`): what's further from every island changes nothing.
pub const GLASS_MARGIN: f32 = 12.0;
/// How coarse the glass's report of what's behind the bar is, in logical pixels.
pub const GLASS_REPORT_CELL: u32 = 16;
/// How far past where they rest the notch and the grown edge are kept from glass: a kick moves
/// them.
pub const GLASS_BEZEL_MARGIN: f32 = 8.0;

/// How far an island attached to a screen edge reaches past it (D-70): further than its corners
/// and anything that moves it, so the edge never shows a gap or a corner.
pub const ATTACH_OVERHANG: f32 = 32.0;

/// Horizontal padding inside each island, at its ends.
pub const ISLAND_PADDING: u16 = 14;
/// Corner radius of an island, before smoothing.
pub const ISLAND_RADIUS: f32 = 14.0;
/// How far each corner's curve blends into the straight edges (0 is a plain rounded rectangle,
/// 0.6 is the iOS look). On an island too short for the full spread, the curve is squeezed to fit
/// rather than the smoothing reduced, so a radius up to half the island's height stays smooth.
pub const ISLAND_SMOOTHING: f32 = 0.6;
/// The least an island following the screen's corners is rounded, with `ISLAND_SMOOTHING`: on a
/// screen with square corners, a hint of a squircle rather than none (D-95).
pub const MIN_ISLAND_RADIUS: f32 = 4.0;
/// A live island's padding at its ends while it has no items. With items, it's the room above
/// and below them (D-78).
pub const LIVE_ISLAND_PADDING: u16 = 4;
/// How far each side of a pressed island moves in, in logical pixels.
pub const PRESS_INSET: f32 = 1.5;
/// The most the part of a dragged island under the pointer moves, sideways and up or down. It only
/// approaches these: the further the pointer goes, the harder it pulls back.
pub const DRAG_LIMIT_X: f32 = 10.0;
pub const DRAG_LIMIT_Y: f32 = 6.0;
/// How much of a drag moves the whole island. The rest stretches the part near where it was
/// grabbed, less and less further away, and not at all past `DRAG_REACH`. Wide, so a pull bends
/// the island in one long curve rather than a dent.
pub const DRAG_FOLLOW: f32 = 0.5;
pub const DRAG_REACH: f32 = 200.0;
/// How directly a drag moves the island at first: 1 follows the pointer, lower feels heavier.
pub const DRAG_RESISTANCE: f32 = 0.5;

// The motion language (D-83, docs/design.md, Motion). Every spring an interaction plays on is
// one of three feels, each given as its response (how long one swing would take, undamped, in
// seconds) and its damping ratio (below 1 it overshoots a little, 1 is critical): what the hand
// moves rides HAND, what an event kicks rides REACT, what's going somewhere rides MORPH. A
// motion says how far it goes, in pixels; the feel works out how hard that is to kick. Turning
// these few numbers changes how the whole bar feels, together.

/// What the hand moves: the press's squeeze and the drag's stretch, after the pointer and back.
/// Damped enough that letting go only overshoots a little.
pub const HAND_RESPONSE: f32 = 0.26;
pub const HAND_DAMPING: f32 = 0.75;
/// What an event kicks out and back: a nudge's push and lift, the nudged item's kick, the jelly's
/// pokes, a popover's squish and splat. Just under critical, so it comes back without a swing you
/// can see.
pub const REACT_RESPONSE: f32 = 0.20;
pub const REACT_DAMPING: f32 = 0.92;
/// The shape and items of an island when its content changes, or it turns into a popover and
/// back. Damped close to critical, so it settles without wobbling: the squish rides on top.
pub const MORPH_RESPONSE: f32 = 0.28;
pub const MORPH_DAMPING: f32 = 0.95;
/// An island's items fading out as it turns into a popover, and the popover's content fading in
/// (and back). Quicker than the morph, and critically damped, so a fade never overshoots. Not part
/// of the language: it's a value, not a shape.
pub const FADE_RESPONSE: f32 = 0.16;
pub const FADE_DAMPING: f32 = 1.0;
/// How far every reaction goes, all together: 1 is as the peaks below say.
pub const REACT_SIZE: f32 = 0.8;
/// How hard each piece of an island's outline is pulled toward its neighbors, as a share of how
/// hard REACT pulls it back where it belongs. That pull is what spreads a poke into a smooth curve
/// ([`crate::jelly`]): higher spreads it wider, and softer.
pub const JELLY_SPREAD: f32 = 2.33;
/// How much stretching along one axis pulls the other in, per pixel or share stretched: a squished
/// thing keeps about its size.
pub const BULGE: f32 = 0.5;

/// How far a sideways nudge pushes the whole island, and an up or down one lifts it, in logical
/// pixels at the furthest.
pub const NUDGE_PUSH: f32 = 5.0;
pub const NUDGE_LIFT: f32 = 0.5;
/// How much further than its island a sideways nudge pushes the item that asked for it.
pub const NUDGE_KICK: f32 = 2.5;
/// How much a kicked item squishes per pixel it's out: narrower by this share, taller by the
/// bulge of it. About 7 % narrower at the furthest.
pub const NUDGE_KICK_SQUISH: f32 = 0.03;
/// How close an island comes to the screen's edges, in logical pixels, however it's pushed,
/// dragged or jellied (D-104): they're walls. A push that gets there bounces back off with
/// `WALL_BOUNCE` of its speed, and the side that hit squashes in by `WALL_SQUASH` of it, bulging
/// the sides next to it.
pub const WALL_GAP: f32 = 2.0;
pub const WALL_BOUNCE: f32 = 0.5;
pub const WALL_SQUASH: f32 = 0.3;
/// How long the squash of an island showing or hiding its collapsible items lasts: a nudge's
/// motion, slowed down to this.
pub const COLLAPSE_SQUASH: Duration = Duration::from_millis(450);
/// How far apart items that come or go together set off, like collapsible items shown or hidden:
/// one at a time, in order from the toggle, so the eye follows them out. Short enough that a
/// handful is all out about as soon as the island has sprung to its new width on `MORPH`.
pub const STAGGER: Duration = Duration::from_millis(45);
/// How long a collapsed item stays out after its widget nudges, from the last nudge.
pub const REVEAL_SHOWN: Duration = Duration::from_millis(2000);
/// A bar that shows on hover (D-88): how tall the strip at the top edge is that brings it out,
/// and how long it stays once the pointer has left it. A peek, on a workspace switch or a
/// nudge, keeps it out for [`REVEAL_SHOWN`].
pub const REVEAL_EDGE: u32 = 2;
pub const HIDE_AFTER: Duration = Duration::from_millis(400);
/// How far a poke takes the jelly out, in logical pixels. The bump: the edge a drop bounces
/// toward. The lean: the end (or edge) a nudged island is going toward swells and the one it's
/// leaving flattens, each way.
pub const JELLY_BUMP: f32 = 2.0;
pub const JELLY_LEAN: f32 = 1.5;
/// An up or down nudge holds a shelf out of the edge over its item (D-105): flat across the item,
/// `NUDGE_SHELF` out (`REACT_SIZE` included), its ends easing back into the edge over
/// `NUDGE_SHOULDER` each. It rides REACT out, stays while nudges keep coming, and rides it back
/// `NUDGE_HOLD` after the last one.
pub const NUDGE_SHELF: f32 = 3.0;
pub const NUDGE_SHOULDER: f32 = 28.0;
pub const NUDGE_HOLD: Duration = Duration::from_millis(250);
/// A pushed island (one sliding as a whole, because a neighbor made room for itself) piles up at
/// the end it's pushed from: that end of its top and bottom edges is poked out to `PUSH_LEAN`,
/// every frame for as long as it slides at `PUSH_FULL_SPEED` or more (less, slower), working up
/// to that over `PUSH_BUILD`, which piles up to about 1 px. It counts as sliding above
/// `PUSH_MOVING`. When it stops, the swell is poked across to the other end, `PUSH_WIGGLE` out,
/// and the jelly settles from there.
pub const PUSH_LEAN: f32 = 0.6;
pub const PUSH_WIGGLE: f32 = 1.2;
pub const PUSH_BUILD: Duration = Duration::from_millis(250);
pub const PUSH_FULL_SPEED: f32 = 600.0;
pub const PUSH_MOVING: f32 = 20.0;
/// How an opening popover gives as it sets off (D-110): wider by this share of how much further
/// it goes across than down (narrower when it goes further down), at most `OPEN_MAX` logical
/// pixels, and the other way by the bulge. Easing back as it grows.
pub const OPEN_STRETCH: f32 = 0.08;
pub const OPEN_MAX: f32 = 12.0;
/// How a shape lands (D-110), a closing popover or an island changing size: each side carries on
/// the way it was going, `LAND_CARRY` logical pixels for each pixel per second it went at its
/// fastest, at most `LAND_MAX`. The morph goes about 8.5 times as fast as how far it goes, so a
/// side that came 200 px gives about 3.4 (before `REACT_SIZE`). A side slower than `LAND_MIN` (a
/// dozen pixels) doesn't. Going out, a side stays inside the `EDGE_GAP` from the screen's sides.
pub const LAND_CARRY: f32 = 0.002;
pub const LAND_MAX: f32 = 4.0;
pub const LAND_MIN: f32 = 100.0;
/// A shape lands so its give peaks as it arrives: when every edge will be this close to where
/// it's going, in logical pixels, by the time REACT peaks.
pub const SQUISH_LAND: f32 = 1.0;

// The dormer comes out of the notch (or the top edge) as a drop (D-86): a dot at its middle
// swells into a ball, falls out of it on a neck of black that pinches off, and unfolds into the
// panel. Closing runs it back, and the notch flattens as the ball lands in it.

/// How wide the ball is, in logical pixels. It swells half out of where it comes out first.
pub const DROP_BALL: f32 = 26.0;
/// How far below where it comes out the ball's top goes before it unfolds into the panel: far
/// enough that the neck has pinched off by then.
pub const DROP_FALL: f32 = 36.0;
/// How far, from 0 to 1, the shape gets to each stop before it heads on to the next. Early, so it
/// heads on while it's still quick: it never slows down on the way, and moves as one motion.
pub const DROP_WAYPOINT: f32 = 0.4;
/// How much the ball is squeezed in from the sides as it sets off to unfold, and taller by the
/// bulge, in logical pixels.
pub const DROP_SQUISH: f32 = 4.0;
/// How far apart the drop and where it comes out still flow together, in logical pixels: the
/// neck is a wide, smooth bell when the drop is right under it, which narrows as it falls and
/// pinches off about half this far away.
pub const NECK_BLEND: f32 = 56.0;
/// How far each way from the drop's middle the neck can spread along the notch's bottom: short of
/// its lower corners.
pub const NECK_SPREAD: f32 = 76.0;
/// How far a drop lurches as its neck lets go of it or catches it, and as it falls into place,
/// in logical pixels: a nudge up or down, bigger.
pub const DROP_BOUNCE: f32 = 3.0;
/// How long a drop's bounce lasts: a nudge's motion slowed down to this, as a paced nudge is.
/// Slower than a nudge's own, because a drop is heavier than a workspace mark.
pub const DROP_BOUNCE_LASTING: Duration = Duration::from_millis(420);
/// How far the notch pinches in from each side as the drop leaves it, and flattens as it lands back,
/// in logical pixels. Never past its grown room (D-75): it doesn't shrink smaller than it is.
pub const NOTCH_PINCH: f32 = 4.0;
pub const NOTCH_FLATTEN: f32 = 3.0;
/// Without a notch, the grown top edge (D-81) does it where the drop comes out: pulled down in a
/// narrow bump as it leaves, dented up in a wide one, no further than its grown room, as it lands.
/// How deep, and how wide end to end, in logical pixels.
pub const EDGE_PULL: f32 = 4.0;
pub const EDGE_PULL_WIDTH: f32 = 48.0;
pub const EDGE_DENT: f32 = 3.0;
pub const EDGE_DENT_WIDTH: f32 = 140.0;
/// Without a notch, a click this wide in the middle of the top, above the islands, brings the
/// dormer out, as one on the notch does (D-106).
pub const EDGE_CLICK_WIDTH: f32 = 240.0;
/// How far a press may move before it's a drag, not a click, in logical pixels.
pub const CLICK_SLOP: f32 = 4.0;
/// The furthest the outline reaches out of the island: it stays inside `EDGE_GAP` above and at
/// the sides, and `DRAG_ROOM` below.
pub const NUDGE_BUMP_ROOM: f32 = 4.0;
/// Horizontal padding inside each item, so its click target is wider than its text.
pub const ITEM_PADDING: u16 = 6;
pub const ITEM_SPACING: u32 = 4;
/// A divider between items: a line this wide and tall.
pub const DIVIDER_WIDTH: f32 = 1.0;
pub const DIVIDER_HEIGHT: f32 = 14.0;
/// Space between islands in the same zone.
pub const ISLAND_SPACING: u32 = EDGE_GAP as u32;

/// Space between a popover's edge and its content.
pub const PANEL_PADDING: u16 = 12;
pub const SPACING: u32 = 8;
pub const GRID_CELL: f32 = 28.0;
pub const BUTTON_RADIUS: f32 = 6.0;

/// Every popover is this wide, padding included, so rows can push things to their far end and
/// moving between a popover's pages only changes its height (D-40).
pub const PANEL_WIDTH: f32 = 320.0;
/// A popover that asked to be wide ([`eave_api::ui::Panel::wide`]), for reading: a conversation.
pub const PANEL_WIDE_WIDTH: f32 = 480.0;
/// The tallest a [`eave_api::ui::scroll`] grows in a popover before it scrolls.
pub const SCROLL_MAX_HEIGHT: f32 = 520.0;
/// A widget's dialog, in the middle of the screen (D-90): wider than a wide popover, and its
/// scrolls grow taller.
pub const DIALOG_WIDTH: f32 = 640.0;
pub const DIALOG_SCROLL_MAX_HEIGHT: f32 = 600.0;
/// How much of the screen a dialog keeps beside its scroll: the rows above and below it, its
/// padding, and a margin at the top and bottom of the screen. A short screen's scroll gets less.
pub const DIALOG_CHROME: f32 = 260.0;
/// Markdown: the room between blocks.
pub const BLOCK_SPACING: u32 = 6;
/// A code block numbers its lines once it has this many.
pub const CODE_NUMBERED: usize = 4;

/// A timeline: the room left of its steps where the line runs, the line, and the marks on it.
/// Marks sit level with the middle of a step's first line of text.
pub const RAIL_GUTTER: f32 = 22.0;
pub const RAIL_LINE: f32 = 1.0;
pub const RAIL_MARK: f32 = 9.0;
pub const RAIL_NOTE: f32 = 5.0;
pub const RAIL_MARK_Y: f32 = 9.5;
/// Below each step, before the next.
pub const RAIL_STEP_GAP: f32 = 10.0;
/// A mark under way: a ring, with a faint halo around it. It doesn't pulse: a loop redraws the
/// whole popover (D-64).
/// A done mark and a note: on the line, a little brighter than it.
/// A notification card's close button: smaller than a popover's icon buttons, to sit beside the
/// app's name.
pub const NOTICE_CLOSE: f32 = 22.0;
/// How many lines of a notification's body show before it's cut off, fading out.
pub const NOTICE_BODY_LINES: f32 = 4.0;
/// A notification's own picture, beside its summary and body.
pub const NOTICE_PICTURE: f32 = 40.0;
/// A notification's wide picture, under its text and as wide as the card: at most this tall,
/// cropped from the middle beyond that, and its corners' radius.
pub const NOTICE_IMAGE_MAX: f32 = 180.0;
pub const NOTICE_IMAGE_RADIUS: f32 = 8.0;
/// The dot before a critical notification's app name.
pub const NOTICE_DOT: f32 = 8.0;
/// A notification asking several questions: a dot for each, the one asked now lit.
pub const STEP_DOT: f32 = 5.0;
/// A check box beside an option, when several can be picked: a ring at rest, a square in the
/// accent fill with a check once picked.
pub const CHECK_SIZE: f32 = 16.0;
/// How many cards the panel of notifications shows at once. The rest wait, under a line that
/// says how many, which opens their popover (D-85).
pub const NOTICES_SHOWN: usize = 3;
/// How many earlier notifications of its thread a card lists under itself, a line each.
pub const NOTICE_STACK_LINES: usize = 3;
/// Within a card: below the line it's from, between its text and its picture, and above its
/// reply field and actions. The summary sits tight on its body.
pub const NOTICE_GAP: f32 = 8.0;
pub const NOTICE_PICTURE_GAP: f32 = 12.0;
pub const NOTICE_REPLY_GAP: f32 = 12.0;
pub const NOTICE_SUMMARY_GAP: f32 = 2.0;
/// The bar across the top of a notification that shows its time left on screen.
pub const COUNTDOWN_HEIGHT: f32 = 2.0;
/// The shortest time between two frames of that bar: it moves a pixel at a time, but no faster
/// than this. 30 fps, like the battery's gulp: smooth enough for motion this slow, and half the
/// redraws of 60 (D-47).
pub const COUNTDOWN_STEP: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// The room above and below the line between two notifications.
pub const NOTICE_SPACING: u32 = 10;
/// A list row with one line, and with a subtitle under it.
pub const ROW_HEIGHT: f32 = 36.0;
pub const ROW_HEIGHT_TALL: f32 = 46.0;
/// Space between list rows.
pub const ROW_SPACING: u32 = 2;
/// A list taller than this many one-line rows scrolls.
pub const LIST_MAX_ROWS: f32 = 6.0;
/// Square buttons holding just an icon, and the height of text buttons.
pub const ICON_BUTTON: f32 = 28.0;
/// A prominent icon button (play and pause): a disc this wide, in the accent fill.
pub const PROMINENT_BUTTON: f32 = 36.0;
pub const BUTTON_HEIGHT: f32 = 30.0;
/// A slider is a line with no knob: this thick at rest, `SLIDER_RAIL_HOVER` under the pointer or
/// while dragged, easing between the two over `SLIDER_GROW`. `SLIDER_HEIGHT` is the room it takes,
/// and what the pointer can grab. A click eases the fill over `SLIDER_EASE`.
pub const SLIDER_RAIL: f32 = 2.0;
pub const SLIDER_RAIL_HOVER: f32 = 4.0;
pub const SLIDER_HEIGHT: f32 = 16.0;
pub const SLIDER_GROW: Duration = Duration::from_millis(180);
pub const SLIDER_EASE: Duration = Duration::from_millis(220);
pub const METER_HEIGHT: f32 = 6.0;
/// How long a meter takes to ease to a new value.
pub const METER_EASE: Duration = Duration::from_millis(400);
/// A toggle's pill, its knob, and how long the knob takes to slide over.
pub const TOGGLE_WIDTH: f32 = 34.0;
pub const TOGGLE_HEIGHT: f32 = 20.0;
pub const TOGGLE_KNOB: f32 = 14.0;
pub const TOGGLE_SLIDE: Duration = Duration::from_millis(160);
/// A text field's height.
pub const FIELD_HEIGHT: f32 = 34.0;
/// The ring of a mark (today in a calendar), how thick its line is, and the height of a
/// calendar's rows.
pub const MARK_SIZE: f32 = 28.0;
pub const MARK_RING: f32 = 1.5;
pub const GRID_ROW: f32 = 32.0;
/// The corners of a badge, and of a keycap.
pub const BADGE_RADIUS: f32 = 4.0;
/// A keycap ([`eave_api::ui::keys`]) is filled like a button, faint enough that its key reads
/// first. Its padding makes one digit about square; caps of one shortcut sit this far apart.
pub const KEYCAP_PADDING: [u16; 2] = [1, 5];
pub const KEYCAP_SPACING: u32 = 3;
/// A count badge ([`eave_api::ui::counted`]): as tall as it is wide for one digit, so one digit
/// makes a circle and two a pill.
pub const PIP_HEIGHT: f32 = 14.0;
/// Its number, and the room on each side of it once there's more than one digit.
pub const PIP_TEXT_SIZE: f32 = SMALL_SIZE - 1.0;
pub const PIP_PADDING: f32 = 4.0;
/// How far the badge reaches back over what it counts, and how far its middle sits below that
/// thing's top: enough to sit on its corner without covering it.
pub const PIP_OVERLAP: f32 = 7.0;
pub const PIP_DROP: f32 = 3.0;
/// Tabs: a pill-shaped track with a mark sliding behind the current tab, inset by `TAB_INSET`.
pub const TAB_HEIGHT: f32 = 28.0;
pub const TAB_INSET: f32 = 2.0;
/// Switching tabs hands the page over: the old one slides `TAB_SLIDE` away from the new tab as it
/// fades out, then the new one slides in from that side, over `TAB_FADE` in all. Further than an
/// alternate's slide, since pages are wide.
pub const TAB_FADE: Duration = Duration::from_millis(240);
pub const TAB_SLIDE: f32 = 16.0;
/// The width at the end of a long title that fades out instead of being cut.
pub const TEXT_FADE: f32 = 16.0;
/// How tall the fade is at the foot of text cut off below, like a notification's body.
pub const FOLD_FADE: f32 = 24.0;

/// How long an alternate takes to fade from one option to the next: out, then in.
pub const ALTERNATE_FADE: Duration = Duration::from_millis(240);
/// How far an option slides as it fades, when the pointer moving onto or off its item set it off:
/// sideways, or up and down (less, to stay inside the island).
pub const ALTERNATE_SLIDE_X: f32 = 6.0;
pub const ALTERNATE_SLIDE_Y: f32 = 4.0;
/// How long the pointer rests on a growing alternate before it changes.
pub const HOVER_REST: Duration = Duration::from_millis(200);
/// How long a counter takes to count to a new number, through every one in between.
pub const COUNTER_COUNT: Duration = Duration::from_millis(320);
/// How long a switcher's mark takes to reach a newly current option.
pub const SWITCHER_SLIDE: Duration = Duration::from_millis(180);
/// While charging, the battery gulps like a bottle being poured: a few gulps, then a long rest
/// with no frames at all. One gulp, and the time from one burst of gulps to the next.
pub const BATTERY_GULP: Duration = Duration::from_millis(380);
pub const BATTERY_GULPS: u32 = 3;
pub const BATTERY_GULP_EVERY: Duration = Duration::from_secs(8);
/// How long each of a [`eave_api::ui::gulp`]'s gulps takes, and how much it grows at the height
/// of one. Slow and steady, unlike the battery icon's quick bursts.
pub const GULP: Duration = Duration::from_millis(700);
pub const GULP_SWELL: f32 = 0.08;
/// How often the charging battery is redrawn: 30 frames a second, cheaper than the compositor's
/// rate and smooth enough for motion this small.
pub const BATTERY_FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// How long a sprite shows each frame, and how many times it plays its loop when it shows up or
/// changes before it rests ([D-64](../../../docs/decisions.md)).
pub const SPRITE_FRAME: Duration = Duration::from_millis(250);
pub const SPRITE_PLAYS: usize = 2;
/// A sprite's height in the bar and in a popover's rows; its width follows its frames. Pixel art
/// scales by whole screen pixels at scale 2: 10 rows of art are 3 and 4 pixels a row.
pub const SPRITE_HEIGHT: f32 = 15.0;
pub const SPRITE_HEIGHT_PANEL: f32 = 20.0;
/// A primary press held at least this long is a long press: it plays with the island, but the
/// item's action and popover don't fire on release.
pub const LONG_PRESS: Duration = Duration::from_millis(400);
/// Touchpad scrolling distance, in logical pixels, that counts as one scroll step.
pub const SCROLL_STEP: f32 = 30.0;
/// A flung list slows like this: its speed falls to about a third every `SCROLL_GLIDE_DECAY`
/// seconds (iOS's feel), so it covers speed × this in all. Not a spring: nothing pulls it
/// anywhere, it just runs out ([D-77](../../../docs/decisions.md)).
pub const SCROLL_GLIDE_DECAY: f32 = 0.325;
/// Slower than this, in logical pixels a second, a fling stops, and a lift doesn't start one.
pub const SCROLL_GLIDE_MIN: f32 = 20.0;
/// A wheel step eases to where it goes, two thirds of the way every this many seconds: settled
/// in about 120 ms, quick enough that the wheel still feels direct. The eases above run a set
/// time from one value to another, but steps add up while one is under way.
pub const SCROLL_WHEEL_EASE: f32 = 0.04;

pub const TEXT_SIZE: f32 = 14.0;
/// Height of an icon, and width of the square ones.
pub const ICON_SIZE: f32 = 16.0;
/// The battery lies on its side, so it is wider than the others.
pub const BATTERY_ICON_WIDTH: f32 = 24.0;
/// A cover's side when it's a popover's main picture, and its corner radius. At icon size its
/// corners are rounded like this, but less.
pub const COVER_SIZE: f32 = 128.0;
pub const COVER_RADIUS: f32 = 12.0;
pub const COVER_ICON_RADIUS: f32 = 3.0;
pub const ICON_STROKE: f32 = 1.5;
/// How visible the parts of an icon that are off stay, against the parts that are on.
pub const ICON_OFF_ALPHA: f32 = 0.3;
/// The opacity of an app icon in `Tone::Dim`, like a workspace you're not on.
pub const DIM_APP_ICON: f32 = 0.45;
pub const HEADING_SIZE: f32 = 17.0;
pub const HEADING_FONT: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};
/// Section captions, subtitles and badges.
pub const SMALL_SIZE: f32 = 12.0;
/// A selected row's title, section captions, and text buttons.
pub const STRONG_FONT: Font = Font {
    weight: font::Weight::Semibold,
    ..Font::DEFAULT
};
/// Code in Markdown.
pub const MONO_FONT: Font = Font::MONOSPACE;
/// The large number of a popover ([`eave_api::ui::display`]).
pub const DISPLAY_SIZE: f32 = 32.0;

/// Sans-serif families, most wanted first. iced's text engine asks for one it names itself (Open
/// Sans); when that isn't installed, plain text still finds a sans face, but bold and semibold
/// fall back to whatever has that weight, often a serif. So the host names one that is there.
pub const SANS_FAMILIES: &[&str] = &[
    "Open Sans",
    "Noto Sans",
    "Inter",
    "Cantarell",
    "DejaVu Sans",
    "Liberation Sans",
    "Roboto",
    "Ubuntu",
    "FreeSans",
];

/// Which way round the palette is: black islands and white ink, or white islands and black ink
/// (D-92).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Scheme {
    #[default]
    Dark,
    Light,
}

impl Scheme {
    pub fn palette(self) -> &'static Palette {
        match self {
            Scheme::Dark => &DARK,
            Scheme::Light => &LIGHT,
        }
    }

    /// The iced theme the bar runs with, which is how the widgets that draw themselves find the
    /// palette ([`Palette::of`]).
    pub fn theme(self) -> Theme {
        match self {
            Scheme::Dark => Theme::Dark,
            Scheme::Light => Theme::Light,
        }
    }
}

/// Every color the host draws with. Two of them: [`DARK`] and [`LIGHT`], each the other's mirror.
#[derive(Debug, PartialEq)]
pub struct Palette {
    pub scheme: Scheme,
    /// The islands, popovers and the notch they come out of.
    pub island_bg: Color,
    pub hover_bg: Color,
    pub pressed_bg: Color,
    pub separator: Color,
    /// Behind the current option of a switcher (the active workspace): the current thing is
    /// marked with light, not a hue.
    pub switcher_mark: Color,
    /// A divider between items in an island.
    pub divider: Color,
    /// Markdown: behind code, and the bar beside a quote.
    pub code_bg: Color,
    pub quote_bar: Color,
    /// Code is content, so it's colored (D-65): soft hues, all about as light, so none of them
    /// shouts and none reads as a signal. Comments and punctuation sit back.
    pub code_plain: Color,
    pub code_keyword: Color,
    pub code_type: Color,
    pub code_function: Color,
    pub code_string: Color,
    pub code_number: Color,
    pub code_comment: Color,
    pub code_punctuation: Color,
    /// A code block's line numbers, and the marks in a diff's margin.
    pub code_gutter: Color,
    /// Behind a diff's added and removed lines: the signal colors, faint, so the code on them
    /// reads.
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_added: Color,
    pub diff_removed: Color,
    /// A timeline's mark under way: the faint halo around its ring.
    pub rail_halo: Color,
    /// A done mark and a note: on the line, a little stronger than it.
    pub rail_done: Color,
    /// A check box at rest: the ink of the countdown.
    pub check_ring: Color,
    /// The bar across the top of a notification that shows its time left on screen.
    pub countdown: Color,
    /// Behind the list row in use, like the current option of a switcher but softer, since a row
    /// is bigger.
    pub selected_bg: Color,
    /// A text button at rest in a popover, so it reads as a button and not a label. Keycaps and
    /// the tabs' track are filled with it too.
    pub button_bg: Color,
    /// What's drawn on the accent fill: a prominent button's text or icon, a toggle's knob when
    /// on.
    pub on_accent: Color,
    /// A prominent button's fill, a toggle's pill when on, a picked check box: the ink's color.
    pub accent_fill: Color,
    /// The empty part of a slider, toggle or meter.
    pub track: Color,
    pub meter_track: Color,
    /// A text field: its background, edge, focused edge, placeholder text and selection.
    pub field_bg: Color,
    pub field_border: Color,
    pub field_focus: Color,
    pub placeholder: Color,
    pub selection: Color,
    /// A badge's outline.
    pub badge_border: Color,
    /// A scrolling list's scrollbar.
    pub scroller: Color,
    /// The mark behind the current tab: a little stronger than a selected row's fill, since it
    /// sits on the track.
    pub tab_mark: Color,
    /// The ink, by [`Tone`].
    pub normal: Color,
    pub dim: Color,
    pub accent: Color,
    pub warning: Color,
    pub critical: Color,
    pub good: Color,
}

impl Palette {
    /// The palette of the theme iced draws with: how widgets that draw themselves, and style
    /// functions, find it.
    pub fn of(theme: &Theme) -> &'static Palette {
        match theme {
            Theme::Light => &LIGHT,
            _ => &DARK,
        }
    }

    pub fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Normal => self.normal,
            Tone::Dim => self.dim,
            Tone::Accent => self.accent,
            Tone::Warning => self.warning,
            Tone::Critical => self.critical,
            Tone::Good => self.good,
        }
    }

    pub fn keycap_bg(&self) -> Color {
        self.button_bg
    }

    pub fn tab_track(&self) -> Color {
        self.button_bg
    }
}

/// The overlays: white at `alpha` on the dark scheme's islands, black on the light one's.
const fn white(alpha: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, alpha)
}

const fn black(alpha: f32) -> Color {
    Color::from_rgba(0.0, 0.0, 0.0, alpha)
}

const fn grey(value: f32) -> Color {
    Color::from_rgb(value, value, value)
}

/// Black islands and white ink.
pub static DARK: Palette = Palette {
    scheme: Scheme::Dark,
    island_bg: Color::BLACK,
    hover_bg: white(0.08),
    pressed_bg: white(0.14),
    separator: white(0.10),
    switcher_mark: white(0.14),
    divider: white(0.18),
    code_bg: white(0.07),
    quote_bar: white(0.18),
    code_plain: Color::from_rgb8(0xD8, 0xDA, 0xE0),
    code_keyword: Color::from_rgb8(0xC7, 0xAE, 0xF0),
    code_type: Color::from_rgb8(0x9F, 0xD3, 0xDC),
    code_function: Color::from_rgb8(0xE9, 0xD7, 0xA6),
    code_string: Color::from_rgb8(0xB7, 0xD2, 0xA6),
    code_number: Color::from_rgb8(0xE8, 0xB8, 0x8F),
    code_comment: Color::from_rgb8(0x6C, 0x74, 0x80),
    code_punctuation: Color::from_rgb8(0x8B, 0x8F, 0x98),
    code_gutter: Color::from_rgb8(0x4A, 0x4A, 0x4A),
    diff_added_bg: Color::from_rgba(0.188, 0.820, 0.345, 0.12),
    diff_removed_bg: Color::from_rgba(1.0, 0.271, 0.227, 0.12),
    diff_added: Color::from_rgb8(0x30, 0xD1, 0x58),
    diff_removed: Color::from_rgb8(0xFF, 0x45, 0x3A),
    rail_halo: white(0.12),
    rail_done: grey(0.33),
    check_ring: white(0.35),
    countdown: white(0.35),
    selected_bg: white(0.10),
    button_bg: white(0.08),
    on_accent: Color::BLACK,
    accent_fill: Color::WHITE,
    track: white(0.14),
    meter_track: white(0.10),
    field_bg: white(0.06),
    field_border: white(0.10),
    field_focus: white(0.50),
    placeholder: white(0.30),
    selection: white(0.25),
    badge_border: white(0.18),
    scroller: white(0.20),
    tab_mark: white(0.16),
    // White ink at 94 % and 50 %: brightness, not hue, sets the hierarchy. Written as the grey it
    // makes on a black island, so overlapping strokes in an icon don't show seams.
    normal: grey(0.94),
    dim: grey(0.50),
    // The thing to notice is the brightest thing there.
    accent: Color::WHITE,
    // Signals: vivid, because they're the only color on screen.
    warning: Color::from_rgb8(0xFF, 0xB0, 0x20),
    critical: Color::from_rgb8(0xFF, 0x45, 0x3A),
    good: Color::from_rgb8(0x30, 0xD1, 0x58),
};

/// The mirror: white islands and black ink, every overlay black at the alpha the dark one is
/// white. Hues that sit on the island get darker, so they read on white.
pub static LIGHT: Palette = Palette {
    scheme: Scheme::Light,
    island_bg: Color::WHITE,
    hover_bg: black(0.06),
    pressed_bg: black(0.11),
    separator: black(0.10),
    switcher_mark: black(0.10),
    divider: black(0.16),
    code_bg: black(0.05),
    quote_bar: black(0.16),
    code_plain: Color::from_rgb8(0x2A, 0x2D, 0x33),
    code_keyword: Color::from_rgb8(0x7A, 0x4F, 0xC4),
    code_type: Color::from_rgb8(0x1F, 0x7A, 0x88),
    code_function: Color::from_rgb8(0x8A, 0x66, 0x10),
    code_string: Color::from_rgb8(0x4A, 0x7A, 0x32),
    code_number: Color::from_rgb8(0xA8, 0x5A, 0x1F),
    code_comment: Color::from_rgb8(0x90, 0x97, 0xA0),
    code_punctuation: Color::from_rgb8(0x6E, 0x72, 0x79),
    code_gutter: Color::from_rgb8(0xB8, 0xB8, 0xB8),
    diff_added_bg: Color::from_rgba(0.141, 0.541, 0.239, 0.12),
    diff_removed_bg: Color::from_rgba(0.843, 0.0, 0.082, 0.10),
    diff_added: Color::from_rgb8(0x24, 0x8A, 0x3D),
    diff_removed: Color::from_rgb8(0xD7, 0x00, 0x15),
    rail_halo: black(0.10),
    rail_done: grey(0.67),
    check_ring: black(0.35),
    countdown: black(0.35),
    selected_bg: black(0.07),
    button_bg: black(0.06),
    on_accent: Color::WHITE,
    accent_fill: Color::BLACK,
    track: black(0.12),
    meter_track: black(0.08),
    field_bg: black(0.04),
    field_border: black(0.12),
    field_focus: black(0.50),
    placeholder: black(0.35),
    selection: black(0.18),
    badge_border: black(0.18),
    scroller: black(0.22),
    tab_mark: black(0.12),
    // Black ink at 88 % and 50 %, written as the grey it makes on a white island.
    normal: grey(0.12),
    dim: grey(0.50),
    accent: Color::BLACK,
    // Signals, deep enough to read on white.
    warning: Color::from_rgb8(0xC2, 0x5E, 0x00),
    critical: Color::from_rgb8(0xD7, 0x00, 0x15),
    good: Color::from_rgb8(0x24, 0x8A, 0x3D),
};
