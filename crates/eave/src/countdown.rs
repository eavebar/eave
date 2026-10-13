//! A thin bar across the top of a notification, shrinking as its time on screen runs out. It
//! stands still while the clock is stopped: while the pointer is on the notifications, or before
//! they're on screen.

use std::time::{Duration, Instant};

use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Border, Element, Event, Length, Rectangle, Size, window};

use crate::theme;

/// How long a notification has on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Countdown {
    /// All of it.
    pub total: Duration,
    /// What's left, while the clock is stopped.
    pub left: Duration,
    /// When it runs out, while the clock runs.
    pub ends: Option<Instant>,
}

impl Countdown {
    /// How much is left, from 1 (all of it) to 0.
    fn fraction(&self, now: Instant) -> f32 {
        let left = match self.ends {
            Some(ends) => ends.saturating_duration_since(now),
            None => self.left,
        };
        if self.total.is_zero() {
            return 0.0;
        }
        (left.as_secs_f32() / self.total.as_secs_f32()).clamp(0.0, 1.0)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Countdown {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(theme::COUNTDOWN_HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let height = theme::COUNTDOWN_HEIGHT;
        let size = limits.resolve(Length::Fill, Length::Fixed(height), Size::new(0.0, height));
        layout::Node::new(size)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bar = layout.bounds();
        let rounded = Border::default().rounded(bar.height / 2.0);
        renderer.fill_quad(
            renderer::Quad {
                bounds: bar,
                border: rounded,
                ..renderer::Quad::default()
            },
            theme::METER_TRACK,
        );
        let left = bar.width * self.fraction(Instant::now());
        if left >= 1.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(bar.position(), Size::new(left, bar.height)),
                    border: rounded,
                    ..renderer::Quad::default()
                },
                theme::COUNTDOWN,
            );
        }
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        // Asks for the frame in which the bar is a pixel shorter, not for every frame.
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && let Some(ends) = self.ends
            && ends > *now
        {
            let width = layout.bounds().width.max(1.0);
            let step = self.total.div_f32(width).max(theme::COUNTDOWN_STEP);
            shell.request_redraw_at((*now + step).min(ends));
        }
    }
}

impl<'a, M: 'a> From<Countdown> for Element<'a, M> {
    fn from(countdown: Countdown) -> Self {
        Element::new(countdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_runs_down_only_while_its_clock_runs() {
        let now = Instant::now();
        let total = Duration::from_secs(4);
        let stopped = Countdown {
            total,
            left: Duration::from_secs(1),
            ends: None,
        };
        assert_eq!(stopped.fraction(now), 0.25);
        assert_eq!(stopped.fraction(now + Duration::from_secs(9)), 0.25);

        let running = Countdown {
            ends: Some(now + Duration::from_secs(2)),
            ..stopped
        };
        assert_eq!(running.fraction(now), 0.5);
        assert_eq!(running.fraction(now + Duration::from_secs(9)), 0.0);
    }
}
