//! Turns the plugin vocabulary ([`eave_api::ui`]) into iced elements, styled by [`theme`].
//! This is the only place plugin UI becomes pixels.
//!
//! The same node can land in the bar or in a popover. A popover is laid out at a known width
//! ([D-40](../../../docs/decisions.md)), so there rows stretch, lists scroll and `fill` pushes
//! things to the end. The bar has no width to give, so those collapse to their content.

use std::hash::{Hash, Hasher};
use std::time::SystemTime;

use eave_api::erased::AnyMessage;
use eave_api::ui::{
    ChoiceParts, ChoicesParts, Kind, Mark, Node, NoticeParts, ReplyParts, Shortcut, Tone, Urgency,
};
use iced::widget::scrollable::{self, Scrollbar};
use iced::widget::{
    Space, button, column, container, keyed_column, mouse_area, row, rule, text, text_input,
};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Size, Theme, alignment};

use crate::alternate::Alternate;
use crate::apps::{FileKey, Icons, Wanted};
use crate::clip::{Clip, Fold};
use crate::code;
use crate::countdown::Countdown;
use crate::counter::Counter;
use crate::cover;
use crate::field::Field;
use crate::glide::Glide;
use crate::gulp::Gulp;
use crate::icon;
use crate::label::Label;
use crate::markdown;
use crate::meter::Meter;
use crate::pip::Pip;
use crate::shortcut;
use crate::slider::Slider;
use crate::slot::Slot;
use crate::sprite::Sprite;
use crate::steps::Steps;
use crate::switcher::Switcher;
use crate::tabs::Tabs;
use crate::text_area::TextArea;
use crate::theme::{self, Palette};
use crate::timeline;
use crate::toggle::Toggle;

/// A node in the bar.
pub fn node<'a>(
    node: Node<AnyMessage>,
    icons: &Icons,
    palette: &'static Palette,
) -> Element<'a, AnyMessage> {
    render(node, Ctx::new(Place::Bar, palette), icons)
}

/// A popover's content, laid out at the panel's width. When it shows a large cover with color in
/// it, its sliders and main button take that color: the content's, never one a plugin picked
/// (D-61).
pub fn panel<'a>(
    node: Node<AnyMessage>,
    icons: &Icons,
    palette: &'static Palette,
) -> Element<'a, AnyMessage> {
    in_panel(node, icons, palette, Place::Panel, theme::SCROLL_MAX_HEIGHT)
}

/// A dialog's content: a panel's, with more room to scroll in (D-90): up to `scroll` tall.
pub fn dialog<'a>(
    node: Node<AnyMessage>,
    icons: &Icons,
    palette: &'static Palette,
    scroll: f32,
) -> Element<'a, AnyMessage> {
    in_panel(node, icons, palette, Place::Dialog, scroll)
}

fn in_panel<'a>(
    node: Node<AnyMessage>,
    icons: &Icons,
    palette: &'static Palette,
    place: Place,
    scroll: f32,
) -> Element<'a, AnyMessage> {
    let content = large_cover(node.kind())
        .and_then(|url| icons.covers.get(url))
        .and_then(|cover| cover.as_ref()?.color(palette.scheme));
    let ctx = Ctx {
        content,
        scroll,
        ..Ctx::new(place, palette)
    };
    render(node, ctx, icons)
}

/// The URL of the first large cover in `kind`, if there is one.
fn large_cover<M>(kind: &Kind<M>) -> Option<&str> {
    fn first<M>(children: &[Node<M>]) -> Option<&str> {
        children.iter().find_map(|child| large_cover(child.kind()))
    }
    match kind {
        Kind::Cover {
            url, large: true, ..
        } => Some(url),
        Kind::Row(children) | Kind::Column(children) | Kind::List(children) => first(children),
        Kind::Grid { cells, .. } => first(cells),
        Kind::Tabs(pages) => pages.iter().find_map(|(_, page)| large_cover(page.kind())),
        _ => None,
    }
}

/// Notifications' cards, newest first, as one popover's content, with a line between each and
/// the next, then `footer` if there's one. Cards are keyed, so what's typed into one stays with
/// it as others come and go above it.
pub fn notices<'a, M: 'a>(
    cards: impl IntoIterator<Item = ((u64, u64), Element<'a, M>)>,
    footer: Option<Element<'a, M>>,
    p: &'static Palette,
) -> Element<'a, M> {
    // A keyed column diffs a kept child without checking it's the same kind of widget, and its
    // guess at which child is which can be off when several change at once. So every child is
    // a container, which diffs what's in it the checking way, and a card carries the line
    // above it.
    let entry = |line: bool, content: Element<'a, M>| -> Element<'a, M> {
        let line = line.then(|| separator(p));
        container(
            column![]
                .push(line)
                .push(content)
                .spacing(theme::NOTICE_SPACING),
        )
        .into()
    };
    let mut column = keyed_column(std::iter::empty()).spacing(theme::NOTICE_SPACING);
    let mut first = true;
    for (key, card) in cards {
        column = column.push((key.0, key.1), entry(!first, card));
        first = false;
    }
    if let Some(footer) = footer {
        column = column.push((u64::MAX, u64::MAX), entry(!first, footer));
    }
    column.into()
}

/// The line under the cards when more wait: how many, and a way to their list.
pub fn more_notices<'a, M: Clone + 'a>(
    count: usize,
    on_press: M,
    p: &'static Palette,
) -> Element<'a, M> {
    let label = row![
        text(format!("{count} more"))
            .size(theme::SMALL_SIZE)
            .font(theme::STRONG_FONT),
        Space::new().width(Length::Fill),
        icon::view(eave_api::ui::Icon::Forward, p.tone(Tone::Dim)),
    ]
    .align_y(alignment::Vertical::Center);
    button(label)
        .on_press(on_press)
        .width(Length::Fill)
        .padding(0)
        .style(quiet_button_style)
        .into()
}

/// A thin horizontal line between groups.
fn separator<'a, M: 'a>(p: &'static Palette) -> Element<'a, M> {
    rule::horizontal(1)
        .style(move |_| rule::Style {
            color: p.separator,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// A short vertical line between items in an island, as `divider = true` in config.toml asks.
pub fn divider<'a, M: 'a>(p: &'static Palette) -> Element<'a, M> {
    let line = rule::vertical(theme::DIVIDER_WIDTH).style(move |_| rule::Style {
        color: p.divider,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    });
    container(container(line).height(theme::DIVIDER_HEIGHT))
        .center_y(Length::Fill)
        .into()
}

/// The line height of a notification's body, relative to its size: iced's default.
const LINE_HEIGHT: f32 = 1.3;

/// A card's width: the panel's, inside its padding.
pub const CARD_WIDTH: f32 = theme::PANEL_WIDTH - 2.0 * theme::PANEL_PADDING as f32;

/// One card in the panel of notifications, as the host has it ([D-85](../../../docs/decisions.md)).
pub struct Card {
    /// The newest notice of its thread: the one shown in full.
    pub notice: NoticeParts<AnyMessage>,
    /// Earlier ones of the same thread under it, newest first: a line each.
    pub earlier: Vec<NoticeParts<AnyMessage>>,
    /// The time it has left on screen, if it expires.
    pub countdown: Option<Countdown>,
    /// Its body shows in full: "Show all" was clicked.
    pub unfolded: bool,
    /// The question it asked before, while that may still be sliding away.
    pub previous: Option<NoticeParts<AnyMessage>>,
}

/// One notification card, laid out at the panel's width ([D-85](../../../docs/decisions.md)):
///
/// - the time it has left, if it expires;
/// - where it's from: the app's icon and name, its thread, which question of several it is, and
///   when it came, which turns into a close button under the pointer;
/// - what it says: its summary and body, cut after a few lines, with its own picture beside them,
///   or under them, full width, if the picture is wide;
/// - the earlier notices of its thread, a line each;
/// - how far along it is;
/// - what to answer: its options, then its reply field and actions.
///
/// A click anywhere else on it sends its `on_press`. Its widget's messages are wrapped by `wrap`;
/// `engaged` hears the reply field get the focus from a click, or lose it. `dismiss` is what its
/// close button sends, for the whole stack; `unfold`, what "Show all" sends.
#[allow(clippy::too_many_arguments)]
pub fn notice<'a, M: Clone + 'a>(
    card: Card,
    icons: &Icons,
    p: &'static Palette,
    now: SystemTime,
    wrap: impl Fn(AnyMessage) -> M + Clone + 'a,
    engaged: impl Fn(bool) -> M + Clone + 'a,
    dismiss: Option<M>,
    unfold: M,
) -> Element<'a, M> {
    let Card {
        notice,
        earlier,
        countdown,
        unfolded,
        previous,
    } = card;
    let on_press = notice.on_press.clone();
    let source = source(&notice, icons, p, now, wrap.clone(), dismiss);
    let step = notice.step.as_ref().map(|step| step.index);
    let content = |notice: NoticeParts<AnyMessage>, earlier, unfolded| {
        content(
            notice,
            earlier,
            unfolded,
            icons,
            p,
            now,
            wrap.clone(),
            engaged.clone(),
            unfold.clone(),
        )
    };
    let current = content(notice, earlier, unfolded);
    // A question of several slides over to the next.
    let content = match step {
        Some(index) => {
            let previous = previous
                .and_then(|previous| Some((previous.step.as_ref()?.index, previous)))
                .map(|(at, previous)| (at, content(previous, Vec::new(), false)));
            Steps::new(index, current, previous).into()
        }
        None => current,
    };
    let head = column![].push(countdown).push(source).spacing(4);
    let card = column![head, content]
        .spacing(theme::NOTICE_GAP)
        .width(Length::Fill);
    // The slot tells the time in the corner that the pointer is on the card, to turn into the
    // close button.
    let slot = Slot::new(card);
    match on_press {
        Some(message) => slot.on_press(wrap(message)).into(),
        None => slot.into(),
    }
}

/// Where a notification is from, its place among several questions, and when it came: that last
/// turns into its close button under the pointer, so the card is quiet at rest.
fn source<'a, M: Clone + 'a>(
    notice: &NoticeParts<AnyMessage>,
    icons: &Icons,
    p: &'static Palette,
    now: SystemTime,
    wrap: impl Fn(AnyMessage) -> M + 'a,
    dismiss: Option<M>,
) -> Element<'a, M> {
    let critical = notice.urgency == Urgency::Critical;
    let tone = if critical { Tone::Critical } else { Tone::Dim };
    let small = |content: String, tone: Tone, strong: bool| {
        text(content)
            .size(theme::SMALL_SIZE)
            .font(if strong {
                theme::STRONG_FONT
            } else {
                Font::DEFAULT
            })
            .color(p.tone(tone))
            .wrapping(text::Wrapping::None)
    };
    // Every notification has an icon: its symbol, else its app's, else a bell. One still loading
    // keeps its place empty, so nothing moves when it arrives.
    let symbol = |symbol| icon::view(symbol, p.tone(Tone::Normal));
    let bell = || symbol(eave_api::ui::Icon::Bell { muted: false });
    let blank = || -> Element<'a, M> {
        Space::new()
            .width(theme::ICON_SIZE)
            .height(theme::ICON_SIZE)
            .into()
    };
    let app = match (
        notice.icon,
        notice.app_icon.as_deref().or(notice.app.as_deref()),
    ) {
        (Some(icon), _) => symbol(icon),
        (None, None) => bell(),
        // A file the app gave for its icon is drawn like a cover, at icon size.
        (None, Some(app_id)) if is_file(app_id) => match icons.covers.get(app_id) {
            Some(Some(cover)) => cover.view(false, 1.0),
            Some(None) => bell(),
            None => blank(),
        },
        (None, Some(app_id)) => match icons.apps.get(app_id) {
            Some(Some(_)) => app_icon(app_id, Tone::Normal, icons, p).map(wrap),
            Some(None) => bell(),
            None => blank(),
        },
    };
    let app = container(app).center(theme::ICON_SIZE);
    let dot = critical.then(|| {
        text("●")
            .size(theme::NOTICE_DOT)
            .color(p.tone(Tone::Critical))
    });
    // The last words take the free room, so the time is at the far end. A `Clip` is only as wide
    // as its text, even when given more.
    let name = notice.app.clone().map(|name| small(name, tone, true));
    let thread = notice
        .thread_name
        .clone()
        .map(|thread| small(thread, Tone::Dim, false));
    let (name, last): (Option<Element<'a, M>>, Element<'a, M>) = match (name, thread) {
        (name, Some(thread)) => (
            name.map(Element::from),
            container(Clip::new(thread, p.island_bg))
                .width(Length::Fill)
                .into(),
        ),
        (Some(name), None) => (
            None,
            container(Clip::new(name, p.island_bg))
                .width(Length::Fill)
                .into(),
        ),
        (None, None) => (None, Space::new().width(Length::Fill).into()),
    };
    let dots = notice
        .step
        .as_ref()
        .filter(|step| step.count > 1)
        .map(|step| {
            row((0..step.count).map(|index| {
                let color = match index.cmp(&step.index) {
                    std::cmp::Ordering::Less => p.tone(Tone::Dim),
                    std::cmp::Ordering::Equal => p.tone(Tone::Accent),
                    std::cmp::Ordering::Greater => p.track,
                };
                container(Space::new())
                    .width(theme::STEP_DOT)
                    .height(theme::STEP_DOT)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(color)),
                        border: Border::default().rounded(theme::STEP_DOT / 2.0),
                        ..container::Style::default()
                    })
                    .into()
            }))
            .spacing(4)
            .align_y(alignment::Vertical::Center)
        });
    let close = dismiss.map(|message| -> Element<'a, M> {
        button(
            container(icon::view(eave_api::ui::Icon::Close, p.tone(Tone::Dim)))
                .center(Length::Fixed(theme::NOTICE_CLOSE)),
        )
        .on_press(message)
        .padding(0)
        .style(icon_button_style)
        .into()
    });
    let time = notice.time.map(|time| -> Element<'a, M> {
        container(small(ago(time, now), Tone::Dim, false))
            .center_y(Length::Fixed(theme::NOTICE_CLOSE))
            .into()
    });
    // At rest the time, or nothing; under the pointer the close button.
    let corner: Option<Element<'a, M>> = match (time, close) {
        (time, Some(close)) => {
            let rest = time.unwrap_or_else(|| {
                Space::new()
                    .width(theme::NOTICE_CLOSE)
                    .height(theme::NOTICE_CLOSE)
                    .into()
            });
            Some(Alternate::new(vec![rest, close], 0, Some(1), false).into())
        }
        (time, None) => time,
    };
    row![]
        .push(app)
        .push(dot)
        .push(name)
        .push(last)
        .push(dots)
        .push(corner)
        .spacing(theme::SPACING)
        .height(Length::Fixed(theme::NOTICE_CLOSE))
        .align_y(alignment::Vertical::Center)
        .into()
}

/// Whether a notice's app icon is a file's URL rather than an app id.
pub fn is_file(app_icon: &str) -> bool {
    app_icon.starts_with("file://")
}

/// How long ago `then` was, short: now, 5 min, 2 h, 3 d.
pub fn ago(then: SystemTime, now: SystemTime) -> String {
    let seconds = now.duration_since(then).unwrap_or_default().as_secs();
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{} min", seconds / 60),
        3600..86_400 => format!("{} h", seconds / 3600),
        _ => format!("{} d", seconds / 86_400),
    }
}

/// What a notification says and offers: everything under where it's from.
#[allow(clippy::too_many_arguments)]
fn content<'a, M: Clone + 'a>(
    notice: NoticeParts<AnyMessage>,
    earlier: Vec<NoticeParts<AnyMessage>>,
    unfolded: bool,
    icons: &Icons,
    p: &'static Palette,
    now: SystemTime,
    wrap: impl Fn(AnyMessage) -> M + Clone + 'a,
    engaged: impl Fn(bool) -> M + 'a,
    unfold: M,
) -> Element<'a, M> {
    let ctx = Ctx::new(Place::Panel, p);
    // Its own picture: wide ones whole, under the text; the rest beside it.
    let cover = cover::key(&notice)
        .and_then(|key| icons.covers.get(&key))
        .and_then(Option::as_ref);
    let wide = cover.and_then(|cover| cover.view_wide::<M>(CARD_WIDTH));
    let picture = cover
        .filter(|_| wide.is_none())
        .map(|cover| cover.view_at::<M>(theme::NOTICE_PICTURE, 1.0));
    let beside = picture.is_some();

    let summary_tone = if notice.urgency == Urgency::Low {
        Tone::Dim
    } else {
        Tone::Normal
    };
    let summary = text(notice.summary)
        .size(theme::TEXT_SIZE)
        .font(theme::STRONG_FONT)
        .color(p.tone(summary_tone))
        .wrapping(text::Wrapping::WordOrGlyph);
    let markdown = notice.markdown;
    let body = notice.body.filter(|body| !body.is_empty()).map(|body| {
        let line = (theme::TEXT_SIZE * LINE_HEIGHT).ceil();
        let body: Element<'a, M> = if markdown {
            markdown::view(&body, p.tone(Tone::Dim), p)
        } else {
            text(body)
                .size(theme::TEXT_SIZE)
                .line_height(LINE_HEIGHT)
                .color(p.tone(Tone::Dim))
                .wrapping(text::Wrapping::WordOrGlyph)
                .into()
        };
        if unfolded {
            body
        } else {
            let more = button(text("Show all").size(theme::SMALL_SIZE))
                .on_press(unfold)
                .padding(0)
                .style(quiet_button_style);
            Fold::new(body, p.island_bg, line * theme::NOTICE_BODY_LINES)
                .more(more)
                .into()
        }
    });
    let text_column = column![summary]
        .push(body)
        .spacing(theme::NOTICE_SUMMARY_GAP)
        .width(Length::Fill);
    let words = row![]
        .push(picture)
        .push(text_column)
        .spacing(theme::NOTICE_PICTURE_GAP);

    // The earlier ones of its thread, a line each, under its text.
    let earlier = (!earlier.is_empty()).then(|| {
        let lines = earlier.into_iter().map(|notice| -> Element<'a, M> {
            let said = notice
                .body
                .as_deref()
                .and_then(|body| body.lines().find(|line| !line.trim().is_empty()))
                .map_or(notice.summary.clone(), str::to_string);
            let when = notice.time.map(|time| {
                text(ago(time, now))
                    .size(theme::SMALL_SIZE)
                    .color(p.tone(Tone::Dim))
            });
            row![Clip::new(
                text(said)
                    .size(theme::TEXT_SIZE - 1.0)
                    .color(p.tone(Tone::Dim))
                    .wrapping(text::Wrapping::None),
                p.island_bg,
            )]
            .push(when)
            .spacing(theme::SPACING)
            .align_y(alignment::Vertical::Center)
            .into()
        });
        let indent = if beside {
            theme::NOTICE_PICTURE + theme::NOTICE_PICTURE_GAP
        } else {
            0.0
        };
        container(column(lines).spacing(6)).padding(Padding::ZERO.left(indent))
    });

    let progress = notice.progress.map(|fraction| {
        row![
            Meter::new(fraction, None, p.tone(Tone::Normal)),
            text(format!("{:.0}%", fraction * 100.0))
                .size(theme::SMALL_SIZE)
                .color(p.tone(Tone::Dim)),
        ]
        .spacing(theme::SPACING)
        .align_y(alignment::Vertical::Center)
    });

    let answers = answers(
        notice.choices,
        notice.step.and_then(|step| step.on_back),
        notice.reply,
        notice.actions,
        ctx,
        icons,
        wrap,
        engaged,
    );
    let answers = answers.map(|answers| {
        container(answers).padding(Padding::ZERO.top(theme::NOTICE_REPLY_GAP - theme::NOTICE_GAP))
    });
    let wide = wide.map(|wide| container(wide).width(Length::Fill));
    column![words]
        .push(wide)
        .push(earlier)
        .push(progress)
        .push(answers)
        .spacing(theme::NOTICE_GAP)
        .width(Length::Fill)
        .into()
}

/// What to answer a notification with: its options, then its reply field, then its buttons. A
/// reply with at most two buttons beside it shares their row; with more, the buttons get a row of
/// their own under it, sharing its width.
#[allow(clippy::too_many_arguments)]
fn answers<'a, M: Clone + 'a>(
    choices: Option<ChoicesParts<AnyMessage>>,
    on_back: Option<AnyMessage>,
    reply: Option<ReplyParts<AnyMessage>>,
    actions: Vec<(String, AnyMessage)>,
    ctx: Ctx,
    icons: &Icons,
    wrap: impl Fn(AnyMessage) -> M + Clone + 'a,
    engaged: impl Fn(bool) -> M + 'a,
) -> Option<Element<'a, M>> {
    let p = ctx.palette;
    let mut buttons: Vec<(String, Option<AnyMessage>, bool)> = Vec::new();
    if let Some(back) = on_back {
        buttons.push(("Back".into(), Some(back), false));
    }
    let mut options = None;
    if let Some(choices) = choices {
        let picked = choices
            .options
            .iter()
            .filter(|option| option.picked)
            .count();
        if choices.multiple {
            let label = if picked > 0 {
                format!("Send {picked}")
            } else {
                "Send".into()
            };
            buttons.push((label, choices.on_send.filter(|_| picked > 0), picked > 0));
        }
        let multiple = choices.multiple;
        let rows: Vec<Element<'a, M>> = choices
            .options
            .into_iter()
            .map(|option| choice(option, multiple, ctx.palette).map(wrap.clone()))
            .collect();
        options = Some(column(rows).spacing(2));
    }
    // At most three: the first action is the main one, unless Send already is.
    let main_taken = buttons.iter().any(|(_, _, prominent)| *prominent);
    let room = 3usize.saturating_sub(buttons.len());
    buttons.extend(
        actions
            .into_iter()
            .take(room)
            .enumerate()
            .map(|(index, (label, message))| (label, Some(message), index == 0 && !main_taken)),
    );

    let reply = reply.map(|reply| -> Element<'a, M> {
        if reply.sent {
            return row![
                icon::view(eave_api::ui::Icon::Check, p.tone(Tone::Normal)),
                text("Sent")
                    .size(theme::SMALL_SIZE)
                    .color(p.tone(Tone::Dim)),
            ]
            .spacing(6)
            .height(theme::BUTTON_HEIGHT)
            .align_y(alignment::Vertical::Center)
            .into();
        }
        // The send button only once there's something to send.
        let send = reply
            .on_submit
            .clone()
            .filter(|_| !reply.value.trim().is_empty())
            .map(|message| {
                let send =
                    eave_api::ui::button(eave_api::ui::icon(eave_api::ui::Icon::Forward), message)
                        .prominent();
                render(send, ctx, icons)
            });
        let on_input = reply.on_input;
        let area = TextArea::new(
            reply.placeholder,
            reply.value,
            move |text| on_input(text),
            reply.on_submit,
        );
        let content: Element<'a, AnyMessage> = row![area]
            .push(send)
            .spacing(4)
            .padding(Padding::ZERO.right(3))
            .align_y(alignment::Vertical::Bottom)
            .into();
        // It waits for a click before it takes the focus, and the bar the keyboard (D-62).
        Field::new(content.map(wrap.clone()), None)
            .grows()
            .on_focus(engaged)
            .into()
    });

    let shares = reply.is_some() && buttons.len() <= 2;
    let buttons: Vec<Element<'a, M>> = buttons
        .into_iter()
        .map(|(label, message, prominent)| {
            let width = if shares { Length::Shrink } else { Length::Fill };
            answer_button(label, message, prominent, width, ctx).map(wrap.clone())
        })
        .collect();
    let bottom: Option<Element<'a, M>> = match (reply, buttons.is_empty()) {
        (Some(reply), false) if shares => Some(
            row![reply]
                .extend(buttons)
                .spacing(theme::SPACING)
                .align_y(alignment::Vertical::Bottom)
                .into(),
        ),
        (Some(reply), false) => Some(
            column![row(buttons).spacing(theme::SPACING), reply]
                .spacing(theme::SPACING)
                .into(),
        ),
        (Some(reply), true) => Some(reply),
        (None, false) => Some(row(buttons).spacing(theme::SPACING).into()),
        (None, true) => None,
    };
    if options.is_none() && bottom.is_none() {
        return None;
    }
    Some(
        column![]
            .push(options)
            .push(bottom)
            .spacing(theme::SPACING)
            .into(),
    )
}

/// A button among a notification's answers: a label, filled with the ink if it's the main one, and as
/// wide as `width` gives it.
fn answer_button<'a>(
    label: String,
    message: Option<AnyMessage>,
    prominent: bool,
    width: Length,
    ctx: Ctx,
) -> Element<'a, AnyMessage> {
    let p = ctx.palette;
    let content = text(label)
        .size(theme::TEXT_SIZE)
        .font(theme::STRONG_FONT)
        .color(if prominent {
            p.on_accent
        } else {
            p.tone(Tone::Normal)
        })
        .wrapping(text::Wrapping::None);
    let fill = prominent.then(|| ctx.fill());
    let enabled = message.is_some();
    button(container(content).center_x(width).center_y(Length::Fill))
        .on_press_maybe(message)
        .width(width)
        .height(theme::BUTTON_HEIGHT)
        .padding([0, 12])
        .style(move |theme: &Theme, status| {
            let style = text_button_style(theme, status, fill);
            if enabled {
                style
            } else {
                // Nothing to send yet: there, but faint.
                button::Style {
                    background: style
                        .background
                        .map(|background| background.scale_alpha(0.5)),
                    ..style
                }
            }
        })
        .into()
}

/// One option to pick: its label, with "Recommended" beside it if it is, and what it means
/// under it. A picked one is lit like the list row in use; with several to pick, a check box says
/// which.
fn choice<'a>(
    option: ChoiceParts<AnyMessage>,
    multiple: bool,
    p: &'static Palette,
) -> Element<'a, AnyMessage> {
    let picked = option.picked;
    let label = text(option.label)
        .size(theme::TEXT_SIZE)
        .font(if picked {
            theme::STRONG_FONT
        } else {
            Font::DEFAULT
        })
        .color(p.tone(if picked { Tone::Accent } else { Tone::Normal }))
        .wrapping(text::Wrapping::WordOrGlyph);
    let tag = option.recommended.then(|| {
        container(
            text("Recommended")
                .size(theme::SMALL_SIZE - 1.0)
                .color(p.tone(Tone::Dim)),
        )
        .padding([0, 5])
        .style(move |_| container::Style {
            border: Border {
                color: p.badge_border,
                width: 1.0,
                radius: theme::BADGE_RADIUS.into(),
            },
            ..container::Style::default()
        })
    });
    let title = row![label]
        .push(tag)
        .spacing(7)
        .align_y(alignment::Vertical::Center);
    let description = option.description.map(|description| {
        text(description)
            .size(theme::SMALL_SIZE)
            .color(p.tone(Tone::Dim))
            .wrapping(text::Wrapping::WordOrGlyph)
    });
    let words = column![title]
        .push(description)
        .spacing(1)
        .width(Length::Fill);
    let check = multiple.then(|| -> Element<'a, AnyMessage> {
        let mark = picked.then(|| icon::view(eave_api::ui::Icon::Check, p.on_accent));
        container(mark)
            .center(Length::Fixed(theme::CHECK_SIZE))
            .style(move |_| container::Style {
                background: picked.then_some(Background::Color(p.accent_fill)),
                border: Border {
                    color: if picked { p.accent_fill } else { p.check_ring },
                    width: if picked { 0.0 } else { 1.5 },
                    radius: 5.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    });
    let content = row![]
        .push(check)
        .push(words)
        .spacing(10)
        .align_y(alignment::Vertical::Top);
    button(content)
        .on_press(option.on_pick)
        .width(Length::Fill)
        .padding([7, 6])
        .style(move |theme: &Theme, status| row_style(theme, status, picked))
        .into()
}

/// A hash of what `node` says: its kinds and its text. Two nodes that read the same hash the
/// same, so a timeline can tell its steps apart from one view to the next.
pub fn fingerprint<M>(node: &Node<M>, hasher: &mut impl Hasher) {
    let kind = node.kind();
    std::mem::discriminant(kind).hash(hasher);
    let all = |children: &mut dyn Iterator<Item = &Node<M>>, hasher: &mut _| {
        children.for_each(|child| fingerprint(child, hasher))
    };
    match kind {
        Kind::Label { text, .. }
        | Kind::Heading { text, .. }
        | Kind::Badge { text, .. }
        | Kind::Markdown { text, .. }
        | Kind::Literal { text, .. }
        | Kind::Divider { text } => text.hash(hasher),
        Kind::Bay { name, subject } => (name, subject).hash(hasher),
        Kind::AppIcon { app_id, .. } => app_id.hash(hasher),
        Kind::Cover { url, .. } => url.hash(hasher),
        Kind::Counter { value, unit, .. } => (value, unit).hash(hasher),
        Kind::Keys { shortcut, .. } => shortcut::labels(shortcut).hash(hasher),
        Kind::Counted { content, count } => {
            count.hash(hasher);
            fingerprint(content, hasher);
        }
        Kind::Diff { title, lines } => {
            title.hash(hasher);
            for (change, line) in lines {
                std::mem::discriminant(change).hash(hasher);
                line.hash(hasher);
            }
        }
        Kind::ListItem {
            title,
            subtitle,
            leading,
            trailing,
            ..
        }
        | Kind::Header {
            title,
            subtitle,
            leading,
            trailing,
            ..
        } => {
            (title, subtitle).hash(hasher);
            all(
                &mut leading.iter().chain(trailing).map(|node| &**node),
                hasher,
            );
        }
        Kind::Section { title, trailing } => {
            title.hash(hasher);
            all(&mut trailing.iter().map(|node| &**node), hasher);
        }
        Kind::Detail { name, value } => {
            name.hash(hasher);
            fingerprint(value, hasher);
        }
        Kind::Alternate { options, .. } => all(&mut options.iter(), hasher),
        Kind::Row(children)
        | Kind::Column(children)
        | Kind::List(children)
        | Kind::Timeline(children)
        | Kind::Grid {
            cells: children, ..
        } => all(&mut children.iter(), hasher),
        Kind::Switcher { options, .. } => all(&mut options.iter().map(|(node, _)| node), hasher),
        Kind::Tabs(pages) => pages.iter().for_each(|(name, page)| {
            name.hash(hasher);
            fingerprint(page, hasher);
        }),
        Kind::Button { content, .. }
        | Kind::Gulp(content)
        | Kind::Center(content)
        | Kind::Display(content)
        | Kind::Mark(content)
        | Kind::Scroll { content, .. } => fingerprint(content, hasher),
        Kind::Step {
            mark,
            leading,
            content,
        } => {
            std::mem::discriminant(mark).hash(hasher);
            all(&mut leading.iter().map(|node| &**node), hasher);
            fingerprint(content, hasher);
        }
        Kind::TextInput { .. }
        | Kind::TextArea { .. }
        | Kind::Icon { .. }
        | Kind::Image { .. }
        | Kind::Sprite { .. }
        | Kind::Separator
        | Kind::Empty
        | Kind::Fill
        | Kind::Slider { .. }
        | Kind::Toggle { .. }
        | Kind::Meter { .. } => {}
    }
}

/// Every app and icon file `node` shows an icon for, so the host can load the ones it doesn't
/// have.
pub fn wanted<M: 'static>(node: Node<M>, out: &mut Wanted) {
    let mut all =
        |children: Vec<Node<M>>| children.into_iter().for_each(|child| wanted(child, out));
    match node.into_kind() {
        Kind::AppIcon { app_id, .. } => out.apps.push(app_id),
        Kind::Image { file, .. } => out.files.push(file),
        Kind::Sprite { frames, .. } => out.frames.extend(frames),
        Kind::Cover { url, fallback, .. } => {
            out.covers.push(url);
            wanted(*fallback, out);
        }
        Kind::Alternate { options, .. } => all(options),
        Kind::Row(children) | Kind::Column(children) | Kind::List(children) => all(children),
        Kind::Grid { cells, .. } => all(cells),
        Kind::Button { content, .. }
        | Kind::Gulp(content)
        | Kind::Center(content)
        | Kind::Display(content)
        | Kind::Mark(content)
        | Kind::Counted { content, .. }
        | Kind::Detail { value: content, .. } => all(vec![*content]),
        Kind::Switcher { options, .. } => all(options.into_iter().map(|(node, _)| node).collect()),
        Kind::Tabs(pages) => all(pages.into_iter().map(|(_, page)| page).collect()),
        Kind::ListItem {
            leading, trailing, ..
        } => all(leading
            .into_iter()
            .chain(trailing)
            .map(|node| *node)
            .collect()),
        Kind::Header {
            leading, trailing, ..
        } => all(leading
            .into_iter()
            .chain(trailing)
            .map(|node| *node)
            .collect()),
        Kind::Section { trailing, .. }
        | Kind::TextInput { trailing, .. }
        | Kind::TextArea { trailing, .. } => all(trailing.into_iter().map(|node| *node).collect()),
        Kind::Scroll { content, .. } => all(vec![*content]),
        Kind::Timeline(steps) => all(steps),
        Kind::Step {
            leading, content, ..
        } => all(leading
            .into_iter()
            .map(|node| *node)
            .chain([*content])
            .collect()),
        Kind::Markdown { .. }
        | Kind::Literal { .. }
        | Kind::Divider { .. }
        | Kind::Diff { .. }
        | Kind::Label { .. }
        | Kind::Heading { .. }
        | Kind::Icon { .. }
        | Kind::Counter { .. }
        | Kind::Separator
        | Kind::Empty
        | Kind::Fill
        | Kind::Slider { .. }
        | Kind::Toggle { .. }
        | Kind::Meter { .. }
        | Kind::Badge { .. }
        | Kind::Keys { .. } => {}
        // The host fills bays before it looks for icons, so what parts show is found too.
        Kind::Bay { .. } => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Bar,
    Panel,
    /// A panel in the middle of the screen, taller.
    Dialog,
}

/// How text is set: as a label, a little heavier, or large.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Text {
    Body,
    Strong,
    Display,
}

/// What a node inherits from the nodes around it.
#[derive(Debug, Clone, Copy)]
struct Ctx {
    tone: Tone,
    /// Overrides the tone's color, for text on a filled shape.
    color: Option<Color>,
    text: Text,
    place: Place,
    /// Inside a `center`: text keeps to one line and is cut if it's too long.
    centered: bool,
    /// The color of what the popover shows (its cover's), for its sliders and main button.
    content: Option<Color>,
    /// How tall a scroll gets.
    scroll: f32,
    palette: &'static Palette,
}

impl Ctx {
    fn new(place: Place, palette: &'static Palette) -> Self {
        Self {
            tone: Tone::Normal,
            color: None,
            text: Text::Body,
            place,
            centered: false,
            content: None,
            scroll: theme::SCROLL_MAX_HEIGHT,
            palette,
        }
    }

    /// What a slider's fill and a prominent button are filled with: the content's color, or
    /// the ink's.
    fn fill(&self) -> Color {
        self.content.unwrap_or(self.palette.accent_fill)
    }

    fn color(&self, tone: Option<Tone>) -> Color {
        self.color
            .unwrap_or_else(|| self.palette.tone(tone.unwrap_or(self.tone)))
    }

    fn size(&self) -> f32 {
        match self.text {
            Text::Body | Text::Strong => theme::TEXT_SIZE,
            Text::Display => theme::DISPLAY_SIZE,
        }
    }

    fn font(&self) -> Font {
        match self.text {
            Text::Body => Font::DEFAULT,
            Text::Strong => theme::STRONG_FONT,
            Text::Display => theme::HEADING_FONT,
        }
    }

    fn panel(&self) -> bool {
        matches!(self.place, Place::Panel | Place::Dialog)
    }
}

fn render<'a>(node: Node<AnyMessage>, ctx: Ctx, icons: &Icons) -> Element<'a, AnyMessage> {
    let p = ctx.palette;
    let all = |children: Vec<Node<AnyMessage>>| {
        children
            .into_iter()
            .map(move |child| render(child, ctx, icons))
    };

    match node.into_kind() {
        Kind::Label {
            text: content,
            tone,
        } if ctx.panel() && ctx.centered => Clip::new(
            text(content)
                .size(ctx.size())
                .font(ctx.font())
                .color(ctx.color(tone))
                .wrapping(text::Wrapping::None),
            p.island_bg,
        )
        .into(),
        Kind::Label {
            text: content,
            tone,
        } => Label::new(content, ctx.size(), ctx.font(), ctx.color(tone)).into(),
        // One line in a popover, like the header's title: a long one is cut, not wrapped.
        Kind::Heading {
            text: content,
            tone,
        } if ctx.panel() => Clip::new(
            text(content)
                .size(theme::HEADING_SIZE)
                .font(theme::HEADING_FONT)
                .color(ctx.color(tone))
                .wrapping(text::Wrapping::None),
            p.island_bg,
        )
        .into(),
        Kind::Heading {
            text: content,
            tone,
        } => text(content)
            .size(theme::HEADING_SIZE)
            .font(theme::HEADING_FONT)
            .color(ctx.color(tone))
            .into(),
        Kind::Icon { icon, tone } => icon::view(icon, ctx.color(tone)),
        Kind::AppIcon { app_id, tone } => app_icon(&app_id, tone.unwrap_or(ctx.tone), icons, p),
        Kind::Image { file, tint, tone } => match icons.files.get(&FileKey::of(file)) {
            Some(Some(icon)) if tint => icon.symbol(ctx.color(tone)),
            Some(Some(icon)) => icon.view(dim_opacity(tone.unwrap_or(ctx.tone))),
            // Its room, so nothing moves when it arrives.
            _ => Space::new()
                .width(theme::ICON_SIZE)
                .height(theme::ICON_SIZE)
                .into(),
        },
        Kind::Sprite {
            frames,
            order,
            looping,
            tone,
        } => {
            let height = if ctx.panel() {
                theme::SPRITE_HEIGHT_PANEL
            } else {
                theme::SPRITE_HEIGHT
            };
            let drawn: Option<Vec<_>> = frames
                .iter()
                .map(|file| icons.frames.get(&FileKey::of(file)).cloned().flatten())
                .collect();
            match drawn {
                Some(drawn) if !drawn.is_empty() => {
                    let size = drawn[0].size;
                    Sprite {
                        key: frames.as_ptr() as usize ^ (order.as_ptr() as usize).rotate_left(32),
                        frames: drawn.into_iter().map(|frame| frame.handle).collect(),
                        order,
                        looping,
                        opacity: dim_opacity(tone.unwrap_or(ctx.tone)),
                        size: Size::new(height * size.width / size.height.max(1.0), height),
                    }
                    .into()
                }
                // Its height, until every frame is drawn.
                _ => Space::new().width(height).height(height).into(),
            }
        }
        Kind::Cover {
            url,
            fallback,
            large,
            tone,
        } => match icons.covers.get(&url) {
            Some(Some(cover)) => cover.view(large, dim_opacity(tone.unwrap_or(ctx.tone))),
            _ => render(*fallback, ctx, icons),
        },
        Kind::Counter {
            value,
            range,
            unit,
            tone,
        } => {
            let counter = Counter::new(value, range, unit, ctx.color(tone));
            match ctx.text {
                Text::Body => counter.into(),
                _ => counter.text(ctx.size(), ctx.font()).into(),
            }
        }
        Kind::Alternate {
            options,
            current,
            hovered,
            grows,
        } => Alternate::new(all(options).collect(), current, hovered, grows).into(),
        Kind::Row(children) => row(all(children))
            .spacing(theme::SPACING)
            .align_y(alignment::Vertical::Center)
            .into(),
        Kind::Column(children) => column(all(children)).spacing(theme::SPACING).into(),
        Kind::Grid { columns, cells } => grid(columns, all(cells), ctx),
        Kind::Gulp(content) => Gulp::new(render(*content, ctx, icons)).into(),
        Kind::Center(content) if ctx.panel() => {
            let inner = Ctx {
                centered: true,
                ..ctx
            };
            container(render(*content, inner, icons))
                .center_x(Length::Fill)
                .into()
        }
        Kind::Center(content) => render(*content, ctx, icons),
        Kind::Button {
            content,
            on_press,
            on_secondary,
            prominent,
            shortcut,
        } => {
            let only_icon = matches!(content.kind(), Kind::Icon { .. } | Kind::Image { .. });
            let button = if !ctx.panel() {
                button(render(*content, ctx, icons))
                    .on_press(on_press)
                    .padding([2, 8])
                    .style(bar_button_style)
            } else if only_icon && prominent {
                let inner = Ctx {
                    color: Some(p.on_accent),
                    ..ctx
                };
                let fill = ctx.fill();
                button(
                    container(render(*content, inner, icons))
                        .center(Length::Fixed(theme::PROMINENT_BUTTON)),
                )
                .on_press(on_press)
                .padding(0)
                .style(move |theme: &Theme, status| prominent_button_style(theme, status, fill))
            } else if only_icon {
                button(
                    container(render(*content, ctx, icons))
                        .center(Length::Fixed(theme::ICON_BUTTON)),
                )
                .on_press(on_press)
                .padding(0)
                .style(icon_button_style)
            } else {
                let inner = Ctx {
                    text: Text::Strong,
                    color: prominent.then_some(p.on_accent),
                    ..ctx
                };
                let fill = prominent.then(|| ctx.fill());
                let content = render(*content, inner, icons);
                // Its key, if it has one, on the button after what it says.
                let content: Element<'_, AnyMessage> =
                    match shortcut.and_then(|binding| binding.key) {
                        Some(key) => row![content, keycaps(&key, inner.color(None), p)]
                            .spacing(theme::SPACING)
                            .align_y(alignment::Vertical::Center)
                            .into(),
                        None => content,
                    };
                button(container(content).center_y(Length::Fill))
                    .on_press(on_press)
                    .height(theme::BUTTON_HEIGHT)
                    .padding([0, 14])
                    .style(move |theme: &Theme, status| text_button_style(theme, status, fill))
            };
            match on_secondary {
                Some(message) => mouse_area(button).on_right_press(message).into(),
                None => button.into(),
            }
        }
        Kind::Switcher { options, current } => {
            let options = options.into_iter().map(|(content, on_press)| {
                button(render(content, ctx, icons))
                    .on_press(on_press)
                    .padding([2, 8])
                    .style(bar_button_style)
                    .into()
            });
            Switcher::new(row(options).spacing(theme::SPACING / 2), current).into()
        }
        Kind::Separator => separator(p),
        Kind::Divider { text: caption } => {
            let caption = text(caption)
                .size(theme::SMALL_SIZE)
                .color(p.tone(Tone::Dim));
            row![separator(p), caption, separator(p)]
                .spacing(theme::SPACING + 2)
                .align_y(alignment::Vertical::Center)
                .into()
        }
        // One line, as typed: a long one fades out at the end.
        Kind::Literal {
            text: content,
            tone,
        } => Clip::new(
            text(content)
                .font(theme::MONO_FONT)
                .size(theme::SMALL_SIZE)
                .color(ctx.color(tone))
                .wrapping(text::Wrapping::None),
            p.island_bg,
        )
        .into(),
        Kind::Timeline(steps) => timeline::view(
            steps
                .into_iter()
                .map(|node| {
                    let mut hasher = std::hash::DefaultHasher::new();
                    fingerprint(&node, &mut hasher);
                    let key = hasher.finish();
                    if !matches!(node.kind(), Kind::Step { .. }) {
                        return timeline::Step {
                            key,
                            mark: Mark::Note,
                            leading: None,
                            content: render(node, ctx, icons),
                        };
                    }
                    let Kind::Step {
                        mark,
                        leading,
                        content,
                    } = node.into_kind()
                    else {
                        unreachable!()
                    };
                    timeline::Step {
                        key,
                        mark,
                        leading: leading.map(|node| render(*node, ctx, icons)),
                        content: render(*content, ctx, icons),
                    }
                })
                .collect(),
        ),
        Kind::Step { content, .. } => render(*content, ctx, icons),
        Kind::Diff { title, lines } => code::diff(Some(title), lines, "", p),
        Kind::Empty => text("").into(),
        // The host fills every bay before rendering (`bays::fill`). One left here has no parts.
        Kind::Bay { .. } => text("").into(),
        Kind::Fill if ctx.panel() => Space::new().width(Length::Fill).into(),
        Kind::Fill => Space::new().into(),
        Kind::ListItem {
            title,
            subtitle,
            leading,
            trailing,
            selected,
            on_press,
            on_secondary,
            shortcut,
        } => list_item(
            ListItem {
                title,
                subtitle,
                leading,
                trailing,
                selected,
                on_press,
                on_secondary,
                shortcut: shortcut.and_then(|binding| binding.key),
            },
            ctx,
            icons,
        ),
        Kind::List(rows) => {
            let rows = column(all(rows)).spacing(theme::ROW_SPACING);
            if !ctx.panel() {
                return rows.into();
            }
            let most = theme::LIST_MAX_ROWS * (theme::ROW_HEIGHT + theme::ROW_SPACING as f32);
            container(Glide::new(
                scrollable::Scrollable::new(rows)
                    .direction(scrollable::Direction::Vertical(
                        Scrollbar::new().width(3).scroller_width(3).margin(1),
                    ))
                    .width(Length::Fill)
                    .style(scrollable_style),
            ))
            .max_height(most)
            .into()
        }
        Kind::Section { title, trailing } => {
            let caption = text(title.to_uppercase())
                .size(theme::SMALL_SIZE)
                .font(theme::STRONG_FONT)
                .color(p.tone(Tone::Dim));
            let trailing = trailing.map(|node| render(*node, ctx, icons));
            row![caption, fill(ctx)]
                .push(trailing)
                .spacing(theme::SPACING)
                .align_y(alignment::Vertical::Center)
                .height(theme::ICON_BUTTON)
                .into()
        }
        Kind::Header {
            title,
            subtitle,
            on_back,
            leading,
            trailing,
        } => {
            let back = on_back.map(|message| {
                button(
                    container(icon::view(eave_api::ui::Icon::Back, ctx.color(None)))
                        .center(Length::Fixed(theme::ICON_BUTTON)),
                )
                .on_press(message)
                .padding(0)
                .style(icon_button_style)
            });
            let title = Clip::new(
                text(title)
                    .size(theme::HEADING_SIZE)
                    .font(theme::HEADING_FONT)
                    .color(ctx.color(None))
                    .wrapping(text::Wrapping::None),
                p.island_bg,
            );
            // A subtitle sits under the title, one line too, cut if it's long.
            let title: Element<'a, AnyMessage> = match subtitle {
                Some(subtitle) => column![
                    title,
                    Clip::new(
                        text(subtitle)
                            .size(theme::SMALL_SIZE)
                            .color(p.tone(Tone::Dim))
                            .wrapping(text::Wrapping::None),
                        p.island_bg,
                    )
                ]
                .width(Length::Fill)
                .into(),
                None => title.into(),
            };
            let leading = leading.map(|node| render(*node, ctx, icons));
            let trailing = trailing.map(|node| render(*node, ctx, icons));
            row![]
                .push(back)
                .push(leading)
                .push(title)
                .push(trailing)
                .spacing(6)
                .align_y(alignment::Vertical::Center)
                .height(Length::Shrink)
                .into()
        }
        Kind::Detail { name, value } => {
            let name = text(name).size(theme::TEXT_SIZE).color(p.tone(Tone::Dim));
            let value =
                container(row![render(*value, ctx, icons)].align_y(alignment::Vertical::Center))
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Right);
            row![name, value]
                .spacing(12)
                .align_y(alignment::Vertical::Center)
                .height(26)
                .into()
        }
        Kind::Slider {
            value,
            range,
            step,
            on_change,
            on_release,
        } => Slider::new(value, range, step, on_change, on_release, ctx.fill())
            .width(if ctx.panel() {
                Length::Fill
            } else {
                Length::Fixed(80.0)
            })
            .into(),
        Kind::Toggle { on, on_toggle } => Toggle::new(on, on_toggle).into(),
        Kind::Meter {
            fraction,
            limit,
            tone,
        } => Meter::new(fraction, limit, ctx.color(tone)).into(),
        Kind::TextInput {
            placeholder,
            value,
            on_input,
            on_submit,
            secret,
            trailing,
            tone,
        } => {
            let input = text_input(&placeholder, &value)
                .on_input(on_input)
                .on_submit_maybe(on_submit)
                .secure(secret)
                .size(theme::TEXT_SIZE)
                .padding([0, 10])
                .width(Length::Fill)
                .style(field_style);
            let trailing = trailing.map(|node| render(*node, ctx, icons));
            let content = row![input]
                .push(trailing)
                .spacing(4)
                .padding(Padding::ZERO.right(3))
                .align_y(alignment::Vertical::Center);
            let alert = matches!(tone, Some(Tone::Critical | Tone::Warning))
                .then(|| p.tone(tone.unwrap_or_default()));
            Field::new(content, alert).into()
        }
        Kind::Display(content) => render(
            *content,
            Ctx {
                text: Text::Display,
                ..ctx
            },
            icons,
        ),
        Kind::Mark(content) => {
            let inner = Ctx {
                tone: Tone::Accent,
                text: Text::Strong,
                ..ctx
            };
            container(render(*content, inner, icons))
                .center(Length::Fixed(theme::MARK_SIZE))
                .style(move |_| container::Style {
                    border: Border {
                        color: p.tone(Tone::Accent),
                        width: theme::MARK_RING,
                        radius: (theme::MARK_SIZE / 2.0).into(),
                    },
                    ..container::Style::default()
                })
                .into()
        }
        Kind::Counted { content, count } => Pip::new(render(*content, ctx, icons), count).into(),
        Kind::Tabs(pages) if pages.is_empty() => text("").into(),
        Kind::Tabs(pages) if ctx.panel() => {
            let (names, pages): (Vec<_>, Vec<_>) = pages
                .into_iter()
                .map(|(name, page)| (name, render(page, ctx, icons)))
                .unzip();
            Tabs::new(names, pages).into()
        }
        Kind::Tabs(pages) => {
            let (_, first) = pages.into_iter().next().expect("not empty");
            render(first, ctx, icons)
        }
        Kind::Markdown { text: source, tone } => markdown::view(&source, ctx.color(tone), p),
        Kind::TextArea {
            placeholder,
            value,
            on_input,
            on_submit,
            trailing,
        } => {
            let area = TextArea::new(placeholder, value, on_input, on_submit);
            let trailing = trailing.map(|node| render(*node, ctx, icons));
            let content = row![area]
                .push(trailing)
                .spacing(4)
                .padding(Padding::ZERO.right(3))
                .align_y(alignment::Vertical::Bottom);
            Field::new(content, None).grows().into()
        }
        Kind::Scroll { content, bottom } => {
            let content = render(*content, ctx, icons);
            if !ctx.panel() {
                return content;
            }
            // Room on the right for the scrollbar, so it never covers the text.
            let content = container(content).padding(Padding::ZERO.right(8));
            let scroll = scrollable::Scrollable::new(content)
                .direction(scrollable::Direction::Vertical(
                    Scrollbar::new().width(3).scroller_width(3).margin(1),
                ))
                .width(Length::Fill)
                .style(scrollable_style);
            let scroll = if bottom {
                scroll.anchor_bottom()
            } else {
                scroll
            };
            container(Glide::new(scroll).bottom(bottom))
                .max_height(ctx.scroll)
                .into()
        }
        Kind::Badge {
            text: content,
            tone,
        } => {
            let color = ctx.color(tone);
            let edge = match tone.unwrap_or(ctx.tone) {
                Tone::Normal | Tone::Dim => p.badge_border,
                _ => color,
            };
            container(
                text(content)
                    .size(theme::SMALL_SIZE - 1.0)
                    .font(theme::STRONG_FONT)
                    .color(color),
            )
            .padding([1, 4])
            .style(move |_| container::Style {
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: theme::BADGE_RADIUS.into(),
                },
                ..container::Style::default()
            })
            .into()
        }
        Kind::Keys { shortcut, tone } => keycaps(&shortcut, ctx.color(tone), p),
    }
}

/// A shortcut as keycaps, `[Ctrl] [K]`: each key on a faint filled cap, in the text's tone.
fn keycaps<'a>(shortcut: &Shortcut, color: Color, p: &'static Palette) -> Element<'a, AnyMessage> {
    let caps = shortcut::labels(shortcut).into_iter().map(|label| {
        container(
            text(label)
                .size(theme::SMALL_SIZE - 1.0)
                .font(theme::STRONG_FONT)
                .color(color),
        )
        .padding(theme::KEYCAP_PADDING)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.keycap_bg())),
            border: Border::default().rounded(theme::BADGE_RADIUS),
            ..container::Style::default()
        })
        .into()
    });
    row(caps)
        .spacing(theme::KEYCAP_SPACING)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// The free space in a popover row; nothing in the bar.
fn fill<'a>(ctx: Ctx) -> Element<'a, AnyMessage> {
    if ctx.panel() {
        Space::new().width(Length::Fill).into()
    } else {
        Space::new().into()
    }
}

/// Cells in rows of `columns`. In the bar every cell is `GRID_CELL` wide; in a popover the cells
/// share the panel's width, and rows are tall enough for a mark.
fn grid<'a>(
    columns: usize,
    cells: impl Iterator<Item = Element<'a, AnyMessage>>,
    ctx: Ctx,
) -> Element<'a, AnyMessage> {
    let mut cells = cells.map(|cell| {
        let cell = container(cell).align_x(alignment::Horizontal::Center);
        if ctx.panel() {
            cell.width(Length::FillPortion(1))
                .height(theme::GRID_ROW)
                .align_y(alignment::Vertical::Center)
                .into()
        } else {
            cell.width(Length::Fixed(theme::GRID_CELL)).into()
        }
    });
    let mut rows = Vec::new();
    loop {
        let line: Vec<Element<'a, AnyMessage>> = cells.by_ref().take(columns).collect();
        if line.is_empty() {
            break;
        }
        rows.push(row(line).into());
    }
    column(rows).spacing(theme::SPACING / 4).into()
}

struct ListItem {
    title: String,
    subtitle: Option<String>,
    leading: Option<Box<Node<AnyMessage>>>,
    trailing: Option<Box<Node<AnyMessage>>>,
    selected: bool,
    on_press: Option<AnyMessage>,
    on_secondary: Option<AnyMessage>,
    shortcut: Option<Shortcut>,
}

/// A row: leading icon, title over subtitle, then trailing at the far end, then its shortcut's
/// keys. The selected row is
/// lit: a soft fill of ink behind, a heavier title, and its icons in the accent.
fn list_item<'a>(item: ListItem, ctx: Ctx, icons: &Icons) -> Element<'a, AnyMessage> {
    let p = ctx.palette;
    let background = if item.selected {
        // The selected fill over the island, which is what a fading title fades into.
        over(p.selected_bg, p.island_bg)
    } else {
        p.island_bg
    };
    let leading_ctx = Ctx {
        tone: if item.selected {
            Tone::Accent
        } else {
            ctx.tone
        },
        ..ctx
    };
    let title = text(item.title)
        .size(theme::TEXT_SIZE)
        .font(if item.selected {
            theme::STRONG_FONT
        } else {
            Font::DEFAULT
        })
        .color(ctx.color(None))
        .wrapping(text::Wrapping::None);
    let tall = item.subtitle.is_some();
    let subtitle = item.subtitle.map(|subtitle| {
        Clip::new(
            text(subtitle)
                .size(theme::SMALL_SIZE)
                .color(p.tone(Tone::Dim))
                .wrapping(text::Wrapping::None),
            background,
        )
    });
    let lines = column![Clip::new(title, background)]
        .push(subtitle)
        .spacing(1)
        .width(Length::Fill);
    // Icons in a column of their own width; a sprite takes its own.
    let leading = item.leading.map(|node| {
        let sprite = matches!(node.kind(), Kind::Sprite { .. });
        let leading = render(*node, leading_ctx, icons);
        if sprite {
            leading
        } else {
            container(leading).center(theme::ICON_SIZE).into()
        }
    });
    let trailing_ctx = Ctx {
        tone: if item.selected {
            Tone::Accent
        } else {
            Tone::Dim
        },
        ..ctx
    };
    let trailing = item.trailing.map(|node| render(*node, trailing_ctx, icons));
    let keys = item
        .shortcut
        .map(|shortcut| keycaps(&shortcut, trailing_ctx.color(None), p));
    let content = row![]
        .push(leading)
        .push(lines)
        .push(trailing)
        .push(keys)
        .spacing(10)
        .align_y(alignment::Vertical::Center);
    let height = if tall {
        theme::ROW_HEIGHT_TALL
    } else {
        theme::ROW_HEIGHT
    };
    let selected = item.selected;
    let padding = Padding {
        top: 0.0,
        right: 4.0,
        bottom: 0.0,
        left: 8.0,
    };
    let row: Element<'a, AnyMessage> = match item.on_press {
        Some(message) => button(container(content).center_y(Length::Fill))
            .on_press(message)
            .width(Length::Fill)
            .height(height)
            .padding(padding)
            .style(move |theme: &Theme, status| row_style(theme, status, selected))
            .into(),
        None => container(content)
            .center_y(Length::Fixed(height))
            .width(Length::Fill)
            .padding(padding)
            .style(move |_| container::Style {
                background: selected.then_some(Background::Color(p.selected_bg)),
                border: Border::default().rounded(theme::BUTTON_RADIUS),
                ..container::Style::default()
            })
            .into(),
    };
    match item.on_secondary {
        Some(message) => mouse_area(row).on_right_press(message).into(),
        None => row,
    }
}

/// `color` laid over `under`, as one opaque color.
fn over(color: Color, under: Color) -> Color {
    let mix = |top: f32, bottom: f32| top * color.a + bottom * (1.0 - color.a);
    Color::from_rgb(
        mix(color.r, under.r),
        mix(color.g, under.g),
        mix(color.b, under.b),
    )
}

/// How opaque a picture in its own colors is: a tone only dims it.
fn dim_opacity(tone: Tone) -> f32 {
    if tone == Tone::Dim {
        theme::DIM_APP_ICON
    } else {
        1.0
    }
}

/// The app's icon at the size of a status icon, dimmed for `Tone::Dim`. Until it has loaded, or
/// if it has none, the app id's first letter in a box of the same size, so nothing moves when it
/// arrives.
fn app_icon<'a>(
    app_id: &str,
    tone: Tone,
    icons: &Icons,
    p: &'static Palette,
) -> Element<'a, AnyMessage> {
    match icons.apps.get(app_id) {
        Some(Some(icon)) => icon.view(dim_opacity(tone)),
        _ => {
            let letter: String = app_id
                .chars()
                .take(1)
                .flat_map(char::to_uppercase)
                .collect();
            container(Label::new(
                letter,
                theme::TEXT_SIZE,
                Font::DEFAULT,
                p.tone(tone),
            ))
            .width(theme::ICON_SIZE)
            .height(theme::ICON_SIZE)
            .center(theme::ICON_SIZE)
            .into()
        }
    }
}

fn state_background(p: &Palette, status: button::Status) -> Option<Background> {
    match status {
        button::Status::Hovered => Some(Background::Color(p.hover_bg)),
        button::Status::Pressed => Some(Background::Color(p.pressed_bg)),
        button::Status::Active | button::Status::Disabled => None,
    }
}

/// Buttons in the bar: plain until hovered.
fn bar_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let p = Palette::of(theme);
    button::Style {
        background: state_background(p, status),
        text_color: p.tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// Icon buttons in a popover: plain until hovered, like the bar's.
fn icon_button_style(theme: &Theme, status: button::Status) -> button::Style {
    bar_button_style(theme, status)
}

/// A button that's only its text, dim until hovered: a card's "Show all".
fn quiet_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let p = Palette::of(theme);
    let tone = match status {
        button::Status::Hovered | button::Status::Pressed => Tone::Normal,
        button::Status::Active | button::Status::Disabled => Tone::Dim,
    };
    button::Style {
        background: None,
        text_color: p.tone(tone),
        ..button::Style::default()
    }
}

/// How a filled button answers the pointer: a little dimmer hovered, more pressed.
fn filled(fill: Color, status: button::Status) -> Color {
    match status {
        button::Status::Hovered => fill.scale_alpha(0.88),
        button::Status::Pressed => fill.scale_alpha(0.76),
        _ => fill,
    }
}

/// Text buttons in a popover: filled even at rest, so they read as buttons. A prominent one is
/// filled with `prominent`: the ink's color, or the content's.
fn text_button_style(
    theme: &Theme,
    status: button::Status,
    prominent: Option<Color>,
) -> button::Style {
    let p = Palette::of(theme);
    let background = match prominent {
        Some(fill) => filled(fill, status),
        None => match status {
            button::Status::Hovered => p.pressed_bg,
            button::Status::Pressed => p.hover_bg,
            _ => p.button_bg,
        },
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: p.tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// List rows: the selected one tinted, the others plain until hovered.
fn row_style(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let p = Palette::of(theme);
    let background = match (selected, status) {
        (true, button::Status::Pressed) => Some(Background::Color(p.selected_bg.scale_alpha(1.4))),
        (true, _) => Some(Background::Color(p.selected_bg)),
        (false, status) => state_background(p, status),
    };
    button::Style {
        background,
        text_color: p.tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// A prominent icon button (play and pause): a disc filled with `fill`.
fn prominent_button_style(theme: &Theme, status: button::Status, fill: Color) -> button::Style {
    let p = Palette::of(theme);
    button::Style {
        background: Some(Background::Color(filled(fill, status))),
        text_color: p.on_accent,
        border: Border::default().rounded(theme::PROMINENT_BUTTON / 2.0),
        ..button::Style::default()
    }
}

/// The text input inside a [`Field`], which draws the box around it.
fn field_style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let p = Palette::of(theme);
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: p.tone(Tone::Dim),
        placeholder: p.placeholder,
        value: p.tone(Tone::Normal),
        selection: p.selection,
    }
}

fn scrollable_style(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = Palette::of(theme);
    let scroller = match status {
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        }
        | scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged: true,
            ..
        } => p.scroller.scale_alpha(1.6),
        _ => p.scroller,
    };
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(scroller),
            border: Border::default().rounded(1.5),
        },
    };
    let default = scrollable::default(theme, status);
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        ..default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sprite_wants_every_frame() {
        static A: &[u8] = b"<svg/>";
        static B: &[u8] = b"<svg></svg>";
        static FRAMES: [&[u8]; 2] = [A, B];
        let node: Node<()> = eave_api::ui::row([
            eave_api::ui::sprite(&FRAMES, &[0, 1, 1]),
            eave_api::ui::image(A),
        ]);
        let mut out = Wanted::default();
        wanted(node, &mut out);
        assert_eq!(out.frames, [A, B]);
        assert_eq!(out.files, [A]);
    }
}
