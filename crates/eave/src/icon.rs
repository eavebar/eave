//! Status icons ([`Icon`]), drawn as vector paths in the color of the text around them. Parts
//! that are off (a missing signal bar, a quiet volume) stay faintly visible, so the icon reads as a
//! gauge. Geometry is laid out on a 16-unit grid and scaled to [`theme::ICON_SIZE`].
//!
//! A charging battery pours: its charge ripples like a liquid, and every few seconds it gulps a few
//! times, like a bottle being emptied, the whole icon swelling with each gulp. It moves at 30
//! frames a second, not the compositor's rate, to keep it cheap
//! ([D-37](../../../docs/decisions.md)).

use std::f32::consts::{PI, TAU};
use std::time::Instant;

use eave_api::ui::Icon;
use iced::widget::Action;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, path::Arc};
use iced::{Color, Element, Event, Point, Radians, Rectangle, Renderer, Size, Theme, Vector};
use iced::{mouse, window};

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

/// How far the charge surges forward at the height of a gulp, and how far its edge ripples, in
/// grid units.
const SURGE: f32 = 0.5;
const RIPPLE: f32 = 0.3;
/// How fast the ripple runs.
const RIPPLE_HZ: f32 = 1.2;
/// How much the battery stretches up and squeezes in at the height of a gulp.
const SWELL_Y: f32 = 0.12;
const SWELL_X: f32 = 0.06;

/// Where a charging battery is in its cycle of gulps, and its ripple.
#[derive(Default)]
struct Pour {
    /// The first frame drawn while charging, when the first gulp starts.
    start: Option<Instant>,
    /// Seconds into the current cycle of gulps, as of the last frame.
    t: f32,
    /// Where the ripple is along its wave, in radians. It never stops, so it runs on across
    /// cycles.
    phase: f32,
}

impl Pour {
    /// Advances to `now`. Returns when the next frame is needed, if ever: one
    /// [`theme::BATTERY_FRAME`] later, for as long as it's charging.
    fn step(&mut self, charging: bool, now: Instant) -> Option<Instant> {
        if !charging {
            *self = Self::default();
            return None;
        }
        let start = *self.start.get_or_insert(now);
        let elapsed = now.saturating_duration_since(start).as_secs_f64();
        let every = theme::BATTERY_GULP_EVERY.as_secs_f64();
        self.t = (elapsed % every) as f32;
        let period = 1.0 / f64::from(RIPPLE_HZ);
        self.phase = ((elapsed % period) / period) as f32 * TAU;
        Some(now + theme::BATTERY_FRAME)
    }

    fn gulp(&self) -> Gulp {
        Gulp {
            swell: swell(self.t),
            phase: self.phase,
        }
    }
}

/// How the pour moves the battery on a frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Gulp {
    /// From 0 (still) to 1: rises and falls with each gulp, a little less each time.
    swell: f32,
    /// Where the ripple is along its wave, in radians.
    phase: f32,
}

/// The swell at `t` seconds into a cycle: a few gulps, then still until the next cycle.
fn swell(t: f32) -> f32 {
    let length = theme::BATTERY_GULP.as_secs_f32();
    let index = (t / length).floor();
    if index >= theme::BATTERY_GULPS as f32 {
        return 0.0;
    }
    (PI * (t / length - index)).sin().powi(2) * (1.0 - 0.2 * index)
}

struct Glyph {
    icon: Icon,
    color: Color,
}

impl<M> canvas::Program<M> for Glyph {
    type State = Pour;

    fn update(
        &self,
        pour: &mut Pour,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<M>> {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let charging = matches!(self.icon, Icon::Battery { charging: true, .. });
        pour.step(charging, *now).map(Action::request_redraw_at)
    }

    fn draw(
        &self,
        pour: &Pour,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.scale(theme::ICON_SIZE / GRID);
        let gulp = pour.gulp();
        if let Icon::Battery { charging: true, .. } = self.icon {
            // Swells from its middle.
            let middle = Vector::new(battery_width() / 2.0, GRID / 2.0);
            frame.translate(middle);
            frame.scale_nonuniform(Vector::new(
                1.0 - SWELL_X * gulp.swell,
                1.0 + SWELL_Y * gulp.swell,
            ));
            frame.translate(-middle);
        }
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
            Icon::Battery { charge, charging } => pen.battery(charge, charging, gulp),
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

    /// A battery on its side, filled from the left to its charge, and wider than the grid. While
    /// charging, the charge is a liquid, moved by the `gulp`.
    fn battery(&mut self, charge: f32, charging: bool, gulp: Gulp) {
        let width = battery_width();
        let body = Rectangle::new(Point::new(0.75, 3.25), Size::new(width - 3.75, 9.5));
        let outline = Path::rounded_rectangle(body.position(), body.size(), 2.5.into());
        self.line(&outline, self.on);
        let nub = Path::rounded_rectangle(
            Point::new(body.x + body.width + 1.0, 6.0),
            Size::new(1.75, 4.0),
            1.0.into(),
        );
        self.frame.fill(&nub, self.on);

        let inside = Rectangle::new(
            Point::new(body.x + 1.75, body.y + 1.75),
            Size::new(body.width - 3.5, body.height - 3.5),
        );
        let level = inside.width * charge.clamp(0.0, 1.0);
        if level > 0.5 {
            let fill = if charging {
                liquid(inside, level, gulp)
            } else {
                Path::rounded_rectangle(
                    inside.position(),
                    Size::new(level, inside.height),
                    1.0.into(),
                )
            };
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

/// The battery's width in grid units.
fn battery_width() -> f32 {
    theme::BATTERY_ICON_WIDTH / theme::ICON_SIZE * GRID
}

/// A charge `level` units into `inside`, with rounded corners like the still one, but its right
/// edge rippling, and surging with the `gulp`. Kept inside the battery.
fn liquid(inside: Rectangle, level: f32, gulp: Gulp) -> Path {
    const RADIUS: f32 = 1.0;
    const STEPS: usize = 8;
    let (left, top, bottom) = (inside.x, inside.y, inside.y + inside.height);
    let edge = |y: f32| {
        let across = (y - top) / inside.height;
        let ripple = RIPPLE * (TAU * 0.8 * across - gulp.phase).sin();
        (left + level + SURGE * gulp.swell + ripple).clamp(left + 2.0 * RADIUS, left + inside.width)
    };
    Path::new(|path| {
        path.move_to(Point::new(left, top + RADIUS));
        path.quadratic_curve_to(Point::new(left, top), Point::new(left + RADIUS, top));
        let x = edge(top);
        path.line_to(Point::new(x - RADIUS, top));
        path.quadratic_curve_to(Point::new(x, top), Point::new(x, top + RADIUS));
        for step in 1..STEPS {
            let y = top + RADIUS + (inside.height - 2.0 * RADIUS) * step as f32 / STEPS as f32;
            path.line_to(Point::new(edge(y), y));
        }
        let x = edge(bottom);
        path.line_to(Point::new(x, bottom - RADIUS));
        path.quadratic_curve_to(Point::new(x, bottom), Point::new(x - RADIUS, bottom));
        path.line_to(Point::new(left + RADIUS, bottom));
        path.quadratic_curve_to(Point::new(left, bottom), Point::new(left, bottom - RADIUS));
        path.close();
    })
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
    use std::time::Duration;

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
    fn a_charging_battery_ripples_and_gulps_then_rests() {
        let mut pour = Pour::default();
        let start = Instant::now();
        let next = pour.step(true, start);
        assert_eq!(
            next,
            Some(start + theme::BATTERY_FRAME),
            "frames while charging"
        );

        assert!(pour.step(true, start + theme::BATTERY_GULP / 2).is_some());
        let swell = pour.gulp().swell;
        assert!(swell > 0.99, "at the height of the first gulp, {swell}");

        let quarter = Duration::from_secs_f32(0.25 / RIPPLE_HZ);
        let resting = start + theme::BATTERY_GULP * theme::BATTERY_GULPS + quarter;
        assert!(pour.step(true, resting).is_some(), "the ripple never stops");
        assert_eq!(pour.gulp().swell, 0.0, "no gulps between bursts");

        let next = start + theme::BATTERY_GULP_EVERY + theme::BATTERY_GULP / 2;
        pour.step(true, next);
        assert!(pour.gulp().swell > 0.99, "and gulps again");
    }

    #[test]
    fn gulps_fade() {
        let length = theme::BATTERY_GULP.as_secs_f32();
        let heights: Vec<f32> = (0..theme::BATTERY_GULPS)
            .map(|index| swell((index as f32 + 0.5) * length))
            .collect();
        assert!(
            heights.windows(2).all(|pair| pair[1] < pair[0]),
            "{heights:?}"
        );
        assert_eq!(swell(0.0), 0.0, "starts still");
    }

    #[test]
    fn a_battery_that_stops_charging_stops_asking_for_frames() {
        let mut pour = Pour::default();
        let start = Instant::now();
        pour.step(true, start);
        assert_eq!(pour.step(false, start + theme::BATTERY_GULP), None);
        assert_eq!(pour.gulp(), Gulp::default());
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
