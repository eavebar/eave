//! Status icons ([`Icon`]), drawn as vector paths in the color of the text around them. Parts
//! that are off (a missing signal bar, a quiet volume) stay faintly visible, so the icon reads as a
//! gauge. Geometry is laid out on a 16-unit grid and scaled to [`theme::ICON_SIZE`].

use std::f32::consts::PI;

use eave_api::ui::Icon;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, path::Arc};
use iced::{Color, Element, Point, Radians, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme;

/// The grid icons are drawn on, in units.
const GRID: f32 = 16.0;

pub fn view<'a, M: 'a>(icon: Icon, color: Color) -> Element<'a, M> {
    let width = match icon {
        Icon::Battery { .. } => theme::BATTERY_ICON_WIDTH,
        _ => theme::ICON_SIZE,
    };
    canvas::Canvas::new(Glyph { icon, color })
        .width(width)
        .height(theme::ICON_SIZE)
        .into()
}

/// How many sound waves show for a volume level.
fn volume_waves(level: f32) -> u8 {
    match level {
        l if l >= 0.8 => 3,
        l if l >= 0.5 => 2,
        l if l >= 0.2 => 1,
        _ => 0,
    }
}

/// How many arcs of the Wi-Fi fan light up for a signal strength.
fn wifi_bars(strength: f32) -> u8 {
    match strength {
        s if s >= 0.75 => 3,
        s if s >= 0.5 => 2,
        s if s >= 0.25 => 1,
        _ => 0,
    }
}

struct Glyph {
    icon: Icon,
    color: Color,
}

impl<M> canvas::Program<M> for Glyph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.scale(theme::ICON_SIZE / GRID);
        let mut pen = Pen {
            frame: &mut frame,
            on: self.color,
            off: Color {
                a: self.color.a * theme::ICON_OFF_ALPHA,
                ..self.color
            },
        };
        match self.icon {
            Icon::Volume { level, muted } => pen.volume(level, muted),
            Icon::Wifi { strength } => pen.wifi(strength),
            Icon::Wired => pen.wired(),
            Icon::Battery { charge, charging } => pen.battery(charge, charging),
        }
        vec![frame.into_geometry()]
    }
}

struct Pen<'a> {
    frame: &'a mut Frame,
    on: Color,
    off: Color,
}

impl Pen<'_> {
    fn shade(&self, on: bool) -> Color {
        if on { self.on } else { self.off }
    }

    fn line(&mut self, path: &Path, color: Color) {
        self.frame.stroke(
            path,
            Stroke::default()
                .with_color(color)
                .with_width(theme::ICON_STROKE)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
    }

    /// A slash with a gap of island background on either side, so it reads as cut through
    /// whatever is under it.
    fn slash(&mut self, from: Point, to: Point) {
        let path = Path::line(from, to);
        self.frame.stroke(
            &path,
            Stroke::default()
                .with_color(theme::ISLAND_BG)
                .with_width(theme::ICON_STROKE * 3.0)
                .with_line_cap(LineCap::Round),
        );
        self.line(&path, self.on);
    }

    fn volume(&mut self, level: f32, muted: bool) {
        let speaker = polygon(&[
            (1.5, 6.0),
            (4.0, 6.0),
            (7.5, 3.0),
            (7.5, 13.0),
            (4.0, 10.0),
            (1.5, 10.0),
        ]);
        self.frame.fill(&speaker, self.on);
        if muted {
            self.slash(Point::new(1.5, 2.5), Point::new(14.0, 13.5));
            return;
        }
        let waves = volume_waves(level);
        for (index, radius) in [3.0, 5.25, 7.5].into_iter().enumerate() {
            let color = self.shade((index as u8) < waves);
            let wave = arc(Point::new(7.0, 8.0), radius, -0.25 * PI, 0.25 * PI);
            self.line(&wave, color);
        }
    }

    fn wifi(&mut self, strength: Option<f32>) {
        let center = Point::new(8.0, 13.5);
        let bars = strength.map_or(0, wifi_bars);
        let dot = self.shade(strength.is_some());
        self.frame.fill(&Path::circle(center, 1.25), dot);
        for (index, radius) in [4.0, 7.25, 10.5].into_iter().enumerate() {
            let color = self.shade((index as u8) < bars);
            let fan = arc(center, radius, -0.72 * PI, -0.28 * PI);
            self.line(&fan, color);
        }
        if strength.is_none() {
            self.slash(Point::new(2.5, 2.5), Point::new(13.5, 13.5));
        }
    }

    /// Three linked boxes, like a small network diagram.
    fn wired(&mut self) {
        let boxes = [(5.75, 1.5), (1.5, 10.5), (10.0, 10.5)];
        for (x, y) in boxes {
            let square = Path::rounded_rectangle(Point::new(x, y), Size::new(4.5, 4.0), 1.0.into());
            self.frame.fill(&square, self.on);
        }
        let links = Path::new(|path| {
            path.move_to(Point::new(8.0, 5.5));
            path.line_to(Point::new(8.0, 8.0));
            path.move_to(Point::new(3.75, 10.5));
            path.line_to(Point::new(3.75, 8.0));
            path.line_to(Point::new(12.25, 8.0));
            path.line_to(Point::new(12.25, 10.5));
        });
        self.line(&links, self.on);
    }

    /// A battery on its side, filled from the left to its charge, and wider than the grid.
    fn battery(&mut self, charge: f32, charging: bool) {
        let width = theme::BATTERY_ICON_WIDTH / theme::ICON_SIZE * GRID;
        let body = Rectangle::new(Point::new(0.75, 3.25), Size::new(width - 3.75, 9.5));
        let outline = Path::rounded_rectangle(body.position(), body.size(), 2.5.into());
        self.line(&outline, self.on);
        let nub = Path::rounded_rectangle(
            Point::new(body.x + body.width + 1.0, 6.0),
            Size::new(1.75, 4.0),
            1.0.into(),
        );
        self.frame.fill(&nub, self.on);

        let inside = Size::new(body.width - 3.5, body.height - 3.5);
        let level = Size::new(inside.width * charge.clamp(0.0, 1.0), inside.height);
        if level.width > 0.5 {
            let fill = Path::rounded_rectangle(
                Point::new(body.x + 1.75, body.y + 1.75),
                level,
                1.0.into(),
            );
            self.frame.fill(&fill, self.on);
        }

        if charging {
            let (x, y) = (body.center_x(), body.center_y());
            let bolt = polygon(&[
                (x + 1.5, y - 5.0),
                (x - 2.5, y + 0.75),
                (x - 0.25, y + 0.75),
                (x - 1.5, y + 5.0),
                (x + 2.5, y - 0.75),
                (x + 0.25, y - 0.75),
            ]);
            self.frame.stroke(
                &bolt,
                Stroke::default()
                    .with_color(theme::ISLAND_BG)
                    .with_width(theme::ICON_STROKE * 1.5)
                    .with_line_join(LineJoin::Round),
            );
            self.frame.fill(&bolt, self.on);
        }
    }
}

fn polygon(points: &[(f32, f32)]) -> Path {
    Path::new(|path| {
        let mut points = points.iter().map(|&(x, y)| Point::new(x, y));
        if let Some(first) = points.next() {
            path.move_to(first);
            points.for_each(|point| path.line_to(point));
            path.close();
        }
    })
}

/// Angles in radians, clockwise from the right since y points down.
fn arc(center: Point, radius: f32, start: f32, end: f32) -> Path {
    Path::new(|path| {
        path.arc(Arc {
            center,
            radius,
            start_angle: Radians(start),
            end_angle: Radians(end),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_waves_follow_the_level() {
        assert_eq!(volume_waves(0.0), 0);
        assert_eq!(volume_waves(0.15), 0);
        assert_eq!(volume_waves(0.2), 1);
        assert_eq!(volume_waves(0.45), 1);
        assert_eq!(volume_waves(0.5), 2);
        assert_eq!(volume_waves(0.8), 3);
        assert_eq!(volume_waves(1.0), 3);
        assert_eq!(volume_waves(1.3), 3);
    }

    #[test]
    fn wifi_bars_follow_the_signal() {
        assert_eq!(wifi_bars(0.1), 0);
        assert_eq!(wifi_bars(0.25), 1);
        assert_eq!(wifi_bars(0.6), 2);
        assert_eq!(wifi_bars(0.75), 3);
        assert_eq!(wifi_bars(1.0), 3);
    }
}
