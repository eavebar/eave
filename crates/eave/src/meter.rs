//! The host side of [`eave_api::ui::meter`]: a thin bar filled to a fraction, in its tone, with an
//! optional tick at a limit. A new fraction is eased to, not jumped to.

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Border, Color, Element, Event, Length, Point, Rectangle, Size, window};

use crate::ease::Ease;
use crate::theme;

pub struct Meter {
    fraction: f32,
    limit: Option<f32>,
    color: Color,
}

impl Meter {
    pub fn new(fraction: f32, limit: Option<f32>, color: Color) -> Self {
        Self {
            fraction: fraction.clamp(0.0, 1.0),
            limit: limit.map(|limit| limit.clamp(0.0, 1.0)),
            color,
        }
    }
}

/// How far the limit's tick reaches above and below the bar.
const TICK_REACH: f32 = 3.0;
const TICK_WIDTH: f32 = 2.0;

impl<M> Widget<M, iced::Theme, iced::Renderer> for Meter {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Ease>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Ease::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        tree.state.downcast_mut::<Ease>().set(self.fraction);
        // Room for the tick, so it isn't cut off.
        let height = theme::METER_HEIGHT + 2.0 * TICK_REACH;
        let size = limits.resolve(Length::Fill, Length::Fixed(height), Size::new(0.0, height));
        layout::Node::new(size)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let bar = Rectangle::new(
            Point::new(bounds.x, bounds.y + TICK_REACH),
            Size::new(bounds.width, theme::METER_HEIGHT),
        );
        let rounded = Border::default().rounded(theme::METER_HEIGHT / 2.0);
        renderer.fill_quad(
            renderer::Quad {
                bounds: bar,
                border: rounded,
                ..renderer::Quad::default()
            },
            theme::METER_TRACK,
        );
        let filled = bar.width * tree.state.downcast_ref::<Ease>().value();
        if filled >= 1.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        bar.position(),
                        Size::new(filled.max(theme::METER_HEIGHT), bar.height),
                    ),
                    border: rounded,
                    ..renderer::Quad::default()
                },
                self.color,
            );
        }
        if let Some(limit) = self.limit {
            let x = (bar.x + bar.width * limit - TICK_WIDTH / 2.0).round();
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(x, bounds.y),
                        Size::new(TICK_WIDTH, bounds.height),
                    ),
                    border: Border::default().rounded(TICK_WIDTH / 2.0),
                    ..renderer::Quad::default()
                },
                theme::tone(eave_api::ui::Tone::Dim),
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && tree
                .state
                .downcast_mut::<Ease>()
                .step(*now, theme::METER_EASE)
        {
            shell.request_redraw();
        }
    }
}

impl<'a, M: 'a> From<Meter> for Element<'a, M> {
    fn from(meter: Meter) -> Self {
        Element::new(meter)
    }
}
