//! The host side of [`eave_api::ui::timeline`]: steps strung on a thin line, each marked on it
//! with where it stands. Each step is a row: a strip of the line with the step's mark, then the
//! step. The strips meet, so the line runs unbroken from the first mark to the last.
//!
//! A long timeline, like a chat, drops its oldest steps as new ones come. Each step carries a key,
//! so when the steps shift up the line, their widget state shifts with them: text that didn't
//! change keeps its shaping, instead of every step being set again as the one below it.

use eave_api::ui::{Mark, Tone};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::{column, container, row, stack};
use iced::{Border, Color, Element, Event, Length, Padding, Point, Rectangle, Size, Vector};

use crate::theme;

/// One step: its mark, or a picture in its place, and what it shows.
pub struct Step<'a, M> {
    /// Tells this step apart from the others, and from itself in the last view.
    pub key: u64,
    pub mark: Mark,
    pub leading: Option<Element<'a, M>>,
    pub content: Element<'a, M>,
}

pub fn view<'a, M: 'a>(steps: Vec<Step<'a, M>>) -> Element<'a, M> {
    let count = steps.len();
    let keys = steps.iter().map(|step| step.key).collect();
    let steps = column(steps.into_iter().enumerate().map(|(index, step)| {
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
    }));
    Element::new(Keyed {
        keys,
        steps: steps.into(),
    })
}

/// The steps' column, with the key of each step, so their state follows them when the oldest
/// ones are dropped.
struct Keyed<'a, M> {
    keys: Vec<u64>,
    steps: Element<'a, M>,
}

/// The keys of the steps as the last view had them.
#[derive(Default)]
struct Keys(Vec<u64>);

/// How many steps were dropped from the front of `old` to make `new`, if that's what happened:
/// what's left of `old` starts `new`. The last step left may have changed since, like a message
/// still being written, so it doesn't have to match.
fn dropped(old: &[u64], new: &[u64]) -> usize {
    let Some(&first) = new.first() else {
        return 0;
    };
    (1..old.len())
        .find(|&shift| {
            let kept = &old[shift..];
            let both = kept.len().min(new.len());
            kept[0] == first && kept[..both - 1] == new[..both - 1]
        })
        .unwrap_or(0)
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Keyed<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Keys>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Keys(self.keys.clone()))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.steps)]
    }

    fn diff(&self, tree: &mut Tree) {
        let keys = tree.state.downcast_mut::<Keys>();
        let shift = dropped(&keys.0, &self.keys);
        if keys.0 != self.keys {
            keys.0.clone_from(&self.keys);
        }
        if shift > 0
            && let Some(column) = tree.children.first_mut()
        {
            let shift = shift.min(column.children.len());
            column.children.drain(..shift);
        }
        tree.diff_children(std::slice::from_ref(&self.steps));
    }

    fn size(&self) -> Size<Length> {
        self.steps.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.steps.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.steps
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.steps.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.steps
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.steps.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.steps.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.steps.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_dropped_from_the_front_are_found() {
        assert_eq!(dropped(&[1, 2, 3, 4], &[2, 3, 4, 5]), 1);
        assert_eq!(dropped(&[1, 2, 3, 4], &[3, 4, 5, 6]), 2);
        // The last one left changed as it was written.
        assert_eq!(dropped(&[1, 2, 3, 4], &[2, 3, 9, 5]), 1);
    }

    #[test]
    fn steps_that_only_grew_or_changed_are_not_shifted() {
        assert_eq!(dropped(&[1, 2, 3], &[1, 2, 3, 4]), 0);
        assert_eq!(dropped(&[1, 2, 3], &[1, 2, 9]), 0);
        assert_eq!(dropped(&[1, 2, 3], &[7, 8, 9]), 0);
        assert_eq!(dropped(&[], &[1]), 0);
        assert_eq!(dropped(&[1, 2], &[]), 0);
    }

    #[test]
    fn a_repeated_step_does_not_mislead() {
        // Steps 1 and 3 read the same; only one step was dropped.
        assert_eq!(dropped(&[1, 2, 1, 4], &[2, 1, 4, 5]), 1);
    }
}
