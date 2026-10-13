//! The host side of [`eave_api::ui::timeline`]: steps strung on a thin line, each marked on it
//! with where it stands. Each step is a row: a strip of the line with the step's mark, then the
//! step. The strips meet, so the line runs unbroken from the first mark to the last.

use eave_api::ui::{Mark, Tone};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Widget, layout, mouse, renderer};
use iced::widget::{column, container, row, stack};
use iced::{Border, Color, Element, Length, Padding, Point, Rectangle, Size};

use crate::theme;

/// One step: its mark, or a picture in its place, and what it shows.
pub struct Step<'a, M> {
    pub mark: Mark,
    pub leading: Option<Element<'a, M>>,
    pub content: Element<'a, M>,
}

pub fn view<'a, M: 'a>(steps: Vec<Step<'a, M>>) -> Element<'a, M> {
    let count = steps.len();
    column(steps.into_iter().enumerate().map(|(index, step)| {
        let picture = step.leading.is_some();
        let strip = Strip {
            mark: (!picture).then_some(step.mark),
            above: index > 0,
            below: index + 1 < count,
            hole: if picture {
                theme::SPRITE_HEIGHT_PANEL
            } else {
                0.0
            },
        };
        let gutter: Element<'a, M> = match step.leading {
            Some(leading) => stack![
                strip,
                container(leading)
                    .width(theme::RAIL_GUTTER)
                    .height(theme::SPRITE_HEIGHT_PANEL)
                    .center_x(theme::RAIL_GUTTER)
                    .center_y(theme::SPRITE_HEIGHT_PANEL),
            ]
            // As tall as the step, like a bare strip: a stack shrinks by default, and a strip
            // in it would then reach for all the height a scroll has.
            .width(theme::RAIL_GUTTER)
            .height(Length::Fill)
            .into(),
            None => strip.into(),
        };
        let gap = if index + 1 < count {
            theme::RAIL_STEP_GAP
        } else {
            0.0
        };
        let content = container(step.content)
            .width(Length::Fill)
            .padding(Padding::ZERO.bottom(gap));
        row![gutter, content].spacing(theme::SPACING + 2).into()
    }))
    .into()
}

/// A step's strip of the line, as tall as its step.
struct Strip {
    mark: Option<Mark>,
    /// The line comes down from the step before, and goes on to the step after.
    above: bool,
    below: bool,
    /// Room at the top left clear for a picture.
    hole: f32,
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Strip {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(theme::RAIL_GUTTER), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(
            Length::Fixed(theme::RAIL_GUTTER),
            Length::Fill,
            Size::new(theme::RAIL_GUTTER, theme::RAIL_MARK_Y * 2.0),
        );
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

        let bounds = layout.bounds();
        let middle = bounds.x + bounds.width / 2.0;
        let mark_y = bounds.y + theme::RAIL_MARK_Y;
        let mut line = |from: f32, to: f32| {
            if to - from < 0.5 {
                return;
            }
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new((middle - theme::RAIL_LINE / 2.0).round(), from),
                        Size::new(theme::RAIL_LINE, to - from),
                    ),
                    ..renderer::Quad::default()
                },
                theme::TRACK,
            );
        };
        if self.hole > 0.0 {
            // Around the picture, not through it.
            if self.above {
                line(bounds.y, bounds.y + 1.0);
            }
            if self.below {
                line(bounds.y + self.hole + 2.0, bounds.y + bounds.height);
            }
            return;
        }
        let from = if self.above { bounds.y } else { mark_y };
        let to = if self.below {
            bounds.y + bounds.height
        } else {
            mark_y
        };
        line(from, to);

        let Some(mark) = self.mark else {
            return;
        };
        let dot = |renderer: &mut iced::Renderer, size: f32, fill: Color, ring: Option<Color>| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(middle - size / 2.0, mark_y - size / 2.0),
                        Size::new(size, size),
                    ),
                    border: Border {
                        color: ring.unwrap_or(Color::TRANSPARENT),
                        width: if ring.is_some() {
                            theme::MARK_RING
                        } else {
                            0.0
                        },
                        radius: (size / 2.0).into(),
                    },
                    ..renderer::Quad::default()
                },
                fill,
            );
        };
        match mark {
            Mark::Note => dot(renderer, theme::RAIL_NOTE, theme::RAIL_DONE, None),
            Mark::You => dot(renderer, theme::RAIL_MARK, theme::tone(Tone::Accent), None),
            Mark::Done => dot(renderer, theme::RAIL_MARK, theme::RAIL_DONE, None),
            Mark::Failed => dot(
                renderer,
                theme::RAIL_MARK,
                theme::tone(Tone::Critical),
                None,
            ),
            Mark::Waiting => dot(
                renderer,
                theme::RAIL_MARK,
                theme::ISLAND_BG,
                Some(theme::tone(Tone::Warning)),
            ),
            Mark::Running => {
                dot(renderer, theme::RAIL_MARK + 6.0, theme::RAIL_HALO, None);
                dot(
                    renderer,
                    theme::RAIL_MARK,
                    theme::ISLAND_BG,
                    Some(theme::tone(Tone::Accent)),
                );
            }
        }
    }
}

impl<'a, M: 'a> From<Strip> for Element<'a, M> {
    fn from(strip: Strip) -> Self {
        Element::new(strip)
    }
}
