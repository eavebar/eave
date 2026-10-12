//! Turns the plugin vocabulary ([`eave_api::ui`]) into iced elements, styled by [`theme`].
//! This is the only place plugin UI becomes pixels.

use eave_api::erased::AnyMessage;
use eave_api::ui::{Kind, Node, Tone};
use iced::widget::{button, column, container, row, rule, text};
use iced::{Background, Border, Element, Length, Theme, alignment};

use crate::alternate::Alternate;
use crate::counter::Counter;
use crate::gulp::Gulp;
use crate::icon;
use crate::switcher::Switcher;
use crate::theme;

pub fn node<'a>(node: Node<AnyMessage>) -> Element<'a, AnyMessage> {
    render(node, Tone::Normal)
}

fn render<'a>(node: Node<AnyMessage>, inherited: Tone) -> Element<'a, AnyMessage> {
    let all = |children: Vec<Node<AnyMessage>>| {
        children
            .into_iter()
            .map(move |child| render(child, inherited))
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
        Kind::Gulp(content) => Gulp::new(render(*content, inherited)).into(),
        Kind::Button { content, on_press } => button(render(*content, inherited))
            .on_press(on_press)
            .padding([2, 8])
            .style(button_style)
            .into(),
        Kind::Switcher { options, current } => {
            let options = options.into_iter().map(|(content, on_press)| {
                button(render(content, inherited))
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
