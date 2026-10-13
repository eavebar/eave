//! Scrolling that moves like a hand moved it (D-77). Around a scrollable, it lets a touchpad's
//! scroll through as it comes, and keeps it going when the fingers lift, slowing to a stop. A
//! wheel's steps ease to where they go instead of jumping there. Both move the scrollable on the
//! compositor's frames, and stop asking for them at rest or at the end of the content.
//!
//! The compositor doesn't fling: lifting the fingers only sends `axis_stop`, which reaches iced
//! as a scroll of zero pixels. So does `axis_source`, before each scroll a touchpad sends, so a
//! zero only counts as the lift if no scroll comes after it.
//!
//! Fingers that land on the touchpad catch the content where it is, before they move. The
//! compositor says so with a hold gesture, which our patched layershellev counts, and wakes the
//! bar with another zero (D-79).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, Scrollable};
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, keyboard, window};

use crate::theme;

/// How far one step of a wheel scrolls, as iced's scrollable does it.
const WHEEL_STEP: f32 = 60.0;
/// The touchpad scrolling the fling's speed is measured over.
const WINDOW: Duration = Duration::from_millis(100);
/// Without a lift since the last scroll, the fingers count as lifted after this long. Only used
/// where the compositor never sends one.
const QUIET: Duration = Duration::from_millis(80);
/// A zero scroll is the lift if no scroll comes this soon after it.
const SETTLED: Duration = Duration::from_millis(10);
/// Fingers that stood still this long before lifting don't fling.
const RESTED: Duration = Duration::from_millis(50);

pub struct Glide<'a, M> {
    content: Element<'a, M>,
    /// Whether the scrollable is anchored to its bottom, which turns its offsets upside down.
    bottom: bool,
}

impl<'a, M> Glide<'a, M> {
    /// Around `scrollable`, which must be the first scrollable in it.
    pub fn new(scrollable: impl Into<Element<'a, M>>) -> Self {
        Self {
            content: scrollable.into(),
            bottom: false,
        }
    }

    /// The scrollable keeps its end in view (`anchor_bottom`).
    pub fn bottom(mut self, bottom: bool) -> Self {
        self.bottom = bottom;
        self
    }
}

#[derive(Debug, Default)]
struct State {
    motion: Motion,
    shift: bool,
    /// Whether this compositor says when the fingers lift. Until it has, a pause counts as one.
    lifts: bool,
    /// The touchpad holds there have been, as last seen.
    holds: u64,
}

/// What moves the content, in pixels of content toward its end: positive is down or right.
#[derive(Debug, Default, Clone, PartialEq)]
struct Motion {
    /// Recent touchpad scrolls, while the fingers are on it.
    samples: VecDeque<(Instant, Vector)>,
    /// When the fingers lifted, if they did since the last scroll.
    lifted: Option<Instant>,
    /// The fling's speed, in pixels a second.
    velocity: Vector,
    /// Where a wheel's steps still have to go.
    pending: Vector,
    /// The last frame something moved on.
    frame: Option<Instant>,
}

impl Motion {
    /// The fingers moved the content by `delta`, at `now`.
    fn touch(&mut self, delta: Vector, now: Instant) {
        self.velocity = Vector::ZERO;
        self.pending = Vector::ZERO;
        self.lifted = None;
        self.samples.push_back((now, delta));
        while let Some(&(at, _)) = self.samples.front()
            && now.duration_since(at) > WINDOW
        {
            self.samples.pop_front();
        }
    }

    /// The speed of the last scrolls, in pixels a second. The first sample only starts the clock.
    fn speed(&self) -> Vector {
        let (Some(&(first, _)), Some(&(last, _))) = (self.samples.front(), self.samples.back())
        else {
            return Vector::ZERO;
        };
        let span = last.duration_since(first).as_secs_f32();
        if span < 0.008 {
            return Vector::ZERO;
        }
        let moved = self
            .samples
            .iter()
            .skip(1)
            .fold(Vector::ZERO, |sum, &(_, delta)| sum + delta);
        moved * (1.0 / span)
    }

    /// The fingers lifted at `at`: the content flies on at the speed it was going, unless it
    /// had stopped before.
    fn lift(&mut self, at: Instant) {
        let rested = self
            .samples
            .back()
            .is_none_or(|&(last, _)| at.duration_since(last) > RESTED);
        let speed = if rested { Vector::ZERO } else { self.speed() };
        self.samples.clear();
        self.lifted = None;
        if speed.x.hypot(speed.y) >= theme::SCROLL_GLIDE_MIN {
            self.velocity = speed;
            self.frame = Some(at);
        }
    }

    /// A wheel scrolled `delta`: it goes there, on from wherever the last steps were going.
    fn wheel(&mut self, delta: Vector, now: Instant) {
        self.samples.clear();
        self.velocity = Vector::ZERO;
        if self.pending == Vector::ZERO {
            self.frame = Some(now);
        }
        self.pending += delta;
    }

    fn stop(&mut self) {
        *self = Self::default();
    }

    /// Whether a fling or a wheel is still moving the content.
    fn moving(&self) -> bool {
        self.velocity != Vector::ZERO || self.pending != Vector::ZERO
    }

    /// How far the content moves on the frame at `now`. Stops when there's no more to go.
    fn step(&mut self, now: Instant) -> Vector {
        let Some(frame) = self.frame.replace(now) else {
            return Vector::ZERO;
        };
        let dt = now.duration_since(frame).as_secs_f32();
        let mut moved = Vector::ZERO;
        if self.velocity != Vector::ZERO {
            // The distance the decaying speed covers over the frame, exactly.
            let decay = (-dt / theme::SCROLL_GLIDE_DECAY).exp();
            moved += self.velocity * (theme::SCROLL_GLIDE_DECAY * (1.0 - decay));
            self.velocity *= decay;
            if self.velocity.x.hypot(self.velocity.y) < theme::SCROLL_GLIDE_MIN {
                self.velocity = Vector::ZERO;
            }
        }
        if self.pending != Vector::ZERO {
            let eased = 1.0 - (-dt / theme::SCROLL_WHEEL_EASE).exp();
            let mut part = self.pending * eased;
            if (self.pending - part).x.hypot((self.pending - part).y) < 0.5 {
                part = self.pending;
            }
            self.pending -= part;
            moved += part;
        }
        if !self.moving() {
            self.frame = None;
        }
        moved
    }
}

/// Finds the first scrollable, its bounds and how far it's scrolled, and scrolls it `by`.
struct Reach {
    by: Option<Vector>,
    bottom: bool,
    found: Option<(Rectangle, Rectangle, Vector)>,
}

impl Reach {
    /// How far it can still go along `delta`, each way that `delta` goes.
    fn room(&self, delta: Vector) -> Vector {
        let Some((bounds, content, at)) = self.found else {
            return Vector::ZERO;
        };
        let most = Vector::new(
            (content.width - bounds.width).max(0.0),
            (content.height - bounds.height).max(0.0),
        );
        let room = |delta: f32, at: f32, most: f32| {
            if delta > 0.0 {
                (most - at).max(0.0)
            } else if delta < 0.0 {
                at.max(0.0)
            } else {
                0.0
            }
        };
        Vector::new(room(delta.x, at.x, most.x), room(delta.y, at.y, most.y))
    }
}

impl Operation for Reach {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        if self.found.is_none() {
            operate(self);
        }
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        if self.found.is_some() {
            return;
        }
        self.found = Some((bounds, content_bounds, translation));
        if let Some(by) = self.by {
            // A scrollable anchored to its bottom counts its offset up from there.
            let y = if self.bottom { -by.y } else { by.y };
            state.scroll_by(AbsoluteOffset { x: by.x, y }, bounds, content_bounds);
        }
    }
}

impl<M> Glide<'_, M> {
    /// Looks at the scrollable, scrolling it `by` first if given, and says how it was before.
    fn reach(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        by: Option<Vector>,
    ) -> Reach {
        let mut reach = Reach {
            by,
            bottom: self.bottom,
            found: None,
        };
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut reach);
        reach
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Glide<'_, M> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
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
        let now = Instant::now();
        // Fingers landing on the touchpad catch what's moving, wherever the pointer is.
        if let Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 0.0 },
        }) = event
        {
            let state = tree.state.downcast_mut::<State>();
            let holds = layershellev::holds();
            if state.holds != holds {
                state.holds = holds;
                state.motion.stop();
            }
        }
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                tree.state.downcast_mut::<State>().shift = modifiers.shift();
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                tree.state.downcast_mut::<State>().motion.stop();
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(layout.bounds()) && !shell.is_event_captured() =>
            {
                match *delta {
                    mouse::ScrollDelta::Pixels { x, y } => {
                        let state = tree.state.downcast_mut::<State>();
                        if x == 0.0 && y == 0.0 {
                            if !state.motion.samples.is_empty() {
                                state.motion.lifted = Some(now);
                                state.lifts = true;
                                shell.request_redraw();
                            }
                        } else {
                            state.motion.touch(Vector::new(-x, -y), now);
                            // Asks for a frame to see whether the fingers lifted.
                            shell.request_redraw();
                        }
                        // Scrolled as it comes, by the scrollable.
                    }
                    mouse::ScrollDelta::Lines { x, y } => {
                        let shift = tree.state.downcast_ref::<State>().shift;
                        let (x, y) = if shift { (y, x) } else { (x, y) };
                        let delta = Vector::new(-x, -y) * WHEEL_STEP;
                        // A scrollable that can't go that way leaves the wheel to what's around it.
                        let room = self.reach(tree, layout, renderer, None).room(delta);
                        if room != Vector::ZERO {
                            let motion = &mut tree.state.downcast_mut::<State>().motion;
                            motion.wheel(delta, now);
                            shell.capture_event();
                            shell.request_redraw();
                            return;
                        }
                    }
                }
            }
            Event::Window(window::Event::RedrawRequested(frame)) => {
                let state = tree.state.downcast_mut::<State>();
                let quiet =
                    state.motion.samples.back().is_some_and(|&(last, _)| {
                        !state.lifts && frame.duration_since(last) > QUIET
                    });
                if let Some(lifted) = state.motion.lifted {
                    // The zero before a scroll may come a frame ahead of it: a lift is one that
                    // nothing followed.
                    if frame.duration_since(lifted) >= SETTLED {
                        state.motion.lift(lifted);
                    } else {
                        shell.request_redraw();
                    }
                } else if quiet {
                    let last = state.motion.samples.back().map(|&(last, _)| last);
                    if let Some(last) = last {
                        // Lifted, as far as can be told, right after the last scroll.
                        state.motion.lift(last);
                    }
                } else if !state.motion.samples.is_empty() {
                    shell.request_redraw();
                }
                if state.motion.moving() {
                    let moved = state.motion.step(*frame);
                    if moved != Vector::ZERO {
                        let before = self.reach(tree, layout, renderer, Some(moved));
                        let room = before.room(moved);
                        let motion = &mut tree.state.downcast_mut::<State>().motion;
                        // At the end of the content, there's nowhere left to go.
                        let blocked = |room: f32, moved: f32| moved == 0.0 || room < moved.abs();
                        if blocked(room.x, moved.x) && blocked(room.y, moved.y) {
                            motion.stop();
                        }
                    }
                    if tree.state.downcast_ref::<State>().motion.moving() {
                        shell.request_redraw();
                    }
                }
            }
            _ => {}
        }
        self.content.as_widget_mut().update(
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
        self.content.as_widget().mouse_interaction(
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Glide<'a, M>> for Element<'a, M> {
    fn from(glide: Glide<'a, M>) -> Self {
        Element::new(glide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(8);

    /// Touchpad scrolls of `each` pixels a frame, `count` of them, from `start`.
    fn swipe(motion: &mut Motion, start: Instant, each: f32, count: u32) -> Instant {
        let mut at = start;
        for _ in 0..count {
            motion.touch(Vector::new(0.0, each), at);
            at += FRAME;
        }
        at - FRAME
    }

    #[test]
    fn speed_is_measured_over_the_last_scrolls() {
        let mut motion = Motion::default();
        // 4 px every 8 ms is 500 px a second, however long it went on.
        swipe(&mut motion, Instant::now(), 4.0, 40);
        assert!((motion.speed().y - 500.0).abs() < 1.0);
        assert!(motion.samples.len() <= 14);
    }

    #[test]
    fn lifting_flings_and_slows_to_a_stop() {
        let mut motion = Motion::default();
        let last = swipe(&mut motion, Instant::now(), 4.0, 10);
        motion.lift(last + Duration::from_millis(4));
        assert!(motion.moving());
        let mut at = last + Duration::from_millis(4);
        let mut moved = 0.0;
        for _ in 0..1000 {
            at += FRAME;
            moved += motion.step(at).y;
            if !motion.moving() {
                break;
            }
        }
        assert!(!motion.moving());
        // At most what the speed covers decaying all the way: v·τ.
        assert!(moved > 100.0 && moved <= 500.0 * theme::SCROLL_GLIDE_DECAY + 0.01);
    }

    #[test]
    fn fingers_that_rested_before_lifting_dont_fling() {
        let mut motion = Motion::default();
        let last = swipe(&mut motion, Instant::now(), 4.0, 10);
        motion.lift(last + RESTED + Duration::from_millis(1));
        assert!(!motion.moving());
    }

    #[test]
    fn wheel_steps_add_up_and_land_exactly() {
        let mut motion = Motion::default();
        let start = Instant::now();
        motion.wheel(Vector::new(0.0, 60.0), start);
        let mut at = start + FRAME;
        let mut moved = motion.step(at).y;
        assert!(moved > 0.0 && moved < 60.0);
        motion.wheel(Vector::new(0.0, 60.0), at);
        for _ in 0..200 {
            at += FRAME;
            moved += motion.step(at).y;
        }
        assert!(!motion.moving());
        assert!((moved - 120.0).abs() < 1e-3);
    }

    #[test]
    fn scrolling_again_stops_the_fling() {
        let mut motion = Motion::default();
        let last = swipe(&mut motion, Instant::now(), 4.0, 10);
        motion.lift(last);
        motion.touch(Vector::new(0.0, -1.0), last + FRAME);
        assert!(!motion.moving());
    }
}
