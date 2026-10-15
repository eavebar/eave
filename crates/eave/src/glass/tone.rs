//! How bright it is behind the bar, as the glass sees it, so the ink of an island over something
//! bright can turn dark and stay readable.

use iced::Rectangle;

use crate::theme;

#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    /// Logical px, square, from the surface's top left.
    cell: f32,
    cols: usize,
    rows: usize,
    /// 0 to 1, row by row.
    brightness: Vec<f32>,
}

impl Backdrop {
    /// A `backdrop` event's grid; `None` when it doesn't add up. No cells means nothing to report.
    pub fn new(cell: u32, cols: u32, rows: u32, cells: &[u8]) -> Option<Self> {
        let (cols, rows) = (cols as usize, rows as usize);
        (cells.len() == cols * rows && (cell > 0 || cells.is_empty())).then(|| Self {
            cell: cell as f32,
            cols,
            rows,
            brightness: cells.iter().map(|&b| f32::from(b) / 255.0).collect(),
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

    /// Whether the ink of an island at `rect` should go darker, given whether it does now. Apart,
    /// so an island over something in between doesn't flicker. An empty grid turns every island
    /// back.
    pub fn darker(&self, rect: Rectangle, now: bool) -> bool {
        if self.brightness.is_empty() {
            return false;
        }
        match self.behind(rect) {
            Some(brightness) if now => brightness > theme::GLASS_TO_LIGHT_INK,
            Some(brightness) => brightness > theme::GLASS_TO_DARK_INK,
            None => now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_a_grid_that_adds_up() {
        let grid = Backdrop::new(16, 2, 1, &[0, 255]).unwrap();
        assert_eq!(grid.brightness, vec![0.0, 1.0]);
        assert_eq!(Backdrop::new(16, 2, 2, &[0, 255]), None, "too few cells");
        assert_eq!(Backdrop::new(0, 1, 1, &[0]), None, "no cell size");
        assert!(Backdrop::new(0, 0, 0, &[]).is_some(), "nothing to report");
    }

    #[test]
    fn an_empty_grid_turns_islands_back() {
        let grid = Backdrop::new(0, 0, 0, &[]).unwrap();
        assert!(!grid.darker(
            Rectangle::new(iced::Point::ORIGIN, iced::Size::new(10.0, 10.0)),
            true
        ));
    }

    #[test]
    fn ink_turns_dark_and_back_with_a_gap_between() {
        // Columns of 16 px: dark, middling, bright.
        let grid = Backdrop::new(16, 3, 1, &[0, 140, 255]).unwrap();
        let at = |col: f32| {
            Rectangle::new(
                iced::Point::new(col * 16.0 + 2.0, 2.0),
                iced::Size::new(10.0, 10.0),
            )
        };
        assert!(!grid.darker(at(0.0), false));
        assert!(!grid.darker(at(1.0), false), "0.55 isn't above 0.6");
        assert!(grid.darker(at(1.0), true), "but stays dark");
        assert!(grid.darker(at(2.0), false));
    }
}
