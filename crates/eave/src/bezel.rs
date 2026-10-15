//! The screen's black border, as the bar draws it: the notch (D-75) and the grown top edge (D-81).
//! They stand for the screen's own black, whatever the theme (D-92), so they aren't part of the
//! bar: a bar that hides on hover leaves them where they are (D-106). A solid bar's band is the
//! bar, and goes with it (D-89). The dormer's drop squishes them, within what they're grown by
//! (D-86).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use iced::advanced::Renderer as _;
use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::widget::canvas::{Cache, Path};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, window};

use crate::hand::{self, rubber_band};
use crate::notch::Notch;
use crate::spring::{HAND, MAX_FRAME, REACT, Spring};
use crate::squircle::{self, CornerShape, squircle_corners, warp};
use crate::theme::{self, Palette, Scheme};

/// The notch and the grown edge, drawn across the whole surface from its top.
pub struct Bezel<M> {
    /// The notch as the bar draws it, grown, and how far down the surface it starts.
    notch: Option<(Notch, f32)>,
    /// The top edge the bar grows: how much, and the screen's top left and right corners, which
    /// round what shows below it.
    edge: Option<(f32, [CornerShape; 2])>,
    /// How far below the grown edge a solid bar reaches, down to the islands' bottom (D-89). The
    /// notch is under it, so what the dormer's drop squishes is its bottom.
    solid: Option<f32>,
    /// The latest kick from the dormer's drop, with its number. Only one newer than the bezel has
    /// seen plays.
    kick: Option<(u64, Kick)>,
    /// Where it says how far the middle of the notch's bottom (or the edge's) is from where it
    /// rests, as last drawn, if given.
    sag: Option<Sag>,
    /// Sent on a click on the notch, or the middle of the top edge: the dormer comes out (D-106).
    on_click: Option<M>,
    /// Sent with the surface's width whenever it's laid out at a new one.
    on_width: Option<Box<dyn Fn(f32) -> M>>,
}

/// How far the middle of the bottom of what the dormer's drop comes out of is from where it
/// rests, as drawn (down is positive). The bezel draws it and writes it; the drop's neck flows
/// from there, so the two never come apart while the notch squishes.
pub type Sag = Rc<Cell<f32>>;

/// What the dormer's drop does to what it comes out of (D-86).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kick {
    /// It leaves: the notch pinches in from the sides, the edge is pulled down where it leaves.
    Pinch,
    /// It lands back: the notch flattens, the edge dents up where it lands.
    Flatten,
}

/// A bump in the edge's bottom: where its middle is, how deep (down is positive) and how wide.
type Bump = (f32, f32, f32);
/// What the grown edge was drawn from: how thick, the screen's corners, how wide, its bumps (the
/// drop's and the hand's), and the scheme it was filled in.
type EdgeDrawn = (f32, [CornerShape; 2], f32, [Bump; 2], Scheme);

/// What a press on the bezel took hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// The notch: a drag stretches it.
    Notch,
    /// The grown edge: a drag pulls its bottom.
    Edge,
    /// The middle of the top, with nothing grown there to pull: it only clicks.
    Middle,
}

/// A press on the bezel, while the button is down.
#[derive(Debug, Clone, Copy)]
struct Press {
    held: Held,
    at: Point,
    when: Instant,
    /// Whether it has moved far enough to be a drag, not a click.
    dragged: bool,
    /// Whether it's somewhere a click brings the dormer out: the notch, or the middle of the top.
    clicks: bool,
}

/// The notch and the grown edge as last drawn, and what they were drawn from.
struct State {
    cache: Cache,
    drawn: Cell<Option<(Notch, f32, f32)>>,
    edge_cache: Cache,
    edge_drawn: Cell<Option<EdgeDrawn>>,
    /// How many pixels the drop adds to the notch's width and height (negative is narrower or
    /// shorter).
    stretch_x: Spring,
    stretch_y: Spring,
    /// How deep the edge's bump is, and how wide.
    dip: Spring,
    dip_width: f32,
    /// The newest kick already played, once the bezel has had a look.
    seen: Option<u64>,
    /// The press going on, if any.
    press: Option<Press>,
    /// What was grabbed and where, kept after letting go until it's back in place.
    grab: Option<(Held, Point)>,
    /// How far the hand pulls, and how far a press squeezes the notch in from each side.
    hand_x: Spring,
    hand_y: Spring,
    inset: Spring,
    /// How wide the surface was last laid out, once it's been said.
    width: Option<f32>,
    /// The previous frame, while something moves.
    last: Option<Instant>,
}

impl State {
    fn springs(&mut self) -> [&mut Spring; 6] {
        [
            &mut self.stretch_x,
            &mut self.stretch_y,
            &mut self.dip,
            &mut self.hand_x,
            &mut self.hand_y,
            &mut self.inset,
        ]
    }

    /// How far the hand pulls the notch or the edge, while it does.
    fn pulled(&self, held: Held) -> Option<(Point, Vector)> {
        let (grabbed, at) = self.grab?;
        let pull = Vector::new(self.hand_x.value, self.hand_y.value);
        (grabbed == held && pull != Vector::ZERO).then_some((at, pull))
    }

    fn at_rest(&mut self) -> bool {
        self.springs().iter().all(|spring| spring.at_rest())
    }
}

impl<M> Default for Bezel<M> {
    fn default() -> Self {
        Self {
            notch: None,
            edge: None,
            solid: None,
            kick: None,
            sag: None,
            on_click: None,
            on_width: None,
        }
    }
}

impl<M> Bezel<M> {
    /// With the notch the bar draws, `y` down the surface.
    pub fn notch(mut self, notch: Option<Notch>, y: f32) -> Self {
        self.notch = notch.map(|notch| (notch, y));
        self
    }

    /// With the screen's top edge drawn `grow` thicker than it is, from the surface's top, and
    /// rounded into the sides by the screen's top `corners` (D-81). Nothing if it isn't grown.
    pub fn edge(mut self, grow: f32, corners: [CornerShape; 2]) -> Self {
        self.edge = Some((grow, corners));
        self
    }

    /// A solid bar: the edge reaches on `depth` below where it's grown to, and is the bar
    /// (D-89). The notch isn't drawn.
    pub fn solid(mut self, depth: Option<f32>) -> Self {
        self.solid = depth;
        self
    }

    /// The latest kick from the dormer's drop, numbered: the notch squishes (or without one, the
    /// grown edge) when a newer one comes.
    pub fn kick(mut self, kick: Option<(u64, Kick)>) -> Self {
        self.kick = kick;
        self
    }

    /// Writes where the notch's bottom (or the edge's) is drawn to `sag`, each time it's drawn.
    pub fn sag(mut self, sag: Option<Sag>) -> Self {
        self.sag = sag;
        self
    }

    /// Sends `message` on a click on the notch, or the middle of the top edge.
    pub fn on_click(mut self, message: M) -> Self {
        self.on_click = Some(message);
        self
    }

    /// Sends the surface's width whenever it's laid out at a new one.
    pub fn on_width(mut self, on_width: impl Fn(f32) -> M + 'static) -> Self {
        self.on_width = Some(Box::new(on_width));
        self
    }

    /// What's under `at` that a press takes hold of, on a surface `width` wide: the notch, the
    /// grown edge (not a solid bar's band, which is the bar's), or without a notch, the middle of
    /// the top above the islands.
    fn hit(&self, at: Point, width: f32) -> Option<Held> {
        if let Some((notch, y)) = self.drawn_notch() {
            let x = (width - notch.width) / 2.0;
            let inside = at.x >= x - notch.shoulder
                && at.x <= x + notch.width + notch.shoulder
                && at.y <= y + notch.height;
            if inside {
                return Some(Held::Notch);
            }
        }
        let grow = self.edge.map_or(0.0, |(grow, _)| grow);
        if self.solid.is_none() && grow > 0.0 && at.y <= grow {
            return Some(Held::Edge);
        }
        (self.notch.is_none() && self.middle(at, width)).then_some(Held::Middle)
    }

    /// Whether a click on `held` at `at` brings the dormer out: on the notch, or without one, in
    /// the middle of the top.
    fn clicks(&self, held: Held, at: Point, width: f32) -> bool {
        match held {
            Held::Notch | Held::Middle => true,
            Held::Edge => self.notch.is_none() && self.middle(at, width),
        }
    }

    /// Whether `at` is in the middle of the top, above the islands, on a surface `width` wide.
    fn middle(&self, at: Point, width: f32) -> bool {
        let grow = self.edge.map_or(0.0, |(grow, _)| grow);
        (at.x - width / 2.0).abs() <= theme::EDGE_CLICK_WIDTH / 2.0
            && at.y <= grow + f32::from(theme::EDGE_GAP)
    }

    /// The notch the bar draws, if any: none under a solid bar.
    fn drawn_notch(&self) -> Option<(Notch, f32)> {
        self.notch.filter(|_| self.solid.is_none())
    }

    /// The band drawn from the surface's top: how far down the grown edge is, how thick the band
    /// is, and the corners rounding its lower ends. None when there's nothing to draw.
    fn band(&self) -> Option<(f32, f32, [CornerShape; 2])> {
        let (grow, corners) = self.edge?;
        let thick = grow + self.solid.unwrap_or(0.0);
        (thick > 0.0).then_some((grow, thick, corners))
    }

    /// Sets the notch or the edge off, no further than it's grown: it never gets smaller than it
    /// is, and with nothing grown, nothing moves.
    fn play(&self, state: &mut State, kick: Kick) {
        let react = |peak: f32| REACT.kick(peak * theme::REACT_SIZE);
        if let Some((notch, _)) = self.drawn_notch() {
            match kick {
                Kick::Pinch => {
                    let pinch = theme::NOTCH_PINCH.min(notch.grow[0]);
                    state.stretch_x.velocity = -react(2.0 * pinch);
                }
                Kick::Flatten => {
                    let flatten = theme::NOTCH_FLATTEN.min(notch.grow[1]);
                    state.stretch_y.velocity = -react(flatten);
                }
            }
        } else if let Some((grow, _, _)) = self.band() {
            let (depth, width) = match kick {
                Kick::Pinch => (theme::EDGE_PULL, theme::EDGE_PULL_WIDTH),
                Kick::Flatten => (-theme::EDGE_DENT.min(grow), theme::EDGE_DENT_WIDTH),
            };
            state.dip.velocity = react(depth);
            state.dip_width = width;
        }
    }
}

/// Where the hand's pull takes the point `at` of the notch whose top is at `top` and which is
/// `height` tall, grabbed at `grab`: toward the pull, most near the grab and not at all along its
/// top, which hangs from the screen. Never further in than it's grown (D-75).
fn pull_notch(
    notch: Notch,
    top: f32,
    middle: f32,
    at: Point,
    (grab, pull): (Point, Vector),
) -> Point {
    let near = hand::near(at.distance(grab));
    let down = ((at.y - top) / notch.height.max(1.0)).clamp(0.0, 1.0);
    let hung = near * down * down * (3.0 - 2.0 * down);
    let (dx, dy) = (pull.x * hung, pull.y * hung);
    let dx = if at.x < middle {
        dx.min(notch.grow[0])
    } else {
        dx.max(-notch.grow[0])
    };
    Point::new(at.x + dx, at.y + dy.max(-notch.grow[1]))
}

/// `notch` stretched `x` wider and `y` taller (negative is narrower or shorter), each pulling the
/// other in by the bulge. Never smaller than the hardware's: no narrower than its grown sides
/// take away, nor shorter than its grown bottom (D-75).
fn squished(notch: Notch, x: f32, y: f32) -> Notch {
    if x == 0.0 && y == 0.0 {
        return notch;
    }
    let width = (notch.width + x - theme::BULGE * y).max(notch.width - 2.0 * notch.grow[0]);
    let height = (notch.height + y - theme::BULGE * x).max(notch.height - notch.grow[1]);
    Notch {
        width,
        height,
        ..notch
    }
}

/// How far below `grow` the band's bottom is at `x`, in a bump `depth` deep and `width` wide end
/// to end, centered at `center`: eased in and out, flat beyond it. Never above the band's top.
fn bump_at(x: f32, (center, depth, width): Bump, grow: f32) -> f32 {
    let along = (x - center) / (width / 2.0);
    if along.abs() >= 1.0 || width <= 0.0 {
        return 0.0;
    }
    let eased = 0.5 * (1.0 + (std::f32::consts::PI * along).cos());
    (depth * eased).max(-grow)
}

/// Both bumps at `x`, together, never above the band's top.
fn bumps_at(x: f32, bumps: [Bump; 2], grow: f32) -> f32 {
    bumps
        .iter()
        .map(|bump| bump_at(x, *bump, grow))
        .sum::<f32>()
        .max(-grow)
}

/// The grown top edge across `width`, `grow` tall, in a frame its own size: a band whose lower
/// ends curve down into the sides, the way the screen's top `corners` round what shows below it.
/// The frame is as tall as the band and the deeper of its curves.
fn edge_outline(grow: f32, corners: [CornerShape; 2], width: f32, bumps: [Bump; 2]) -> (Path, f32) {
    /// How many straight pieces the bumped stretch of the bottom is drawn with.
    const PIECES: u16 = 48;
    let budget = width / 2.0;
    let [left, right] =
        corners.map(|corner| squircle::Corner::new(corner.radius, corner.smoothing, budget));
    let path = Path::new(|builder| {
        builder.move_to(Point::ORIGIN);
        builder.line_to(Point::new(width, 0.0));
        // Down the right side to where the corner starts, round it back the way what shows below
        // turns there, along the band's bottom, and round the left one down the left side.
        builder.line_to(Point::new(width, grow + right.extent()));
        squircle::corner_into(
            builder,
            &right,
            Point::new(width, grow),
            squircle::UP,
            squircle::LEFT,
            false,
        );
        // The bumps along the bottom, right to left, clear of the corners.
        let (from, to) = (left.extent(), width - right.extent());
        let bumped = bumps
            .iter()
            .filter(|(_, depth, width)| *depth != 0.0 && *width > 0.0)
            .fold(None, |span: Option<(f32, f32)>, (center, _, width)| {
                let (start, end) = (center - width / 2.0, center + width / 2.0);
                Some(span.map_or((start, end), |(a, b)| (a.min(start), b.max(end))))
            })
            .map(|(start, end)| (start.max(from), end.min(to)))
            .filter(|(start, end)| end > start);
        if let Some((start, end)) = bumped {
            for piece in 0..=PIECES {
                let x = end - (end - start) * f32::from(piece) / f32::from(PIECES);
                builder.line_to(Point::new(x, grow + bumps_at(x, bumps, grow)));
            }
        }
        squircle::corner_into(
            builder,
            &left,
            Point::new(0.0, grow),
            squircle::LEFT,
            squircle::DOWN,
            false,
        );
        builder.close();
    });
    let deepest = bumps
        .iter()
        .map(|(_, depth, _)| depth.max(0.0))
        .sum::<f32>();
    (path, grow + left.extent().max(right.extent()).max(deepest))
}

/// The notch starting `y` down the surface, in a frame its own size: what's drawn, and the frame's
/// top left on the surface, centered in its `width`. Hanging from the screen's
/// top edge, it's rounded outwards there like the screen's is, and reaches past it out of sight;
/// lower down, it's a cut-out, rounded all round.
fn notch_outline(notch: Notch, y: f32, width: f32) -> (Path, Point) {
    let lower = CornerShape::new(notch.radius, notch.smoothing);
    let x = (width - notch.width) / 2.0;
    if notch.top > 0.0 {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(notch.width, notch.height));
        return (squircle_corners(bounds, [lower; 4]), Point::new(x, y));
    }
    // Room beside it for the shoulders, and above it to reach past the edge.
    let room = Vector::new(notch.shoulder * 2.0, theme::ATTACH_OVERHANG);
    let path = Path::new(|builder| {
        let bounds = Rectangle::new(
            Point::new(room.x, 0.0),
            Size::new(notch.width, room.y + notch.height),
        );
        let corners = [CornerShape::SQUARE, CornerShape::SQUARE, lower, lower];
        let shoulder = CornerShape::new(notch.shoulder, notch.smoothing);
        squircle::hanging(builder, bounds, corners, room.y, shoulder, [true; 2]);
    });
    (path, Point::new(x, y) - room)
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Bezel<M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let palette = Palette::of(theme);
        let bounds = layout.bounds();
        if let Some((_, thick, corners)) = self.band() {
            let width = bounds.width;
            let dip = (width / 2.0, state.dip.value, state.dip_width);
            // The hand pulls the edge's bottom down (or up, no higher than its top) in a long
            // curve under the pointer, which it follows across.
            let hand = state
                .pulled(Held::Edge)
                .map_or((0.0, 0.0, 0.0), |(at, pull)| {
                    (at.x + pull.x, pull.y, 2.0 * theme::DRAG_REACH)
                });
            let bumps = [dip, hand];
            if self.drawn_notch().is_none()
                && let Some(sag) = &self.sag
            {
                sag.set(bump_at(width / 2.0, dip, thick));
            }
            let drawn = (thick, corners, width, bumps, palette.scheme);
            if state.edge_drawn.replace(Some(drawn)) != Some(drawn) {
                state.edge_cache.clear();
            }
            let (path, height) = edge_outline(thick, corners, width, bumps);
            let color = if self.solid.is_some() {
                palette.island_bg
            } else {
                theme::BEZEL
            };
            let geometry = state
                .edge_cache
                .draw(renderer, Size::new(width, height), |frame| {
                    frame.fill(&path, color);
                });
            renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
                renderer.draw_geometry(geometry);
            });
        }
        if let Some((rest, y)) = self.drawn_notch() {
            // Pinched by the drop, and squeezed in by a press, within its grown sides.
            let x = state.stretch_x.value - 2.0 * state.inset.value;
            let notch = squished(rest, x, state.stretch_y.value);
            if let Some(sag) = &self.sag {
                sag.set(notch.height - rest.height);
            }
            let pulled = state.pulled(Held::Notch);
            let drawn = (notch, y, bounds.width);
            if state.drawn.replace(Some(drawn)) != Some(drawn) || pulled.is_some() {
                state.cache.clear();
            }
            let (path, at) = notch_outline(notch, y, bounds.width);
            // Room all round for the hand's pull.
            let room = Vector::new(theme::DRAG_LIMIT_X, theme::DRAG_LIMIT_Y);
            let size = Size::new(
                notch.width + notch.shoulder * 4.0 + 2.0 * room.x,
                notch.height + theme::ATTACH_OVERHANG + 2.0 * room.y,
            );
            let origin = Vector::new(bounds.x + at.x, bounds.y + at.y) - room;
            let geometry = state.cache.draw(renderer, size, |frame| {
                let path = match pulled {
                    Some((grab, pull)) => {
                        // In the frame's coordinates, where the notch's top and middle are.
                        let grab = grab - origin;
                        let top = y - at.y + room.y;
                        let middle = size.width / 2.0;
                        warp(&path, |point| {
                            pull_notch(notch, top, middle, point + room, (grab, pull)) - room
                        })
                    }
                    None => path,
                };
                frame.with_save(|frame| {
                    frame.translate(room);
                    frame.fill(&path, theme::BEZEL);
                });
            });
            renderer.with_translation(origin, |renderer| {
                renderer.draw_geometry(geometry);
            });
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            cache: Cache::new(),
            drawn: Cell::new(None),
            edge_cache: Cache::new(),
            edge_drawn: Cell::new(None),
            stretch_x: Spring::new(0.0, REACT),
            stretch_y: Spring::new(0.0, REACT),
            dip: Spring::new(0.0, REACT),
            dip_width: 0.0,
            seen: None,
            press: None,
            grab: None,
            hand_x: Spring::new(0.0, HAND),
            hand_y: Spring::new(0.0, HAND),
            inset: Spring::new(0.0, HAND),
            width: None,
            last: None,
        })
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        if state.width != Some(bounds.width) {
            state.width = Some(bounds.width);
            if let Some(on_width) = &self.on_width {
                shell.publish(on_width(bounds.width));
            }
        }
        // A new bezel only notes the kick it's given: it happened before it.
        let number = self.kick.map_or(0, |(number, _)| number);
        if let (Some(seen), Some((number, kick))) = (state.seen, self.kick)
            && number > seen
        {
            self.play(state, kick);
        }
        state.seen = Some(state.seen.map_or(number, |seen| seen.max(number)));
        self.hand(state, event, bounds, cursor, shell);
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && !state.at_rest()
        {
            let elapsed = state
                .last
                .map_or(Duration::ZERO, |last| now.saturating_duration_since(last))
                .min(MAX_FRAME)
                .as_secs_f32();
            state.last = Some(*now);
            for spring in state.springs() {
                spring.advance(elapsed);
            }
            if state.at_rest() {
                state.springs().into_iter().for_each(Spring::settle);
                state.last = None;
                if state.press.is_none() {
                    state.grab = None;
                }
            }
        }
        if !state.at_rest() {
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        // Hovering keeps the plain arrow: the notch and the edge are part of the screen, as a
        // physical notch is, not buttons. Only a drag shows the hand.
        let state = tree.state.downcast_ref::<State>();
        if state.press.is_some() {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::None
        }
    }
}

impl<M: Clone> Bezel<M> {
    /// What the hand does: a press takes hold of the notch or the edge (and squeezes the notch a
    /// little), a drag stretches it toward the pointer, harder the further it goes, and letting go
    /// springs it back. A press let go of soon, without dragging, is a click.
    fn hand(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, M>,
    ) {
        match event {
            // Only what nothing above took: the islands, a popover, the dormer.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if !shell.is_event_captured() =>
            {
                let Some(at) = cursor.land().position_in(bounds) else {
                    return;
                };
                let Some(held) = self.hit(at, bounds.width) else {
                    return;
                };
                shell.capture_event();
                let clicks = self.clicks(held, at, bounds.width);
                state.press = Some(Press {
                    held,
                    at,
                    when: Instant::now(),
                    dragged: false,
                    clicks,
                });
                state.grab = Some((held, at));
                if let (Held::Notch, Some((notch, _))) = (held, self.drawn_notch()) {
                    state.inset.target = theme::PRESS_INSET.min(notch.grow[0]);
                }
                shell.request_redraw();
            }
            // While the button is held, the compositor keeps sending motion even off the surface.
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let Some(press) = &mut state.press else {
                    return;
                };
                let to = *position - Vector::new(bounds.x, bounds.y);
                let (dx, dy) = (to.x - press.at.x, to.y - press.at.y);
                press.dragged |= dx.hypot(dy) > theme::CLICK_SLOP;
                let held = press.held;
                let up = match (held, self.drawn_notch(), self.edge) {
                    (Held::Notch, Some((notch, _)), _) => notch.grow[1],
                    (Held::Edge, _, Some((grow, _))) => grow,
                    _ => 0.0,
                };
                if held != Held::Middle {
                    state.hand_x.target = rubber_band(dx, theme::DRAG_LIMIT_X);
                    let limit = if dy < 0.0 { up } else { theme::DRAG_LIMIT_Y };
                    state.hand_y.target = rubber_band(dy, theme::DRAG_LIMIT_Y.min(limit));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Mouse(mouse::Event::CursorLeft) => {
                let Some(press) = state.press.take() else {
                    return;
                };
                state.hand_x.target = 0.0;
                state.hand_y.target = 0.0;
                state.inset.target = 0.0;
                let released = matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                );
                if released
                    && press.clicks
                    && !press.dragged
                    && press.when.elapsed() < theme::LONG_PRESS
                    && let Some(on_click) = &self.on_click
                {
                    shell.publish(on_click.clone());
                }
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }
}

impl<'a, M: Clone + 'a> From<Bezel<M>> for Element<'a, M> {
    fn from(bezel: Bezel<M>) -> Self {
        Element::new(bezel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notch(grow: [f32; 2]) -> Notch {
        Notch {
            width: 200.0,
            height: 30.0,
            grow,
            ..Notch::default()
        }
    }

    #[test]
    fn a_squished_notch_never_gets_smaller_than_it_is() {
        let grown = notch([3.0, 2.0]);
        // Pinched hard: no narrower than the hardware's, and a little taller.
        let pinched = squished(grown, -20.0, 0.0);
        assert_eq!(pinched.width, 194.0);
        assert_eq!(pinched.height, 40.0);
        // Flattened hard: no shorter than the hardware's, and a little wider.
        let flat = squished(grown, 0.0, -10.0);
        assert_eq!(flat.height, 28.0);
        assert_eq!(flat.width, 205.0);
        // Not grown: there's nowhere to shrink to.
        let tight = squished(notch([0.0; 2]), -4.0, -4.0);
        assert!(tight.width >= 200.0 && tight.height >= 30.0, "{tight:?}");
    }

    #[test]
    fn a_bump_eases_in_and_out_and_dents_no_higher_than_the_top() {
        assert_eq!(bump_at(50.0, (50.0, 4.0, 40.0), 3.0), 4.0);
        assert_eq!(bump_at(70.0, (50.0, 4.0, 40.0), 3.0), 0.0);
        assert!((bump_at(60.0, (50.0, 4.0, 40.0), 3.0) - 2.0).abs() < 1e-4);
        assert_eq!(bump_at(50.0, (50.0, -9.0, 40.0), 3.0), -3.0);
    }

    fn state() -> State {
        let bezel: Bezel<u8> = Bezel::default();
        let tree::State::Some(state) = Widget::state(&bezel) else {
            unreachable!()
        };
        *state.downcast::<State>().unwrap()
    }

    /// Plays `events` on `bezel`, on a 1000 px wide surface, with the pointer at each one's
    /// point. What it sent.
    fn play(bezel: &Bezel<u8>, state: &mut State, events: &[(Point, mouse::Event)]) -> Vec<u8> {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 60.0));
        let mut messages = Vec::new();
        for (at, event) in events {
            let mut shell = Shell::new(&mut messages);
            bezel.hand(
                state,
                &Event::Mouse(*event),
                bounds,
                mouse::Cursor::Available(*at),
                &mut shell,
            );
        }
        messages
    }

    fn with_notch() -> Bezel<u8> {
        Bezel::default()
            .notch(Some(notch([4.0, 4.0])), 0.0)
            .edge(0.0, [CornerShape::SQUARE; 2])
            .on_click(1)
    }

    const PRESS: mouse::Event = mouse::Event::ButtonPressed(mouse::Button::Left);
    const RELEASE: mouse::Event = mouse::Event::ButtonReleased(mouse::Button::Left);

    #[test]
    fn a_click_on_the_notch_brings_the_dormer_out_and_a_drag_does_not() {
        let bezel = with_notch();
        let on = Point::new(500.0, 10.0);
        let mut state = state();
        assert_eq!(play(&bezel, &mut state, &[(on, PRESS), (on, RELEASE)]), [1]);
        // Dragged: it stretches, and springs back, but doesn't click.
        let moved = mouse::Event::CursorMoved {
            position: Point::new(540.0, 40.0),
        };
        let mut state = super::tests::state();
        let sent = play(&bezel, &mut state, &[(on, PRESS), (on, moved)]);
        assert!(sent.is_empty());
        assert!(state.hand_x.target > 0.0 && state.hand_y.target > 0.0);
        assert!(state.inset.target > 0.0, "a press squeezes it");
        let sent = play(&bezel, &mut state, &[(on, RELEASE)]);
        assert!(sent.is_empty());
        assert_eq!(state.hand_x.target, 0.0);
        // Beside the notch, nothing.
        let mut state = super::tests::state();
        let beside = Point::new(100.0, 10.0);
        assert!(play(&bezel, &mut state, &[(beside, PRESS), (beside, RELEASE)]).is_empty());
        assert!(state.press.is_none());
    }

    #[test]
    fn without_a_notch_the_middle_of_the_top_clicks() {
        let bezel: Bezel<u8> = Bezel::default()
            .edge(3.0, [CornerShape::SQUARE; 2])
            .on_click(1);
        let mut state = state();
        let middle = Point::new(520.0, 6.0);
        assert_eq!(
            play(&bezel, &mut state, &[(middle, PRESS), (middle, RELEASE)]),
            [1]
        );
        // The edge is grown, so it can be pulled anywhere along it, but only clicks in the middle.
        assert_eq!(bezel.hit(Point::new(50.0, 2.0), 1000.0), Some(Held::Edge));
        let side = Point::new(50.0, 2.0);
        assert!(play(&bezel, &mut state, &[(side, PRESS), (side, RELEASE)]).is_empty());
        let on_edge = Point::new(500.0, 2.0);
        assert_eq!(
            play(&bezel, &mut state, &[(on_edge, PRESS), (on_edge, RELEASE)]),
            [1]
        );
        // Below the islands' top, nothing.
        assert_eq!(bezel.hit(Point::new(500.0, 20.0), 1000.0), None);
    }

    #[test]
    fn a_pulled_notch_never_gets_smaller_than_it_is() {
        let notch = notch([3.0, 2.0]);
        let grab = Point::new(150.0, 25.0);
        // Pulled hard right and up, its left side and bottom go in no further than they're grown.
        let pull = (grab, Vector::new(40.0, -40.0));
        let left = pull_notch(notch, 0.0, 100.0, Point::new(10.0, 30.0), pull);
        assert!(left.x <= 13.0, "{left:?}");
        assert!(left.y >= 28.0, "{left:?}");
        let right = pull_notch(notch, 0.0, 100.0, Point::new(190.0, 30.0), pull);
        assert!(
            right.x > 190.0,
            "the side pulled toward stretches: {right:?}"
        );
        // Its top hangs from the screen, and stays.
        let top = pull_notch(notch, 0.0, 100.0, Point::new(150.0, 0.0), pull);
        assert_eq!(top, Point::new(150.0, 0.0));
    }

    #[test]
    fn the_hand_pulls_the_edge_where_it_is_and_never_above_its_top() {
        let hand = (300.0, 5.0, 2.0 * theme::DRAG_REACH);
        let none = (500.0, 0.0, 0.0);
        assert_eq!(bumps_at(300.0, [none, hand], 3.0), 5.0);
        assert_eq!(bumps_at(300.0 + theme::DRAG_REACH, [none, hand], 3.0), 0.0);
        assert_eq!(bumps_at(300.0, [none, (300.0, -9.0, 100.0)], 3.0), -3.0);
    }
}
