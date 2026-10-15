//! glass-spike: what's behind the bar, as the compositor's glass plugin sees it, so an island
//! over something bright can turn light and keep its ink readable.
//!
//! The plugin (`hypr-glass/`) sends a coarse grid of the frosted backdrop's brightness over
//! each eave surface, as a Hyprland custom event, whenever it changes:
//! `eave-glass <pid> <cell> <cols> <rows> <one hex digit per cell, row by row>`, with cells
//! `cell` logical pixels square from the surface's top left.

use iced::Rectangle;

pub const EVENT: &str = "eave-glass";

/// Turns light above this brightness, and dark again below the lower one: apart, so an island
/// over something in between doesn't flicker.
const TO_LIGHT: f32 = 0.6;
const TO_DARK: f32 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    cell: f32,
    cols: usize,
    rows: usize,
    /// 0 to 1, row by row.
    brightness: Vec<f32>,
}

impl Backdrop {
    /// The event's data after its name; `None` when it's for another eave or malformed.
    pub fn parse(data: &str) -> Option<Self> {
        let mut words = data.split_whitespace();
        let pid: u32 = words.next()?.parse().ok()?;
        if pid != std::process::id() {
            return None;
        }
        let cell: f32 = words.next()?.parse().ok()?;
        let cols: usize = words.next()?.parse().ok()?;
        let rows: usize = words.next()?.parse().ok()?;
        let brightness = words
            .next()
            .unwrap_or("")
            .chars()
            .map(|digit| Some(digit.to_digit(16)? as f32 / 15.0))
            .collect::<Option<Vec<_>>>()?;
        (cell > 0.0 && brightness.len() == cols * rows).then_some(Self {
            cell,
            cols,
            rows,
            brightness,
        })
    }

    /// How bright it is behind `rect` (surface coordinates), on average.
    fn behind(&self, rect: Rectangle) -> Option<f32> {
        let span = |from: f32, size: f32, count: usize| {
            let first = ((from / self.cell).floor().max(0.0) as usize).min(count);
            let last = (((from + size) / self.cell).ceil().max(0.0) as usize).min(count);
            first..last
        };
        let (cols, rows) = (
            span(rect.x, rect.width, self.cols),
            span(rect.y, rect.height, self.rows),
        );
        let cells: Vec<f32> = rows
            .flat_map(|row| cols.clone().map(move |col| row * self.cols + col))
            .map(|i| self.brightness[i])
            .collect();
        (!cells.is_empty()).then(|| cells.iter().sum::<f32>() / cells.len() as f32)
    }

    /// Whether an island at `rect` should be light, given whether it is now. An empty grid (the
    /// plugin stopped reporting, or unloaded) turns every island back.
    pub fn light(&self, rect: Rectangle, now: bool) -> bool {
        if self.brightness.is_empty() {
            return false;
        }
        match self.behind(rect) {
            Some(brightness) if now => brightness > TO_DARK,
            Some(brightness) => brightness > TO_LIGHT,
            None => now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backdrop(data: &str) -> Option<Backdrop> {
        Backdrop::parse(&format!("{} {data}", std::process::id()))
    }

    #[test]
    fn parses_its_own_and_skips_others() {
        let grid = backdrop("16 2 1 0f").unwrap();
        assert_eq!(grid.brightness, vec![0.0, 1.0]);
        assert_eq!(Backdrop::parse("1 16 2 1 0f"), None);
        assert_eq!(backdrop("16 2 2 0f"), None, "too few cells");
    }

    #[test]
    fn an_empty_grid_turns_islands_back() {
        let grid = backdrop("16 0 0").unwrap();
        assert!(!grid.light(
            Rectangle::new(iced::Point::ORIGIN, iced::Size::new(10.0, 10.0)),
            true
        ));
    }

    #[test]
    fn turns_light_and_back_with_a_gap_between() {
        // Columns of 16 px: dark, middling, bright.
        let grid = backdrop("16 3 1 09f").unwrap();
        let at = |col: f32| {
            Rectangle::new(
                iced::Point::new(col * 16.0 + 2.0, 2.0),
                iced::Size::new(10.0, 10.0),
            )
        };
        assert!(!grid.light(at(0.0), false));
        assert!(!grid.light(at(1.0), false), "0.6 isn't above 0.6");
        assert!(grid.light(at(1.0), true), "but stays light");
        assert!(grid.light(at(2.0), false));
    }
}
