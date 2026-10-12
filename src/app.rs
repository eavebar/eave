use std::time::Duration;

use chrono::{DateTime, Local};
use iced::alignment::Horizontal;
use iced::widget::{container, row, text};
use iced::{Color, Element, Length, Subscription, Task, Theme, theme};
use iced_layershell::to_layer_message;

const BG: Color = Color::from_rgba(0.07, 0.07, 0.09, 0.85);
const FG: Color = Color::from_rgb(0.90, 0.90, 0.92);

pub struct App {
    now: DateTime<Local>,
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    Tick(DateTime<Local>),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        (Self { now: Local::now() }, Task::none())
    }
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Tick(now) => app.now = now,
        _ => {}
    }
    Task::none()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let section = |content, align| {
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(align)
            .center_y(Length::Fill)
    };

    row![
        section(text("workspaces"), Horizontal::Left),
        section(text(app.now.format("%H:%M").to_string()), Horizontal::Center),
        section(text(app.now.format("%a %d %b").to_string()), Horizontal::Right),
    ]
    .padding([0, 12])
    .into()
}

pub fn subscription(_: &App) -> Subscription<Message> {
    iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick(Local::now()))
}

pub fn style(_: &App, _: &Theme) -> theme::Style {
    theme::Style {
        background_color: BG,
        text_color: FG,
    }
}
