//! Every visual constant in one place. Plugins can't reach any of this: they name a [`Tone`], the
//! host picks the color.

use eave_api::ui::Tone;
use iced::{Color, Font, font};

pub const BAR_HEIGHT: u32 = 32;

pub const BAR_BG: Color = Color::from_rgba(0.07, 0.07, 0.09, 0.85);
pub const PANEL_BG: Color = Color::from_rgba(0.09, 0.09, 0.11, 0.95);
pub const PANEL_BORDER: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
pub const HOVER_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
pub const PRESSED_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.14);
pub const SEPARATOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);

/// Horizontal padding at the bar's ends.
pub const BAR_PADDING: u16 = 12;
/// Horizontal padding inside each item, so its click target is wider than its text.
pub const ITEM_PADDING: u16 = 6;
pub const ITEM_SPACING: u32 = 4;

pub const PANEL_PADDING: u16 = 12;
pub const PANEL_RADIUS: f32 = 10.0;
/// Space between the bar and a popover, and between a popover and the screen edge.
pub const PANEL_GAP: i32 = 6;
pub const SPACING: u32 = 8;
pub const GRID_CELL: f32 = 28.0;
pub const BUTTON_RADIUS: f32 = 6.0;

pub const TEXT_SIZE: f32 = 14.0;
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
    }
}
