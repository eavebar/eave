//! A clock, with a month calendar in its popover.

use chrono::format::{Item as FormatItem, StrftimeItems};
use chrono::{DateTime, Datelike, Local, Months, NaiveDate};
use eave_api::time::{Duration, every};
use eave_api::ui::{self, Item, Node, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Clock>("clock");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// `strftime` format for the bar.
    format: Format,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            format: Format("%H:%M".into()),
        }
    }
}

/// A format string checked at load time, so formatting can never fail later.
#[derive(Deserialize)]
#[serde(try_from = "String")]
struct Format(String);

impl TryFrom<String> for Format {
    type Error = String;

    fn try_from(format: String) -> Result<Self, String> {
        if StrftimeItems::new(&format).any(|item| item == FormatItem::Error) {
            return Err(format!("invalid clock format {format:?}"));
        }
        Ok(Self(format))
    }
}

pub struct Clock {
    format: Format,
    now: DateTime<Local>,
    /// First day of the month the calendar shows.
    month: NaiveDate,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    PreviousMonth,
    NextMonth,
    ThisMonth,
}

impl Widget for Clock {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let now = Local::now();
        let clock = Self {
            format: config.format,
            now,
            month: first_of_month(now.date_naive()),
        };
        (clock, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Tick => self.now = Local::now(),
            Message::PreviousMonth => self.month = self.month - Months::new(1),
            Message::NextMonth => self.month = self.month + Months::new(1),
            Message::ThisMonth => self.month = first_of_month(self.now.date_naive()),
        }
        Effect::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        // No event source for "the minute changed", so this is one of the few allowed timers.
        every(Duration::from_secs(1)).map(|_| Message::Tick)
    }

    fn item(&self) -> Item<Message> {
        Item::new(ui::label(self.now.format(&self.format.0).to_string()))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let header = ui::row([
            ui::button(ui::label("‹"), Message::PreviousMonth),
            ui::button(
                ui::label(self.month.format("%B %Y").to_string()),
                Message::ThisMonth,
            ),
            ui::button(ui::label("›"), Message::NextMonth),
        ]);

        Some(Panel::new(ui::column([
            ui::heading(self.now.format("%A %-d %B").to_string()),
            ui::separator(),
            header,
            self.calendar(),
        ])))
    }
}

impl Clock {
    fn calendar(&self) -> Node<Message> {
        let today = self.now.date_naive();
        let weekdays =
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|day| ui::label(day).tone(Tone::Dim));
        let leading = self.month.weekday().num_days_from_monday() as usize;
        let days = self
            .month
            .iter_days()
            .take_while(|day| day.month() == self.month.month())
            .map(|day| {
                let label = ui::label(day.day().to_string());
                if day == today {
                    label.tone(Tone::Accent)
                } else {
                    label
                }
            });

        ui::grid(
            7,
            weekdays
                .into_iter()
                .chain((0..leading).map(|_| ui::empty()))
                .chain(days),
        )
    }
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("every month has a first day")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_formats() {
        assert!(Format::try_from("%H:%M".to_string()).is_ok());
        assert!(Format::try_from("%Q".to_string()).is_err());
    }

    #[test]
    fn config_defaults_when_empty() {
        let config: Config = toml::Table::new().try_into().unwrap();
        assert_eq!(config.format.0, "%H:%M");
    }
}
