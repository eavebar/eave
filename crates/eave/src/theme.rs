//! Every visual constant in one place. Plugins can't reach any of this: they name a [`Tone`], the
//! host picks the color.
//!
//! The palette is black islands and white ink, with opacity for hierarchy. Color only appears as a
//! signal (good, warning, critical) or when it comes from the content, like a cover's (D-61,
//! [docs/design.md](../../../docs/design.md)).

use std::time::Duration;

use eave_api::ui::Tone;
use iced::{Color, Font, font};

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
/// The bar's exclusive zone, and the strip that takes the pointer, with islands `island` tall: an
/// island, the gap above it and the reserved room below it.
pub fn bar_height(island: f32) -> u32 {
    (f32::from(EDGE_GAP) + island + f32::from(RESERVED_ROOM)).ceil() as u32
}

/// The bar surface, with islands `island` tall: an island, the gap above it and all the room
/// below it.
pub fn surface_height(island: f32) -> u32 {
    (f32::from(EDGE_GAP) + island + f32::from(DRAG_ROOM)).ceil() as u32
}

/// How far an island attached to a screen edge reaches past it (D-70): further than its corners
/// and anything that moves it, so the edge never shows a gap or a corner.
pub const ATTACH_OVERHANG: f32 = 32.0;

pub const ISLAND_BG: Color = Color::BLACK;
pub const HOVER_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
pub const PRESSED_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.14);
pub const SEPARATOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
/// Behind the current option of a switcher (the active workspace): the current thing is marked
/// with light, not a hue.
pub const SWITCHER_MARK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.14);

/// Horizontal padding inside each island, at its ends.
pub const ISLAND_PADDING: u16 = 14;
/// Corner radius of an island, before smoothing.
pub const ISLAND_RADIUS: f32 = 14.0;
/// How far each corner's curve blends into the straight edges (0 is a plain rounded rectangle,
/// 0.6 is the iOS look). On an island too short for the full spread, the curve is squeezed to fit
/// rather than the smoothing reduced, so a radius up to half the island's height stays smooth.
pub const ISLAND_SMOOTHING: f32 = 0.6;
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
/// The spring a drag pulls the island on, and brings it back on: softer than the press, and
/// damped enough that letting go only overshoots a little.
pub const DRAG_STIFFNESS: f32 = 220.0;
pub const DRAG_DAMPING: f32 = 0.7;
/// How directly a drag moves the island at first: 1 follows the pointer, lower feels heavier.
pub const DRAG_RESISTANCE: f32 = 0.5;
/// The spring that moves an island back into place: stiffness, and damping ratio (below 1 it
/// overshoots a little before settling).
pub const SPRING_STIFFNESS: f32 = 600.0;
pub const SPRING_DAMPING: f32 = 0.5;
/// The spring an island's shape and items follow when its content changes, or it turns into a
/// popover and back. Damped close to critical, so it settles without wobbling.
pub const MORPH_STIFFNESS: f32 = 380.0;
pub const MORPH_DAMPING: f32 = 0.9;
/// The spring an island's items fade out on as it turns into a popover, and the popover's content
/// fades in on (and back). Critically damped, so a fade never overshoots.
pub const FADE_STIFFNESS: f32 = 1500.0;
pub const FADE_DAMPING: f32 = 1.0;
/// The spring a popover's squish and splat ride on, on top of its morph. Just under critical, so one kick
/// squashes the shape and it eases back, without a visible bounce.
pub const SQUISH_STIFFNESS: f32 = 250.0;
pub const SQUISH_DAMPING: f32 = 0.75;
/// How hard an opening popover is squeezed in from the sides as it sets off, per second, as a
/// share of the panel's width: about 5 % narrower at the peak (and taller, by the bulge), easing
/// back as it grows.
pub const SQUISH_OPEN_SQUEEZE: f32 = 2.0;
/// How hard a closing popover's island splats against the top, in logical pixels per second:
/// about 1.5 px flatter and 3.5 px wider at the peak, before the bulge. Wider stays inside the
/// `EDGE_GAP` from the screen's sides.
pub const SQUISH_CLOSE_FLATTEN: f32 = 50.0;
pub const SQUISH_CLOSE_WIDEN: f32 = 125.0;
/// How close, in logical pixels, a closing popover's bottom edge comes to its island's before it
/// splats. The splat peaks about four frames after, as the shape arrives.
pub const SQUISH_LAND: f32 = 5.0;
/// How much stretching along one axis pulls the other in, per pixel stretched.
pub const SQUISH_BULGE: f32 = 0.5;
/// The spring a nudged island moves back on: damped nearly to critical, so it comes
/// back without swinging past.
pub const NUDGE_STIFFNESS: f32 = 600.0;
pub const NUDGE_DAMPING: f32 = 0.85;
/// How fast a nudge sets an island off, in logical pixels per second. A push sideways peaks at
/// about 5 px. Up or down, the whole island moves about half a pixel.
pub const NUDGE_PUSH_SPEED: f32 = 300.0;
pub const NUDGE_LIFT_SPEED: f32 = 30.0;
/// How fast a sideways nudge sets off the item that asked for it, on top of its island's push:
/// about 2.5 px further, and back.
pub const NUDGE_KICK_SPEED: f32 = 150.0;
/// How much a kicked item squishes per pixel it's out: narrower by this much, taller by half.
/// About 7 % narrower at the furthest.
pub const NUDGE_KICK_SQUISH: f32 = 0.03;
/// About how long a nudge takes to settle: a push is back within 5 % of its peak by then. A
/// nudge asked to last longer (`Effect::nudge_lasting`) plays the same motion slowed down by as
/// much.
pub const NUDGE_LASTING: Duration = Duration::from_millis(190);
/// The jelly an island's outline is made of ([`crate::jelly`]): how hard each piece of the edge
/// is pulled back where it belongs, its damping ratio (just under critical, so it settles without
/// wobbling), and how hard it's pulled toward its neighbors, which is what smooths a poke out.
pub const JELLY_STIFFNESS: f32 = 600.0;
pub const JELLY_DAMPING: f32 = 0.85;
pub const JELLY_COUPLING: f32 = 1400.0;
/// How hard a nudge pokes the jelly, outward in logical pixels per second. The bump: the edge
/// over the nudged item, going up or down, about 2 px out at its peak. The lean: the end (or
/// edge) the island is going toward swells and the one it's leaving flattens, about 1.5 px each
/// way.
pub const JELLY_BUMP: f32 = 150.0;
pub const JELLY_LEAN: f32 = 120.0;
/// A pushed island (one sliding as a whole, because a neighbor made room for itself) piles up at
/// the end it's pushed from: that end of its top and bottom edges swells out, kicked up to this
/// many logical pixels per second for as long as it slides at `PUSH_FULL_SPEED` or more (less,
/// slower), working up to that over `PUSH_BUILD`. It counts as sliding above `PUSH_MOVING`. When
/// it stops, the swell is kicked across to the other end, at up to `PUSH_WIGGLE`, and the jelly
/// settles from there. About 1 px at most each way.
pub const PUSH_LEAN: f32 = 45.0;
/// How long the squash of an island showing or hiding its collapsible items lasts: a nudge's
/// motion, slowed down to this.
pub const COLLAPSE_SQUASH: Duration = Duration::from_millis(450);
/// How long a collapsed item stays out after its widget nudges, from the last nudge.
pub const REVEAL_SHOWN: Duration = Duration::from_millis(2000);
pub const PUSH_BUILD: Duration = Duration::from_millis(250);
pub const PUSH_WIGGLE: f32 = 90.0;
pub const PUSH_FULL_SPEED: f32 = 600.0;
pub const PUSH_MOVING: f32 = 20.0;
/// How wide a nudge's bump in an island's edge is, end to end.
pub const NUDGE_BUMP_WIDTH: f32 = 176.0;
/// The furthest the outline reaches out of the island: it stays inside `EDGE_GAP` above and at
/// the sides, and `DRAG_ROOM` below.
pub const NUDGE_BUMP_ROOM: f32 = 4.0;
/// Horizontal padding inside each item, so its click target is wider than its text.
pub const ITEM_PADDING: u16 = 6;
pub const ITEM_SPACING: u32 = 4;
/// A divider between items: a line this wide and tall, in this color.
pub const DIVIDER_WIDTH: f32 = 1.0;
pub const DIVIDER_HEIGHT: f32 = 14.0;
pub const DIVIDER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.18);
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
/// Markdown: behind code, the bar beside a quote, and the room between blocks.
pub const CODE_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.07);
pub const QUOTE_BAR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.18);
pub const BLOCK_SPACING: u32 = 6;
/// Code is content, so it's colored (D-65): soft hues, all about as light, so none of them
/// shouts and none reads as a signal. Comments and punctuation sit back.
pub const CODE_PLAIN: Color = Color::from_rgb8(0xD8, 0xDA, 0xE0);
pub const CODE_KEYWORD: Color = Color::from_rgb8(0xC7, 0xAE, 0xF0);
pub const CODE_TYPE: Color = Color::from_rgb8(0x9F, 0xD3, 0xDC);
pub const CODE_FUNCTION: Color = Color::from_rgb8(0xE9, 0xD7, 0xA6);
pub const CODE_STRING: Color = Color::from_rgb8(0xB7, 0xD2, 0xA6);
pub const CODE_NUMBER: Color = Color::from_rgb8(0xE8, 0xB8, 0x8F);
pub const CODE_COMMENT: Color = Color::from_rgb8(0x6C, 0x74, 0x80);
pub const CODE_PUNCTUATION: Color = Color::from_rgb8(0x8B, 0x8F, 0x98);
/// A code block's line numbers, and the marks in a diff's margin.
pub const CODE_GUTTER: Color = Color::from_rgb8(0x4A, 0x4A, 0x4A);
/// A code block numbers its lines once it has this many.
pub const CODE_NUMBERED: usize = 4;
/// Behind a diff's added and removed lines: the signal colors, faint, so the code on them reads.
pub const DIFF_ADDED_BG: Color = Color::from_rgba(0.188, 0.820, 0.345, 0.12);
pub const DIFF_REMOVED_BG: Color = Color::from_rgba(1.0, 0.271, 0.227, 0.12);
pub const DIFF_ADDED: Color = Color::from_rgb8(0x30, 0xD1, 0x58);
pub const DIFF_REMOVED: Color = Color::from_rgb8(0xFF, 0x45, 0x3A);

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
pub const RAIL_HALO: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.12);
/// A done mark and a note: on the line, a little brighter than it.
pub const RAIL_DONE: Color = Color::from_rgb(0.33, 0.33, 0.33);
/// A notification card's close button: smaller than a popover's icon buttons, to sit beside the
/// app's name.
pub const NOTICE_CLOSE: f32 = 22.0;
/// How many lines of a notification's body show before it's cut off, fading out.
pub const NOTICE_BODY_LINES: f32 = 4.0;
/// A notification's own picture, beside its summary and body.
pub const NOTICE_PICTURE: f32 = 40.0;
/// Within a card: below the line it's from, between its text and its picture, and above its
/// reply field and actions. The summary sits tight on its body.
pub const NOTICE_GAP: f32 = 8.0;
pub const NOTICE_PICTURE_GAP: f32 = 12.0;
pub const NOTICE_REPLY_GAP: f32 = 12.0;
pub const NOTICE_SUMMARY_GAP: f32 = 2.0;
/// The bar across the top of a notification that shows its time left on screen.
pub const COUNTDOWN_HEIGHT: f32 = 2.0;
pub const COUNTDOWN: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.35);
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
/// Behind the list row in use, like the current option of a switcher but softer, since a row is
/// bigger.
pub const SELECTED_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
/// Square buttons holding just an icon, and the height of text buttons.
pub const ICON_BUTTON: f32 = 28.0;
/// A prominent icon button (play and pause): a white disc this wide.
pub const PROMINENT_BUTTON: f32 = 36.0;
pub const BUTTON_HEIGHT: f32 = 30.0;
/// A text button at rest in a popover, so it reads as a button and not a label.
pub const BUTTON_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
/// What's drawn on a white fill: a prominent button's text or icon, a toggle's knob when on.
pub const ON_ACCENT: Color = Color::BLACK;
/// A prominent button's fill, and a toggle's pill when on.
pub const ACCENT_FILL: Color = Color::WHITE;
/// The empty part of a slider, toggle or meter.
pub const TRACK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.14);
pub const METER_TRACK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
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
/// A text field: its height, background, edge, and placeholder text.
pub const FIELD_HEIGHT: f32 = 34.0;
pub const FIELD_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.06);
pub const FIELD_BORDER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
/// A focused field's edge.
pub const FIELD_FOCUS: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.50);
pub const PLACEHOLDER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.30);
pub const SELECTION: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.25);
/// The ring of a mark (today in a calendar), how thick its line is, and the height of a
/// calendar's rows.
pub const MARK_SIZE: f32 = 28.0;
pub const MARK_RING: f32 = 1.5;
pub const GRID_ROW: f32 = 32.0;
/// A badge's outline.
pub const BADGE_BORDER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.18);
/// A scrolling list's scrollbar.
pub const SCROLLER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.20);
/// Tabs: a pill-shaped track with a mark sliding behind the current tab, inset by `TAB_INSET`.
pub const TAB_HEIGHT: f32 = 28.0;
pub const TAB_INSET: f32 = 2.0;
pub const TAB_TRACK: Color = BUTTON_BG;
/// A little brighter than a selected row's fill, since it sits on the track.
pub const TAB_MARK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.16);
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

pub fn tone(tone: Tone) -> Color {
    match tone {
        // White ink at 94 % and 50 %: brightness, not hue, sets the hierarchy. Written as the
        // grey it makes on a black island, so overlapping strokes in an icon don't show seams.
        Tone::Normal => Color::from_rgb(0.94, 0.94, 0.94),
        Tone::Dim => Color::from_rgb(0.50, 0.50, 0.50),
        // The thing to notice is the brightest thing there.
        Tone::Accent => Color::WHITE,
        // Signals: vivid, because they're the only color on screen.
        Tone::Warning => Color::from_rgb8(0xFF, 0xB0, 0x20),
        Tone::Critical => Color::from_rgb8(0xFF, 0x45, 0x3A),
        Tone::Good => Color::from_rgb8(0x30, 0xD1, 0x58),
    }
}
