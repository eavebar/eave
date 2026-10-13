//! The host side of [`eave_api::ui::tabs`]: a pill-shaped strip of tab names over the current
//! tab's page. Clicking a name shows its page; the mark behind the current name slides there like
//! the switcher's. Which tab is current is ours, not the plugin's: it lives in the widget state,
//! which goes with the panel, so every popover opens on its first tab. Pages that aren't showing
//! keep their state, so a list scrolled on one tab is still scrolled when it comes back.
//!
//! Switching hands the page over: the old one slides away from the new tab as it fades out, then
//! the new one slides in from that side. The tabs take the new page's height at once, so the
//! panel's shape eases to it while the old page goes.

use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{
    Border, Element, Event, Font, Length, Padding, Pixels, Point, Rectangle, Size, Vector,
    alignment, window,
};

use crate::handover::Handover;
use crate::island;
use crate::switcher::Motion;
use crate::theme;

pub struct Tabs<'a, M> {
    names: Vec<String>,
    pages: Vec<Element<'a, M>>,
}

impl<'a, M> Tabs<'a, M> {
    pub fn new(names: Vec<String>, pages: Vec<Element<'a, M>>) -> Self {
        Self { names, pages }
    }
}

#[derive(Debug)]
struct State {
    current: usize,
    hovered: Option<usize>,
    /// The mark, relative to the tabs.
    motion: Motion,
    /// The page on screen, and the fade from one to the next.
    pages: Handover,
}

impl Default for State {
    fn default() -> Self {
        Self {
            current: 0,
            hovered: None,
            motion: Motion::default(),
            pages: Handover::new(Some(0), theme::TAB_FADE),
        }
    }
}

/// The way the old page leaves when going from tab `from` to `to`: away from the new one, which
/// comes in from its own side.
fn away(from: usize, to: usize) -> Vector {
    Vector::new(if to > from { -1.0 } else { 1.0 }, 0.0)
}

/// Which of `count` equal segments across `width` the point `x` falls in.
fn segment_at(width: f32, count: usize, x: f32) -> Option<usize> {
    if count == 0 || !(0.0..width).contains(&x) {
        return None;
    }
    Some(((x / width * count as f32) as usize).min(count - 1))
}

/// The tab to show: the current one, or the first if there are fewer tabs now.
fn showing(current: usize, count: usize) -> usize {
    if current < count { current } else { 0 }
}

/// Segment `index` of `count`, relative to the tabs, inset for the mark.
fn segment(width: f32, count: usize, index: usize) -> Rectangle {
    let each = width / count.max(1) as f32;
    Rectangle::new(
        Point::new(each * index as f32 + theme::TAB_INSET, theme::TAB_INSET),
        Size::new(
            each - 2.0 * theme::TAB_INSET,
            theme::TAB_HEIGHT - 2.0 * theme::TAB_INSET,
        ),
    )
}

/// How far the page sits below the top of the tabs.
const PAGE_TOP: f32 = theme::TAB_HEIGHT + theme::SPACING as f32;

impl<M> Tabs<'_, M> {
    fn current(&self, tree: &Tree) -> usize {
        showing(tree.state.downcast_ref::<State>().current, self.pages.len())
    }

    /// The page on its way out, if there is one and it's still there.
    fn leaving(&self, tree: &Tree) -> Option<usize> {
        let current = self.current(tree);
        tree.state
            .downcast_ref::<State>()
            .pages
            .leaving()
            .filter(|&leaving| leaving != current && leaving < self.pages.len())
    }

    /// The segment under the cursor, if it is over the strip.
    fn under(&self, layout: Layout<'_>, cursor: mouse::Cursor) -> Option<usize> {
        let bounds = layout.bounds();
        let strip = Rectangle::new(
            bounds.position(),
            Size::new(bounds.width, theme::TAB_HEIGHT),
        );
        let at = cursor.position_over(strip)?;
        segment_at(bounds.width, self.names.len(), at.x - bounds.x)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Tabs<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let current = self.current(tree);
        let limits = limits.width(Length::Fill).shrink(Size::new(0.0, PAGE_TOP));
        let page = self.pages[current]
            .as_widget_mut()
            .layout(&mut tree.children[current], renderer, &limits)
            .move_to(Point::new(0.0, PAGE_TOP));
        let width = limits.max().width;
        let width = if width.is_finite() {
            width
        } else {
            page.size().width
        };
        let size = Size::new(width, PAGE_TOP + page.size().height);
        // The page going out is laid out too, after the current one, until it's gone.
        let mut children = vec![page];
        if let Some(leaving) = self.leaving(tree) {
            children.push(
                self.pages[leaving]
                    .as_widget_mut()
                    .layout(&mut tree.children[leaving], renderer, &limits)
                    .move_to(Point::new(0.0, PAGE_TOP)),
            );
        }
        layout::Node::with_children(size, children)
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
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let origin = Vector::new(bounds.x, bounds.y);
        let state = tree.state.downcast_ref::<State>();
        let current = self.current(tree);
        let count = self.names.len();
        let pill = |height: f32| Border::default().rounded(height / 2.0);

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    bounds.position(),
                    Size::new(bounds.width, theme::TAB_HEIGHT),
                ),
                border: pill(theme::TAB_HEIGHT),
                ..renderer::Quad::default()
            },
            theme::TAB_TRACK,
        );
        if let Some(hovered) = state.hovered.filter(|&hovered| hovered != current) {
            let hover = segment(bounds.width, count, hovered) + origin;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: hover,
                    border: pill(hover.height),
                    ..renderer::Quad::default()
                },
                theme::HOVER_BG,
            );
        }
        if let Some(shown) = state.motion.shown {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: shown + origin,
                    border: pill(shown.height),
                    ..renderer::Quad::default()
                },
                theme::TAB_MARK,
            );
        }

        for (index, name) in self.names.iter().enumerate() {
            let place = segment(bounds.width, count, index) + origin;
            let (font, tone) = if index == current {
                (theme::STRONG_FONT, eave_api::ui::Tone::Accent)
            } else {
                (Font::DEFAULT, eave_api::ui::Tone::Dim)
            };
            let Some(clip) = place.intersection(viewport) else {
                continue;
            };
            renderer.fill_text(
                text::Text {
                    content: name.clone(),
                    bounds: place.size(),
                    size: Pixels(theme::TEXT_SIZE),
                    line_height: text::LineHeight::default(),
                    font,
                    align_x: text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::Basic,
                    wrapping: text::Wrapping::None,
                },
                place.center(),
                theme::tone(tone),
                clip,
            );
        }

        let (index, shown, slide) = state.pages.shown(current);
        let (index, page) = if index == current {
            (index, layout.children().next())
        } else if self.leaving(tree) == Some(index) {
            (index, layout.children().nth(1))
        } else {
            (current, layout.children().next())
        };
        let Some(page) = page else {
            return;
        };
        let draw = |renderer: &mut iced::Renderer| {
            self.pages[index].as_widget().draw(
                &tree.children[index],
                renderer,
                theme,
                style,
                page,
                cursor,
                viewport,
            );
        };
        if shown >= 1.0 && slide == Vector::ZERO {
            draw(renderer);
            return;
        }
        // Clipped to where the slide can reach, within what the shape shows: a layer isn't
        // clipped by the ones around it.
        let slide = slide * theme::TAB_SLIDE;
        let cover = page.bounds().expand(2.0);
        let Some(reach) = cover
            .expand(Padding::from([0.0, theme::TAB_SLIDE]))
            .intersection(viewport)
        else {
            return;
        };
        renderer.with_layer(reach, |renderer| {
            renderer.with_translation(slide, |renderer| {
                draw(renderer);
                island::fade(renderer, cover, reach - slide, shown);
            });
        });
    }

    fn children(&self) -> Vec<Tree> {
        self.pages.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.pages);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        // Only the page on screen: a field on a hidden tab can't take focus.
        let current = self.current(tree);
        let Some(page) = layout.children().next() else {
            return;
        };
        self.pages[current].as_widget_mut().operate(
            &mut tree.children[current],
            page,
            renderer,
            operation,
        );
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
        let under = self.under(layout, cursor);
        let current = self.current(tree);
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                if state.hovered != under {
                    state.hovered = under;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(index) = under {
                    if index != current {
                        state.current = index;
                        state.pages.set(index, away(current, index));
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                    shell.capture_event();
                    return;
                }
            }
            // Every frame starts with this event, after layout, so the target is current.
            Event::Window(window::Event::RedrawRequested(now)) => {
                let target = segment(layout.bounds().width, self.names.len(), current);
                let mark = state.motion.step(Some(target), *now);
                let pages = state.pages.step(*now);
                if mark || pages {
                    shell.request_redraw();
                }
            }
            _ => {}
        }

        if let Some(page) = layout.children().next() {
            self.pages[current].as_widget_mut().update(
                &mut tree.children[current],
                event,
                page,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let current = self.current(tree);
        match self.under(layout, cursor) {
            Some(index) if index != current => mouse::Interaction::Pointer,
            Some(_) => mouse::Interaction::None,
            None => layout
                .children()
                .next()
                .map(|page| {
                    self.pages[current].as_widget().mouse_interaction(
                        &tree.children[current],
                        page,
                        cursor,
                        viewport,
                        renderer,
                    )
                })
                .unwrap_or_default(),
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        let current = self.current(tree);
        let page = layout.children().next()?;
        self.pages[current].as_widget_mut().overlay(
            &mut tree.children[current],
            page,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Tabs<'a, M>> for Element<'a, M> {
    fn from(tabs: Tabs<'a, M>) -> Self {
        Element::new(tabs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_lands_in_its_segment() {
        assert_eq!(segment_at(300.0, 3, 0.0), Some(0));
        assert_eq!(segment_at(300.0, 3, 99.9), Some(0));
        assert_eq!(segment_at(300.0, 3, 100.0), Some(1));
        assert_eq!(segment_at(300.0, 3, 299.9), Some(2));
        assert_eq!(segment_at(300.0, 3, 300.0), None);
        assert_eq!(segment_at(300.0, 3, -1.0), None);
        assert_eq!(segment_at(300.0, 0, 10.0), None);
    }

    #[test]
    fn falls_back_to_the_first_tab_when_tabs_go_away() {
        assert_eq!(showing(2, 3), 2);
        assert_eq!(showing(2, 2), 0);
    }

    #[test]
    fn the_old_page_leaves_away_from_the_new_tab() {
        assert_eq!(away(0, 2), Vector::new(-1.0, 0.0), "to the right: out left");
        assert_eq!(away(2, 1), Vector::new(1.0, 0.0), "to the left: out right");
    }

    #[test]
    fn switching_lays_out_both_pages_until_the_old_one_is_gone() {
        let mut tabs: Tabs<'_, ()> = Tabs::new(
            vec!["a".into(), "b".into()],
            vec![
                iced::widget::Space::new().width(10).height(100).into(),
                iced::widget::Space::new().width(10).height(40).into(),
            ],
        );
        let mut tree = Tree::new(&tabs as &dyn Widget<_, _, _>);
        let renderer = iced::Renderer::new(Font::DEFAULT, Pixels(14.0));
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 1000.0));
        let node = tabs.layout(&mut tree, &renderer, &limits);
        assert_eq!(node.children().len(), 1);

        let state = tree.state.downcast_mut::<State>();
        state.current = 1;
        state.pages.set(1, away(0, 1));
        let node = tabs.layout(&mut tree, &renderer, &limits);
        assert_eq!(
            node.size().height,
            PAGE_TOP + 40.0,
            "the new page's height at once"
        );
        assert_eq!(node.children().len(), 2, "the old page, while it goes");
        assert_eq!(node.children()[1].size().height, 100.0);

        let state = tree.state.downcast_mut::<State>();
        let start = std::time::Instant::now();
        state.pages.step(start);
        assert!(!state.pages.step(start + theme::TAB_FADE));
        let node = tabs.layout(&mut tree, &renderer, &limits);
        assert_eq!(node.children().len(), 1);
    }

    #[test]
    fn the_mark_sits_inside_its_segment() {
        let mark = segment(300.0, 3, 1);
        assert_eq!(mark.x, 100.0 + theme::TAB_INSET);
        assert_eq!(mark.width, 100.0 - 2.0 * theme::TAB_INSET);
        assert_eq!(mark.height, theme::TAB_HEIGHT - 2.0 * theme::TAB_INSET);
    }
}
