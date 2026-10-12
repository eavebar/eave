//! Every visual constant in one place. Plugins can't reach any of this: they name a [`Tone`], the
//! host picks the color.

use std::time::Duration;

use eave_api::ui::Tone;
use iced::{Color, Font, font};

/// Height of an island, the visible part of the bar.
pub const ISLAND_HEIGHT: u32 = 30;
/// Space between the islands and the top, left and right screen edges. A popover keeps the same
/// distance from the sides.
pub const EDGE_GAP: u16 = 6;
/// Transparent space below the islands, so a dragged island can dip down without being cut off.
pub const DRAG_ROOM: u16 = 6;
/// The bar surface and its exclusive zone: an island, the gap above it and the room below it.
pub const BAR_HEIGHT: u32 = EDGE_GAP as u32 + ISLAND_HEIGHT + DRAG_ROOM as u32;

pub const ISLAND_BG: Color = Color::BLACK;
pub const ISLAND_BORDER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
pub const HOVER_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
pub const PRESSED_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.14);
pub const SEPARATOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
/// Behind the current option of a switcher (the active workspace).
pub const SWITCHER_MARK: Color = Color::from_rgba(0.53, 0.70, 1.00, 0.22);

/// Horizontal padding inside each island, at its ends.
pub const ISLAND_PADDING: u16 = 14;
/// Corner radius of an island, before smoothing.
pub const ISLAND_RADIUS: f32 = 14.0;
/// How far each corner's curve blends into the straight edges (0 is a plain rounded rectangle,
/// 0.6 is the iOS look). On an island too short for the full spread, the curve is squeezed to fit
/// rather than the smoothing reduced, so a radius up to half the island's height stays smooth.
pub const ISLAND_SMOOTHING: f32 = 0.6;
/// How far each side of a pressed island moves in, in logical pixels.
pub const PRESS_INSET: f32 = 1.5;
/// The most a dragged island moves, sideways and up or down. It only approaches these: the further
/// the pointer goes, the harder it pulls back.
pub const DRAG_LIMIT_X: f32 = 10.0;
pub const DRAG_LIMIT_Y: f32 = 5.0;
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
/// The spring a popover's splat rides on, on top of its morph. Well under critical, so one kick
/// squashes the shape, swings it back a little the other way, and settles.
pub const SQUISH_STIFFNESS: f32 = 400.0;
pub const SQUISH_DAMPING: f32 = 0.35;
/// How hard an opening popover splats against where its bottom lands, in logical pixels per
/// second: flattened and widened by about 3 px each at the peak, before the bulge below.
pub const SQUISH_OPEN_FLATTEN: f32 = 100.0;
pub const SQUISH_OPEN_WIDEN: f32 = 100.0;
/// How hard a closing popover's island splats against the top: about 2 px flatter and 5 px wider
/// at the peak, before the bulge. Wider stays inside the `EDGE_GAP` from the screen's sides.
pub const SQUISH_CLOSE_FLATTEN: f32 = 60.0;
pub const SQUISH_CLOSE_WIDEN: f32 = 160.0;
/// How close, in logical pixels, a popover's bottom edge comes to where it's going before it
/// splats. The splat peaks about four frames after, as the shape arrives.
pub const SQUISH_LAND: f32 = 5.0;
/// How much stretching along one axis pulls the other in, per pixel stretched.
pub const SQUISH_BULGE: f32 = 0.5;
/// The spring a nudged island and its bump move back on: damped nearly to critical, so they come
/// back without swinging past.
pub const NUDGE_STIFFNESS: f32 = 600.0;
pub const NUDGE_DAMPING: f32 = 0.85;
/// How fast a nudge sets an island off, in logical pixels per second. A push sideways peaks at
/// about 5 px. Up or down, the whole island moves about half a pixel, and the bump in its edge
/// peaks at about 2 px.
pub const NUDGE_PUSH_SPEED: f32 = 300.0;
pub const NUDGE_LIFT_SPEED: f32 = 30.0;
pub const NUDGE_BUMP_SPEED: f32 = 120.0;
/// How much of its bump the nudged item moves with: it rides along, by less than the edge.
pub const NUDGE_ITEM_FOLLOW: f32 = 0.5;
/// How wide a bump in an island's edge is, end to end.
pub const NUDGE_BUMP_WIDTH: f32 = 64.0;
/// The furthest a bump reaches out of the island: it stays inside `EDGE_GAP` above and `DRAG_ROOM`
/// below.
pub const NUDGE_BUMP_ROOM: f32 = 4.0;
/// Horizontal padding inside each item, so its click target is wider than its text.
pub const ITEM_PADDING: u16 = 6;
pub const ITEM_SPACING: u32 = 4;

/// Space between a popover's edge and its content.
pub const PANEL_PADDING: u16 = 12;
pub const SPACING: u32 = 8;
pub const GRID_CELL: f32 = 28.0;
pub const BUTTON_RADIUS: f32 = 6.0;

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
/// A primary press held at least this long is a long press: it plays with the island, but the
/// item's action and popover don't fire on release.
pub const LONG_PRESS: Duration = Duration::from_millis(400);
/// Touchpad scrolling distance, in logical pixels, that counts as one scroll step.
pub const SCROLL_STEP: f32 = 30.0;

pub const TEXT_SIZE: f32 = 14.0;
/// Height of an icon, and width of the square ones.
pub const ICON_SIZE: f32 = 16.0;
/// The battery lies on its side, so it is wider than the others.
pub const BATTERY_ICON_WIDTH: f32 = 24.0;
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

pub fn tone(tone: Tone) -> Color {
    match tone {
        Tone::Normal => Color::from_rgb(0.90, 0.90, 0.92),
        Tone::Dim => Color::from_rgb(0.55, 0.55, 0.60),
        Tone::Accent => Color::from_rgb(0.53, 0.70, 1.00),
        Tone::Warning => Color::from_rgb(0.98, 0.76, 0.36),
        Tone::Critical => Color::from_rgb(0.97, 0.42, 0.42),
        Tone::Good => Color::from_rgb(0.45, 0.85, 0.55),
    }
}
