//! The island in the middle of the screen that shows what `eave calibrate` is changing. It morphs
//! like the bar's: its size and the mark on the selected row move on the morph spring, and its
//! lines fade on the fade spring as rows come and go, or as it folds into a pill and back.

use std::time::Instant;

use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Program, Stroke};
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, mouse, window};

use crate::spring::{FADE, MAX_FRAME, MORPH, Spring};
use crate::squircle::squircle;
use crate::theme;

use super::rows::Line;

const WIDTH: f32 = 320.0;
const ROW: f32 = 26.0;
const HEADING: f32 = 30.0;
const PADDING: f32 = 14.0;
/// The keys, under the rows.
const FOOTER: [&str; 2] = [
    "↑ ↓ pick   ← → change   Shift ×10   Space toggle",
    "Enter save   Esc quit   ? hide",
];
const FOOTER_LINE: f32 = 18.0;
/// The pill it folds into.
const PILL: Size = Size::new(150.0, theme::ISLAND_HEIGHT as f32);
const PILL_TEXT: &str = "? calibrate";

/// What the panel shows: its lines with their values, which row is selected, and whether it's
/// folded away.
pub struct Panel {
    pub lines: Vec<(Line, String)>,
    /// Among the rows, not the lines.
    pub selected: usize,
    pub hidden: bool,
}

impl Panel {
    /// Its size, open: all its lines and the keys.
    fn size(&self) -> Size {
        let lines: f32 = self
            .lines
            .iter()
            .map(|(line, _)| match line {
                Line::Heading(_) => HEADING,
                Line::Row(_) => ROW,
            })
            .sum();
        Size::new(
            WIDTH,
            2.0 * PADDING + lines + FOOTER_LINE * FOOTER.len() as f32 + PADDING / 2.0,
        )
    }

    /// Where the selected row starts, from the panel's top.
    fn mark(&self) -> f32 {
        let mut y = PADDING;
        let mut rows = 0;
        for (line, _) in &self.lines {
            match line {
                Line::Heading(_) => y += HEADING,
                Line::Row(_) => {
                    if rows == self.selected {
                        return y;
                    }
                    rows += 1;
                    y += ROW;
                }
            }
        }
        y
    }
}

/// The springs, and when they last moved.
pub struct Motion {
    width: Spring,
    height: Spring,
    mark: Spring,
    /// 1 shows its lines, 0 the pill's.
    open: Spring,
    /// Fades the lines in when what they are changes: rows coming or going.
    lines: Spring,
    count: usize,
    last: Option<Instant>,
}

impl Default for Motion {
    fn default() -> Self {
        // Settled where it is on the first frame.
        Self {
            width: Spring::new(f32::NAN, MORPH),
            height: Spring::new(f32::NAN, MORPH),
            mark: Spring::new(f32::NAN, MORPH),
            open: Spring::new(1.0, FADE),
            lines: Spring::new(1.0, FADE),
            count: 0,
            last: None,
        }
    }
}

impl Motion {
    fn springs(&mut self) -> [&mut Spring; 5] {
        [
            &mut self.width,
            &mut self.height,
            &mut self.mark,
            &mut self.open,
            &mut self.lines,
        ]
    }
}

impl<Message> Program<Message> for Panel {
    type State = Motion;

    fn update(
        &self,
        motion: &mut Motion,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        // Every frame starts with this, after the view is rebuilt, so the targets are current.
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let size = if self.hidden { PILL } else { self.size() };
        motion.width.target = size.width;
        motion.height.target = size.height;
        motion.mark.target = self.mark();
        motion.open.target = if self.hidden { 0.0 } else { 1.0 };
        if motion.count != self.lines.len() {
            // New lines come in from nothing, while the shape makes room for them.
            if motion.count != 0 {
                motion.lines.value = 0.0;
            }
            motion.count = self.lines.len();
        }
        motion.lines.target = 1.0;
        for spring in motion.springs() {
            if spring.value.is_nan() {
                spring.settle();
            }
        }

        let seconds = motion.last.map_or(0.0, |last| {
            now.saturating_duration_since(last)
                .min(MAX_FRAME)
                .as_secs_f32()
        });
        let mut moving = false;
        for spring in motion.springs() {
            spring.advance(seconds);
            if spring.at_rest() {
                spring.settle();
            } else {
                moving = true;
            }
        }
        if moving {
            motion.last = Some(*now);
            Some(canvas::Action::request_redraw())
        } else {
            motion.last = None;
            None
        }
    }

    fn draw(
        &self,
        motion: &Motion,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let open = self.size();
        let size = if motion.width.value.is_nan() {
            if self.hidden { PILL } else { open }
        } else {
            Size::new(motion.width.value, motion.height.value)
        };
        let at = Point::new(
            (bounds.width - size.width) / 2.0,
            (bounds.height - size.height) / 2.0,
        );
        let shape = Rectangle::new(at, size);
        let radius = if self.hidden {
            PILL.height / 2.0
        } else {
            theme::ISLAND_RADIUS
        };
        frame.fill(
            &squircle(shape, radius, theme::ISLAND_SMOOTHING),
            theme::ISLAND_BG,
        );
        let hairline = Rectangle::new(
            at + Vector::new(0.5, 0.5),
            Size::new(size.width - 1.0, size.height - 1.0),
        );
        frame.stroke(
            &squircle(hairline, radius - 0.5, theme::ISLAND_SMOOTHING),
            Stroke::default()
                .with_color(theme::ISLAND_BORDER)
                .with_width(1.0),
        );

        let open_by = if motion.open.value.is_nan() {
            if self.hidden { 0.0 } else { 1.0 }
        } else {
            motion.open.value.clamp(0.0, 1.0)
        };
        let shown = open_by * motion.lines.value.clamp(0.0, 1.0);
        let faded = |color: Color, by: f32| Color {
            a: color.a * by,
            ..color
        };
        let text = |frame: &mut Frame, content: &str, x: f32, y: f32, color, font, align_x| {
            frame.fill_text(canvas::Text {
                content: content.to_string(),
                position: Point::new(x, y),
                color,
                size: Pixels(theme::TEXT_SIZE),
                font,
                align_x,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
        };

        // Nothing spills out of the shape while it grows or shrinks.
        frame.with_clip(shape, |frame| {
            // The clip only clips: move the origin to the shape's top left.
            frame.translate(Vector::new(at.x, at.y));
            let left = (size.width - open.width) / 2.0 + PADDING;
            let right = left + open.width - 2.0 * PADDING;
            if shown > 0.0 {
                let mark_y = if motion.mark.value.is_nan() {
                    self.mark()
                } else {
                    motion.mark.value
                };
                let mark = Rectangle::new(
                    Point::new(left - PADDING / 2.0, mark_y),
                    Size::new(right - left + PADDING, ROW),
                );
                frame.fill(
                    &squircle(mark, ROW / 2.0, theme::ISLAND_SMOOTHING),
                    faded(theme::SWITCHER_MARK, open_by),
                );

                let mut y = PADDING;
                for (line, value) in &self.lines {
                    match line {
                        Line::Heading(heading) => {
                            text(
                                frame,
                                heading,
                                left,
                                y + HEADING / 2.0 + 3.0,
                                faded(theme::tone(eave_api::ui::Tone::Dim), shown),
                                theme::STRONG_FONT,
                                iced::alignment::Horizontal::Left.into(),
                            );
                            y += HEADING;
                        }
                        Line::Row(row) => {
                            let middle = y + ROW / 2.0;
                            text(
                                frame,
                                row.label(),
                                left,
                                middle,
                                faded(theme::tone(eave_api::ui::Tone::Normal), shown),
                                Font::DEFAULT,
                                iced::alignment::Horizontal::Left.into(),
                            );
                            text(
                                frame,
                                value,
                                right,
                                middle,
                                faded(theme::tone(eave_api::ui::Tone::Accent), shown),
                                theme::STRONG_FONT,
                                iced::alignment::Horizontal::Right.into(),
                            );
                            y += ROW;
                        }
                    }
                }
                y += PADDING / 2.0;
                for line in FOOTER {
                    text(
                        frame,
                        line,
                        size.width / 2.0,
                        y + FOOTER_LINE / 2.0,
                        faded(theme::tone(eave_api::ui::Tone::Dim), shown),
                        Font::DEFAULT,
                        iced::alignment::Horizontal::Center.into(),
                    );
                    y += FOOTER_LINE;
                }
            }
            if open_by < 1.0 {
                text(
                    frame,
                    PILL_TEXT,
                    size.width / 2.0,
                    size.height / 2.0,
                    faded(theme::tone(eave_api::ui::Tone::Normal), 1.0 - open_by),
                    Font::DEFAULT,
                    iced::alignment::Horizontal::Center.into(),
                );
            }
        });
        vec![frame.into_geometry()]
    }
}
