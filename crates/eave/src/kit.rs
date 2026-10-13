//! Turns the plugin vocabulary ([`eave_api::ui`]) into iced elements, styled by [`theme`].
//! This is the only place plugin UI becomes pixels.
//!
//! The same node can land in the bar or in a popover. A popover is laid out at a known width
//! ([D-40](../../../docs/decisions.md)), so there rows stretch, lists scroll and `fill` pushes
//! things to the end. The bar has no width to give, so those collapse to their content.

use eave_api::erased::AnyMessage;
use eave_api::ui::{Kind, Node, NoticeParts, Tone, Urgency};
use iced::widget::scrollable::{self, Scrollbar};
use iced::widget::{
    Space, button, column, container, mouse_area, row, rule, slider, text, text_input,
};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme, alignment};

use crate::alternate::Alternate;
use crate::apps::Icons;
use crate::clip::Clip;
use crate::counter::Counter;
use crate::field::Field;
use crate::gulp::Gulp;
use crate::icon;
use crate::label::Label;
use crate::meter::Meter;
use crate::switcher::Switcher;
use crate::tabs::Tabs;
use crate::theme;
use crate::toggle::Toggle;

/// A node in the bar.
pub fn node<'a>(node: Node<AnyMessage>, icons: &Icons) -> Element<'a, AnyMessage> {
    render(node, Ctx::new(Place::Bar), icons)
}

/// A popover's content, laid out at the panel's width.
pub fn panel<'a>(node: Node<AnyMessage>, icons: &Icons) -> Element<'a, AnyMessage> {
    render(node, Ctx::new(Place::Panel), icons)
}

/// Notifications' cards, newest first, as one popover's content, with a line between each and
/// the next.
pub fn notices<'a, M: 'a>(cards: impl IntoIterator<Item = Element<'a, M>>) -> Element<'a, M> {
    let mut column = column![].spacing(theme::NOTICE_SPACING);
    for (index, card) in cards.into_iter().enumerate() {
        if index > 0 {
            column = column.push(separator());
        }
        column = column.push(card);
    }
    column.into()
}

/// A thin horizontal line between groups.
fn separator<'a, M: 'a>() -> Element<'a, M> {
    rule::horizontal(1)
        .style(|_| rule::Style {
            color: theme::SEPARATOR,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// The line height of a notification's body, relative to its size: iced's default.
const LINE_HEIGHT: f32 = 1.3;

/// One notification, laid out at the panel's width: where it's from and a close button, the
/// summary, the body cut off after a few lines, then its actions. A click anywhere else on it
/// sends its `on_press`.
pub fn notice<'a>(notice: NoticeParts<AnyMessage>, icons: &Icons) -> Element<'a, AnyMessage> {
    let tone = match notice.urgency {
        Urgency::Critical => Tone::Critical,
        Urgency::Normal | Urgency::Low => Tone::Dim,
    };
    let ctx = Ctx::new(Place::Panel);
    let from = notice.app_icon.as_deref().or(notice.app.as_deref());
    let app = from
        .map(|app_id| container(app_icon(app_id, Tone::Normal, icons)).center(theme::ICON_SIZE));
    let name = notice.app.map(|name| {
        Clip::new(
            text(name)
                .size(theme::SMALL_SIZE)
                .font(theme::STRONG_FONT)
                .color(theme::tone(tone))
                .wrapping(text::Wrapping::None),
            theme::ISLAND_BG,
        )
    });
    let close = notice.on_dismiss.map(|message| {
        button(
            container(icon::view(
                eave_api::ui::Icon::Close,
                theme::tone(Tone::Dim),
            ))
            .center(Length::Fixed(theme::NOTICE_CLOSE)),
        )
        .on_press(message)
        .padding(0)
        .style(icon_button_style)
    });
    let top = row![]
        .push(app)
        .push(name)
        .push(fill(ctx))
        .push(close)
        .spacing(theme::SPACING)
        .align_y(alignment::Vertical::Center);

    let summary_tone = if notice.urgency == Urgency::Low {
        Tone::Dim
    } else {
        Tone::Normal
    };
    let summary = text(notice.summary)
        .size(theme::TEXT_SIZE)
        .font(theme::STRONG_FONT)
        .color(theme::tone(summary_tone))
        .wrapping(text::Wrapping::WordOrGlyph);
    let body = notice.body.filter(|body| !body.is_empty()).map(|body| {
        let line = (theme::TEXT_SIZE * LINE_HEIGHT).ceil();
        container(
            text(body)
                .size(theme::TEXT_SIZE)
                .line_height(LINE_HEIGHT)
                .color(theme::tone(Tone::Dim))
                .wrapping(text::Wrapping::WordOrGlyph),
        )
        .max_height(line * theme::NOTICE_BODY_LINES)
        .clip(true)
    });
    let actions = (!notice.actions.is_empty()).then(|| {
        let buttons = notice
            .actions
            .into_iter()
            .enumerate()
            .map(|(index, (label, message))| {
                let button = eave_api::ui::button(eave_api::ui::label(label), message);
                // The first is the main action.
                let button = if index == 0 {
                    button.prominent()
                } else {
                    button
                };
                render(button, ctx, icons)
            });
        row![fill(ctx)].extend(buttons).spacing(theme::SPACING)
    });
    let card = column![top, summary]
        .push(body)
        .push(actions)
        .spacing(4)
        .width(Length::Fill);
    match notice.on_press {
        Some(message) => mouse_area(card)
            .on_press(message)
            .interaction(iced::mouse::Interaction::Pointer)
            .into(),
        None => card.into(),
    }
}

/// Every app id `node` shows an icon for, so the host can load the ones it doesn't have.
pub fn app_ids<M: 'static>(node: Node<M>, out: &mut Vec<String>) {
    let mut all =
        |children: Vec<Node<M>>| children.into_iter().for_each(|child| app_ids(child, out));
    match node.into_kind() {
        Kind::AppIcon { app_id, .. } => out.push(app_id),
        Kind::Alternate { options, .. } => all(options),
        Kind::Row(children) | Kind::Column(children) | Kind::List(children) => all(children),
        Kind::Grid { cells, .. } => all(cells),
        Kind::Button { content, .. }
        | Kind::Gulp(content)
        | Kind::Display(content)
        | Kind::Mark(content)
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
        Kind::Section { trailing, .. }
        | Kind::Header { trailing, .. }
        | Kind::TextInput { trailing, .. } => all(trailing.into_iter().map(|node| *node).collect()),
        Kind::Label { .. }
        | Kind::Heading { .. }
        | Kind::Icon { .. }
        | Kind::Counter { .. }
        | Kind::Separator
        | Kind::Empty
        | Kind::Fill
        | Kind::Slider { .. }
        | Kind::Toggle { .. }
        | Kind::Meter { .. }
        | Kind::Badge { .. } => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Bar,
    Panel,
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
}

impl Ctx {
    fn new(place: Place) -> Self {
        Self {
            tone: Tone::Normal,
            color: None,
            text: Text::Body,
            place,
        }
    }

    fn color(&self, tone: Option<Tone>) -> Color {
        self.color
            .unwrap_or_else(|| theme::tone(tone.unwrap_or(self.tone)))
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
        self.place == Place::Panel
    }
}

fn render<'a>(node: Node<AnyMessage>, ctx: Ctx, icons: &Icons) -> Element<'a, AnyMessage> {
    let all = |children: Vec<Node<AnyMessage>>| {
        children
            .into_iter()
            .map(move |child| render(child, ctx, icons))
    };

    match node.into_kind() {
        Kind::Label {
            text: content,
            tone,
        } => Label::new(content, ctx.size(), ctx.font(), ctx.color(tone)).into(),
        Kind::Heading {
            text: content,
            tone,
        } => text(content)
            .size(theme::HEADING_SIZE)
            .font(theme::HEADING_FONT)
            .color(ctx.color(tone))
            .into(),
        Kind::Icon { icon, tone } => icon::view(icon, ctx.color(tone)),
        Kind::AppIcon { app_id, tone } => app_icon(&app_id, tone.unwrap_or(ctx.tone), icons),
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
        Kind::Button {
            content,
            on_press,
            on_secondary,
            prominent,
        } => {
            let only_icon = matches!(content.kind(), Kind::Icon { .. });
            let button = if !ctx.panel() {
                button(render(*content, ctx, icons))
                    .on_press(on_press)
                    .padding([2, 8])
                    .style(bar_button_style)
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
                    color: prominent.then_some(theme::ON_ACCENT),
                    ..ctx
                };
                button(container(render(*content, inner, icons)).center_y(Length::Fill))
                    .on_press(on_press)
                    .height(theme::BUTTON_HEIGHT)
                    .padding([0, 14])
                    .style(move |theme: &Theme, status| text_button_style(theme, status, prominent))
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
        Kind::Separator => separator(),
        Kind::Empty => text("").into(),
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
        } => list_item(
            ListItem {
                title,
                subtitle,
                leading,
                trailing,
                selected,
                on_press,
                on_secondary,
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
            container(
                scrollable::Scrollable::new(rows)
                    .direction(scrollable::Direction::Vertical(
                        Scrollbar::new().width(3).scroller_width(3).margin(1),
                    ))
                    .width(Length::Fill)
                    .style(scrollable_style),
            )
            .max_height(most)
            .into()
        }
        Kind::Section { title, trailing } => {
            let caption = text(title.to_uppercase())
                .size(theme::SMALL_SIZE)
                .font(theme::STRONG_FONT)
                .color(theme::tone(Tone::Dim));
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
            on_back,
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
                theme::ISLAND_BG,
            );
            let trailing = trailing.map(|node| render(*node, ctx, icons));
            row![]
                .push(back)
                .push(title)
                .push(trailing)
                .spacing(6)
                .align_y(alignment::Vertical::Center)
                .height(Length::Shrink)
                .into()
        }
        Kind::Detail { name, value } => {
            let name = text(name)
                .size(theme::TEXT_SIZE)
                .color(theme::tone(Tone::Dim));
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
        } => {
            let slider = slider(range, value, on_change)
                .step(step)
                .width(if ctx.panel() {
                    Length::Fill
                } else {
                    Length::Fixed(80.0)
                })
                .height(16)
                .style(slider_style);
            match on_release {
                Some(message) => slider.on_release(message).into(),
                None => slider.into(),
            }
        }
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
                .then(|| theme::tone(tone.unwrap_or_default()));
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
                color: Some(theme::ON_ACCENT),
                text: Text::Strong,
                ..ctx
            };
            container(render(*content, inner, icons))
                .center(Length::Fixed(theme::MARK_SIZE))
                .style(|_| container::Style {
                    background: Some(Background::Color(theme::tone(Tone::Accent))),
                    border: Border::default().rounded(theme::MARK_SIZE / 2.0),
                    ..container::Style::default()
                })
                .into()
        }
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
        Kind::Badge {
            text: content,
            tone,
        } => {
            let color = ctx.color(tone);
            let edge = match tone.unwrap_or(ctx.tone) {
                Tone::Normal | Tone::Dim => theme::BADGE_BORDER,
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
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            })
            .into()
        }
    }
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
}

/// A row: leading icon, title over subtitle, then trailing at the far end. Selected rows take the
/// accent: behind, in the icon, and in a heavier title.
fn list_item<'a>(item: ListItem, ctx: Ctx, icons: &Icons) -> Element<'a, AnyMessage> {
    let background = if item.selected {
        // The selected tint over the island, which is what a fading title fades into.
        let tint = theme::SELECTED_BG;
        Color::from_rgb(tint.r * tint.a, tint.g * tint.a, tint.b * tint.a)
    } else {
        theme::ISLAND_BG
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
                .color(if item.selected {
                    theme::SELECTED_SUBTITLE
                } else {
                    theme::tone(Tone::Dim)
                })
                .wrapping(text::Wrapping::None),
            background,
        )
    });
    let lines = column![Clip::new(title, background)]
        .push(subtitle)
        .spacing(1)
        .width(Length::Fill);
    let leading = item
        .leading
        .map(|node| container(render(*node, leading_ctx, icons)).center(theme::ICON_SIZE));
    let trailing_ctx = Ctx {
        tone: if item.selected {
            Tone::Accent
        } else {
            Tone::Dim
        },
        ..ctx
    };
    let trailing = item.trailing.map(|node| render(*node, trailing_ctx, icons));
    let content = row![]
        .push(leading)
        .push(lines)
        .push(trailing)
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
                background: selected.then_some(Background::Color(theme::SELECTED_BG)),
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

/// The app's icon at the size of a status icon, dimmed for `Tone::Dim`. Until it has loaded, or
/// if it has none, the app id's first letter in a box of the same size, so nothing moves when it
/// arrives.
fn app_icon<'a>(app_id: &str, tone: Tone, icons: &Icons) -> Element<'a, AnyMessage> {
    let opacity = if tone == Tone::Dim {
        theme::DIM_APP_ICON
    } else {
        1.0
    };
    match icons.get(app_id) {
        Some(Some(icon)) => icon.view(opacity),
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
                theme::tone(tone),
            ))
            .width(theme::ICON_SIZE)
            .height(theme::ICON_SIZE)
            .center(theme::ICON_SIZE)
            .into()
        }
    }
}

fn state_background(status: button::Status) -> Option<Background> {
    match status {
        button::Status::Hovered => Some(Background::Color(theme::HOVER_BG)),
        button::Status::Pressed => Some(Background::Color(theme::PRESSED_BG)),
        button::Status::Active | button::Status::Disabled => None,
    }
}

/// Buttons in the bar: plain until hovered.
fn bar_button_style(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: state_background(status),
        text_color: theme::tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// Icon buttons in a popover: plain until hovered, like the bar's.
fn icon_button_style(theme: &Theme, status: button::Status) -> button::Style {
    bar_button_style(theme, status)
}

/// Text buttons in a popover: filled even at rest, so they read as buttons. A prominent one is
/// filled with the accent.
fn text_button_style(_: &Theme, status: button::Status, prominent: bool) -> button::Style {
    let background = if prominent {
        let accent = theme::tone(Tone::Accent);
        match status {
            button::Status::Hovered => accent.scale_alpha(0.88),
            button::Status::Pressed => accent.scale_alpha(0.76),
            _ => accent,
        }
    } else {
        match status {
            button::Status::Hovered => theme::PRESSED_BG,
            button::Status::Pressed => theme::HOVER_BG,
            _ => theme::BUTTON_BG,
        }
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: theme::tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// List rows: the selected one tinted, the others plain until hovered.
fn row_style(_: &Theme, status: button::Status, selected: bool) -> button::Style {
    let background = match (selected, status) {
        (true, button::Status::Pressed) => {
            Some(Background::Color(theme::SELECTED_BG.scale_alpha(1.4)))
        }
        (true, _) => Some(Background::Color(theme::SELECTED_BG)),
        (false, status) => state_background(status),
    };
    button::Style {
        background,
        text_color: theme::tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}

fn slider_style(_: &Theme, status: slider::Status) -> slider::Style {
    let knob = match status {
        slider::Status::Active => theme::tone(Tone::Normal),
        slider::Status::Hovered | slider::Status::Dragged => Color::WHITE,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(theme::tone(Tone::Accent)),
                Background::Color(theme::TRACK),
            ),
            width: theme::SLIDER_RAIL,
            border: Border::default().rounded(theme::SLIDER_RAIL / 2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: theme::SLIDER_KNOB,
            },
            background: Background::Color(knob),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

/// The text input inside a [`Field`], which draws the box around it.
fn field_style(_: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: theme::tone(Tone::Dim),
        placeholder: theme::PLACEHOLDER,
        value: theme::tone(Tone::Normal),
        selection: theme::SELECTION,
    }
}

fn scrollable_style(_: &Theme, status: scrollable::Status) -> scrollable::Style {
    let scroller = match status {
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        }
        | scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged: true,
            ..
        } => theme::SCROLLER.scale_alpha(1.6),
        _ => theme::SCROLLER,
    };
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(scroller),
            border: Border::default().rounded(1.5),
        },
    };
    let default = scrollable::default(&Theme::Dark, status);
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        ..default
    }
}
