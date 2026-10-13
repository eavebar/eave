//! What `eave calibrate` lets you change, one row each, and how the keys change it.

use crate::config::NotchShape;

use super::shape::Shape;

/// One thing to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// All four corners at once.
    Corners,
    /// One corner: top left, top right, bottom right, bottom left.
    Corner(usize),
    CornerSmoothing,
    Notch,
    NotchWidth,
    NotchHeight,
    NotchRadius,
    NotchSmoothing,
    NotchShoulder,
    NotchTop,
    /// One edge's inset: top, right, bottom, left.
    Inset(usize),
}

/// A line of the panel: a heading, or a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Heading(&'static str),
    Row(Row),
}

/// The panel's lines, for a model with or without a notch: its rows only show with one.
pub fn lines(notch: bool) -> Vec<Line> {
    let mut lines = vec![
        Line::Heading("Corners"),
        Line::Row(Row::Corners),
        Line::Row(Row::Corner(0)),
        Line::Row(Row::Corner(1)),
        Line::Row(Row::Corner(2)),
        Line::Row(Row::Corner(3)),
        Line::Row(Row::CornerSmoothing),
        Line::Heading("Notch"),
        Line::Row(Row::Notch),
    ];
    if notch {
        lines.extend(
            [
                Row::NotchWidth,
                Row::NotchHeight,
                Row::NotchRadius,
                Row::NotchSmoothing,
                Row::NotchShoulder,
                Row::NotchTop,
            ]
            .map(Line::Row),
        );
    }
    lines.push(Line::Heading("Hidden under the bezel"));
    lines.extend((0..4).map(|edge| Line::Row(Row::Inset(edge))));
    lines
}

/// The rows alone, in order.
pub fn rows(notch: bool) -> Vec<Row> {
    lines(notch)
        .into_iter()
        .filter_map(|line| match line {
            Line::Row(row) => Some(row),
            Line::Heading(_) => None,
        })
        .collect()
}

const CORNERS: [&str; 4] = ["Top left", "Top right", "Bottom right", "Bottom left"];
const EDGES: [&str; 4] = ["Top", "Right", "Bottom", "Left"];

/// How far one step of smoothing goes.
const SMOOTHING_STEP: f32 = 0.05;
/// How many steps a big one, with Shift, takes: of pixels, and of smoothing.
const BIG_PIXELS: f32 = 10.0;
const BIG_SMOOTHING: f32 = 4.0;
/// Where a notch that's moved below the top edge starts, if it never was before.
const FIRST_TOP: f32 = 10.0;

impl Row {
    pub fn label(self) -> &'static str {
        match self {
            Row::Corners => "All corners",
            Row::Corner(corner) => CORNERS[corner],
            Row::CornerSmoothing | Row::NotchSmoothing => "Smoothing",
            Row::Notch => "Notch",
            Row::NotchWidth => "Width",
            Row::NotchHeight => "Height",
            Row::NotchRadius => "Lower corners",
            Row::NotchShoulder => "Shoulders",
            Row::NotchTop => "From the top",
            Row::Inset(edge) => EDGES[edge],
        }
    }

    /// Its value as the panel shows it.
    pub fn value(self, shape: &Shape) -> String {
        let pixels = |size: f32| format!("{size} px");
        let smoothing = |smoothing: f32| {
            if smoothing <= 0.0 {
                "circle".to_string()
            } else {
                format!("squircle {smoothing:.2}")
            }
        };
        let corners = shape.screen.corners;
        let notch = shape.notch.unwrap_or_default();
        match self {
            Row::Corners if corners.iter().all(|corner| *corner == corners[0]) => {
                pixels(corners[0])
            }
            Row::Corners => "mixed".to_string(),
            Row::Corner(corner) => pixels(corners[corner]),
            Row::CornerSmoothing => smoothing(shape.screen.smoothing),
            Row::Notch => if shape.notch.is_some() { "on" } else { "off" }.to_string(),
            Row::NotchWidth => pixels(notch.width),
            Row::NotchHeight => pixels(notch.height.unwrap_or_default()),
            Row::NotchRadius => pixels(notch.radius),
            Row::NotchSmoothing => smoothing(notch.smoothing),
            Row::NotchShoulder => pixels(notch.shoulder),
            Row::NotchTop if notch.top <= 0.0 => "attached".to_string(),
            Row::NotchTop => pixels(notch.top),
            Row::Inset(edge) => pixels(shape.screen.inset[edge]),
        }
    }
}

/// The model being changed, and what it remembers to bring back what's toggled off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Editing {
    pub shape: Shape,
    /// The notch to bring back when it's turned on again.
    pub notch: NotchShape,
    /// Where the notch went the last time it was below the top edge.
    pub top: f32,
}

impl Editing {
    pub fn new(shape: Shape, notch: NotchShape) -> Self {
        Self {
            notch: shape.notch.unwrap_or(notch),
            top: shape
                .notch
                .map(|notch| notch.top)
                .filter(|top| *top > 0.0)
                .unwrap_or(FIRST_TOP),
            shape,
        }
    }

    /// Changes `row` by `steps` (1 or -1), each a pixel, or a twentieth of smoothing; ten times
    /// that, or four for smoothing, if `big`.
    pub fn step(&mut self, row: Row, steps: f32, big: bool) {
        let pixels = steps * if big { BIG_PIXELS } else { 1.0 };
        let smoothing = steps * SMOOTHING_STEP * if big { BIG_SMOOTHING } else { 1.0 };
        let grow = |size: &mut f32, by: f32, least: f32| *size = (*size + by).max(least);
        let ease = |value: &mut f32| {
            // Rounded, so stepping back and forth lands on the same values.
            *value = ((*value + smoothing).clamp(0.0, 1.0) * 100.0).round() / 100.0;
        };
        let screen = &mut self.shape.screen;
        match row {
            Row::Corners => {
                for corner in &mut screen.corners {
                    grow(corner, pixels, 0.0);
                }
            }
            Row::Corner(corner) => grow(&mut screen.corners[corner], pixels, 0.0),
            Row::CornerSmoothing => ease(&mut screen.smoothing),
            Row::Inset(edge) => grow(&mut screen.inset[edge], pixels, 0.0),
            Row::Notch => self.toggle(row),
            Row::NotchTop
                if self.shape.notch.is_some_and(|notch| notch.top <= 0.0) && steps < 0.0 => {}
            _ => {
                let Some(notch) = &mut self.shape.notch else {
                    return;
                };
                match row {
                    Row::NotchWidth => grow(&mut notch.width, pixels, 2.0),
                    Row::NotchHeight => {
                        let height = notch.height.get_or_insert(0.0);
                        grow(height, pixels, 1.0);
                    }
                    Row::NotchRadius => grow(&mut notch.radius, pixels, 0.0),
                    Row::NotchSmoothing => ease(&mut notch.smoothing),
                    Row::NotchShoulder => grow(&mut notch.shoulder, pixels, 0.0),
                    Row::NotchTop => {
                        grow(&mut notch.top, pixels, 0.0);
                        if notch.top > 0.0 {
                            self.top = notch.top;
                        }
                    }
                    _ => {}
                }
                self.notch = *notch;
            }
        }
    }

    /// Turns what `row` is on or off: the notch, or its hanging from the top edge. Nothing for
    /// the rest.
    pub fn toggle(&mut self, row: Row) {
        match row {
            Row::Notch => {
                self.shape.notch = match self.shape.notch {
                    Some(notch) => {
                        self.notch = notch;
                        None
                    }
                    None => Some(self.notch),
                }
            }
            Row::NotchTop => {
                if let Some(notch) = &mut self.shape.notch {
                    notch.top = if notch.top > 0.0 { 0.0 } else { self.top };
                    self.notch = *notch;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing() -> Editing {
        let notch = NotchShape {
            width: 370.0,
            height: Some(66.0),
            ..NotchShape::default()
        };
        Editing::new(Shape::default(), notch)
    }

    #[test]
    fn the_notchs_rows_show_only_with_a_notch() {
        assert!(!rows(false).contains(&Row::NotchWidth));
        assert!(rows(false).contains(&Row::Notch));
        assert!(rows(true).contains(&Row::NotchWidth));
        assert_eq!(rows(true).len(), rows(false).len() + 6);
    }

    #[test]
    fn steps_are_pixels_or_twentieths_and_never_go_below_zero() {
        let mut editing = editing();
        editing.step(Row::Corner(1), 1.0, false);
        editing.step(Row::Corner(1), 1.0, true);
        assert_eq!(editing.shape.screen.corners, [0.0, 11.0, 0.0, 0.0]);
        assert_eq!(Row::Corners.value(&editing.shape), "mixed");
        editing.step(Row::Corners, -1.0, true);
        assert_eq!(editing.shape.screen.corners, [0.0, 1.0, 0.0, 0.0]);
        for _ in 0..3 {
            editing.step(Row::CornerSmoothing, 1.0, false);
        }
        assert_eq!(editing.shape.screen.smoothing, 0.15);
        editing.step(Row::CornerSmoothing, 1.0, true);
        editing.step(Row::CornerSmoothing, 1.0, true);
        editing.step(Row::CornerSmoothing, 1.0, true);
        editing.step(Row::CornerSmoothing, 1.0, true);
        assert_eq!(editing.shape.screen.smoothing, 0.95);
        editing.step(Row::CornerSmoothing, 1.0, true);
        assert_eq!(editing.shape.screen.smoothing, 1.0);
    }

    #[test]
    fn the_notch_comes_back_as_it_was() {
        let mut editing = editing();
        editing.toggle(Row::Notch);
        editing.step(Row::NotchWidth, 1.0, false);
        assert_eq!(editing.shape.notch.unwrap().width, 371.0);
        editing.toggle(Row::Notch);
        assert_eq!(editing.shape.notch, None);
        // Steps on its rows do nothing while it's off.
        editing.step(Row::NotchWidth, 1.0, false);
        editing.step(Row::Notch, 1.0, false);
        assert_eq!(editing.shape.notch.unwrap().width, 371.0);
    }

    #[test]
    fn the_notch_goes_down_and_back_up_to_where_it_was() {
        let mut editing = editing();
        editing.toggle(Row::Notch);
        assert_eq!(Row::NotchTop.value(&editing.shape), "attached");
        editing.step(Row::NotchTop, -1.0, false);
        assert_eq!(editing.shape.notch.unwrap().top, 0.0);
        editing.toggle(Row::NotchTop);
        assert_eq!(editing.shape.notch.unwrap().top, FIRST_TOP);
        editing.step(Row::NotchTop, 1.0, false);
        editing.toggle(Row::NotchTop);
        assert_eq!(editing.shape.notch.unwrap().top, 0.0);
        editing.toggle(Row::NotchTop);
        assert_eq!(editing.shape.notch.unwrap().top, FIRST_TOP + 1.0);
    }
}
