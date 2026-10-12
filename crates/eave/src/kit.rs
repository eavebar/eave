//! Turns the plugin vocabulary ([`eave_api::ui`]) into iced elements, styled by [`theme`].
//! This is the only place plugin UI becomes pixels.

use eave_api::erased::AnyMessage;
use eave_api::ui::{Kind, Node, Tone};
use iced::widget::{button, column, container, image, mouse_area, row, rule, text};
use iced::{Background, Border, Element, Length, Theme, alignment};

use crate::alternate::Alternate;
use crate::apps::{AppIcon, Icons};
use crate::counter::Counter;
use crate::expand::Expand;
use crate::icon;
use crate::switcher::Switcher;
use crate::theme;

/// `icons` holds the app icons loaded so far; an app icon not in it shows its first letter.
pub fn node<'a>(node: Node<AnyMessage>, icons: &Icons) -> Element<'a, AnyMessage> {
    render(node, Tone::Normal, icons)
}

/// Every app id `node` shows an icon for, so the host can load the ones it doesn't have.
pub fn app_ids<M>(node: Node<M>, out: &mut Vec<String>) {
    let mut all =
        |children: Vec<Node<M>>| children.into_iter().for_each(|child| app_ids(child, out));
    match node.into_kind() {
        Kind::AppIcon { app_id, .. } => out.push(app_id),
        Kind::Alternate { options, .. } => all(options),
        Kind::Row(children) | Kind::Column(children) => all(children),
        Kind::Grid { cells, .. } => all(cells),
        Kind::Expand {
            collapsed,
            expanded,
        } => all(vec![*collapsed, *expanded]),
        Kind::Button { content, .. } => all(vec![*content]),
        Kind::Switcher { options, .. } => all(options.into_iter().map(|(node, _)| node).collect()),
        Kind::Label { .. }
        | Kind::Heading { .. }
        | Kind::Icon { .. }
        | Kind::Counter { .. }
        | Kind::Separator
        | Kind::Empty => {}
    }
}

fn render<'a>(node: Node<AnyMessage>, inherited: Tone, icons: &Icons) -> Element<'a, AnyMessage> {
    let all = |children: Vec<Node<AnyMessage>>| {
        children
            .into_iter()
            .map(move |child| render(child, inherited, icons))
    };

    match node.into_kind() {
        Kind::Label {
            text: content,
            tone,
        } => text(content)
            .size(theme::TEXT_SIZE)
            .color(theme::tone(tone.unwrap_or(inherited)))
            .into(),
        Kind::Heading {
            text: content,
            tone,
        } => text(content)
            .size(theme::HEADING_SIZE)
            .font(theme::HEADING_FONT)
            .color(theme::tone(tone.unwrap_or(inherited)))
            .into(),
        Kind::Icon { icon, tone } => icon::view(icon, theme::tone(tone.unwrap_or(inherited))),
        Kind::AppIcon { app_id, tone } => app_icon(&app_id, tone.unwrap_or(inherited), icons),
        Kind::Counter {
            value,
            range,
            unit,
            tone,
        } => Counter::new(value, range, unit, theme::tone(tone.unwrap_or(inherited))).into(),
        Kind::Alternate {
            options,
            current,
            hovered,
        } => Alternate::new(all(options).collect(), current, hovered).into(),
        Kind::Expand {
            collapsed,
            expanded,
        } => Expand::new(
            render(*collapsed, inherited, icons),
            render(*expanded, inherited, icons),
        )
        .into(),
        Kind::Row(children) => row(all(children))
            .spacing(theme::SPACING)
            .align_y(alignment::Vertical::Center)
            .into(),
        Kind::Column(children) => column(all(children)).spacing(theme::SPACING).into(),
        Kind::Grid { columns, cells } => {
            let mut cells = all(cells).map(|cell| {
                container(cell)
                    .width(Length::Fixed(theme::GRID_CELL))
                    .align_x(alignment::Horizontal::Center)
                    .into()
            });
            let mut rows = Vec::new();
            loop {
                let line: Vec<Element<'a, AnyMessage>> = cells.by_ref().take(columns).collect();
                if line.is_empty() {
                    break;
                }
                rows.push(row(line).into());
            }
            column(rows).spacing(theme::SPACING / 2).into()
        }
        Kind::Button {
            content,
            on_press,
            on_secondary,
        } => {
            let button = button(render(*content, inherited, icons))
                .on_press(on_press)
                .padding([2, 8])
                .style(button_style);
            match on_secondary {
                Some(message) => mouse_area(button).on_right_press(message).into(),
                None => button.into(),
            }
        }
        Kind::Switcher { options, current } => {
            let options = options.into_iter().map(|(content, on_press)| {
                button(render(content, inherited, icons))
                    .on_press(on_press)
                    .padding([2, 8])
                    .style(button_style)
                    .into()
            });
            Switcher::new(row(options).spacing(theme::SPACING / 2), current).into()
        }
        Kind::Separator => rule::horizontal(1)
            .style(|_| rule::Style {
                color: theme::SEPARATOR,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            })
            .into(),
        Kind::Empty => text("").into(),
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
        Some(Some(AppIcon(handle))) => image(handle.clone())
            .width(theme::ICON_SIZE)
            .height(theme::ICON_SIZE)
            .opacity(opacity)
            .into(),
        _ => {
            let letter: String = app_id
                .chars()
                .take(1)
                .flat_map(char::to_uppercase)
                .collect();
            container(text(letter).size(theme::TEXT_SIZE).color(theme::tone(tone)))
                .width(theme::ICON_SIZE)
                .height(theme::ICON_SIZE)
                .center(theme::ICON_SIZE)
                .into()
        }
    }
}

fn button_style(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(Background::Color(theme::HOVER_BG)),
        button::Status::Pressed => Some(Background::Color(theme::PRESSED_BG)),
        button::Status::Active | button::Status::Disabled => None,
    };
    button::Style {
        background,
        text_color: theme::tone(Tone::Normal),
        border: Border::default().rounded(theme::BUTTON_RADIUS),
        ..button::Style::default()
    }
}
