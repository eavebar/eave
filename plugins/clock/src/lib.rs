//! A clock that shows the date while hovered, with a month calendar in its popover.

use chrono::format::{Item as FormatItem, StrftimeItems};
use chrono::{DateTime, Datelike, Days, Local, Months, NaiveDate};
use eave_api::time::{Duration, sleep};
use eave_api::ui::{self, Icon, Item, Node, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use futures_util::{Stream, stream};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Clock>("clock");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// `strftime` format for the bar.
    format: Format,
    /// `strftime` format shown while the clock is hovered.
    hover_format: Format,
    /// Shows ISO week numbers down the calendar's left side.
    week_numbers: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            format: Format("%H:%M".into()),
            hover_format: Format("%b %-d".into()),
            week_numbers: true,
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
    hover_format: Format,
    week_numbers: bool,
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
            hover_format: config.hover_format,
            week_numbers: config.week_numbers,
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
        let formats = (self.format.0.clone(), self.hover_format.0.clone());
        Subscription::run_with(formats, |formats| changes(formats.clone())).map(|_| Message::Tick)
    }

    fn item(&self) -> Item<Message> {
        let format = |format: &Format| ui::label(self.now.format(&format.0).to_string());
        Item::new(ui::hover(format(&self.format), format(&self.hover_format)))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let today = self.now.date_naive();
        let away = self.month != first_of_month(today);
        let mut title = ui::header(self.now.format("%A %-d %B").to_string());
        if away {
            title = title.trailing(ui::button(ui::label("Today"), Message::ThisMonth));
        }
        let week = today.iso_week();
        // A month is a page: the next one comes in from the right, the one before from the left.
        let order = i64::from(self.month.year()) * 12 + i64::from(self.month.month0());
        let months = ui::row([
            ui::button(ui::icon(Icon::Back), Message::PreviousMonth),
            ui::fill(),
            ui::page(order, ui::label(self.month.format("%B %Y").to_string())),
            ui::fill(),
            ui::button(ui::icon(Icon::Forward), Message::NextMonth),
        ]);
        let panel = ui::column([
            ui::column([
                title,
                ui::label(format!("Week {} · {}", week.week(), week.year())).tone(Tone::Dim),
            ]),
            months,
            ui::page(order, self.calendar()),
        ]);
        // It always opens on this month.
        Some(Panel::new(panel).on_open(Message::ThisMonth))
    }
}

/// Yields whenever the time reads differently in either format. There's no event source for
/// "the minute changed", so this is one of the few allowed timers: it looks at the start of every
/// second, but only a change becomes a message, since each message rebuilds the whole bar.
fn changes((format, hover): (String, String)) -> impl Stream<Item = ()> {
    stream::unfold(None, move |last: Option<(String, String)>| {
        let (format, hover) = (format.clone(), hover.clone());
        async move {
            loop {
                let into = Local::now().timestamp_subsec_nanos();
                let wait = 1_000_000_000u32.saturating_sub(into).max(1_000_000);
                sleep(Duration::from_nanos(wait.into())).await;
                let now = Local::now();
                let shown = (
                    now.format(&format).to_string(),
                    now.format(&hover).to_string(),
                );
                if last.as_ref() != Some(&shown) {
                    return Some(((), Some(shown)));
                }
            }
        }
    })
}

impl Clock {
    /// Six weeks from the Monday on or before the first of the month, so the calendar never
    /// changes height, with days of the months around it dimmed.
    fn calendar(&self) -> Node<Message> {
        let today = self.now.date_naive();
        let weekdays =
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|day| ui::label(day).tone(Tone::Dim));
        let corner = self.week_numbers.then(ui::empty);
        let mut cells: Vec<Node<Message>> = corner.into_iter().chain(weekdays).collect();
        for week in weeks(self.month).chunks(7) {
            if self.week_numbers {
                cells.push(ui::label(week[0].iso_week().week().to_string()).tone(Tone::Dim));
            }
            cells.extend(week.iter().map(|&day| {
                let label = ui::label(day.day().to_string());
                if day == today {
                    ui::mark(label)
                } else if day.month() != self.month.month() {
                    label.tone(Tone::Dim)
                } else {
                    label
                }
            }));
        }
        ui::grid(if self.week_numbers { 8 } else { 7 }, cells)
    }
}

/// The 42 days a calendar of `month` shows: the six weeks from the Monday on or before its
/// first day.
fn weeks(month: NaiveDate) -> Vec<NaiveDate> {
    let back = month.weekday().num_days_from_monday();
    let start = month - Days::new(back.into());
    start.iter_days().take(42).collect()
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
        assert_eq!(config.hover_format.0, "%b %-d");
        assert!(config.week_numbers);
    }

    #[test]
    fn a_calendar_is_six_weeks_from_a_monday() {
        let october = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let days = weeks(october);
        assert_eq!(days.len(), 42);
        assert_eq!(days[0], NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());
        assert_eq!(days[0].iso_week().week(), 40);
        assert_eq!(days[41], NaiveDate::from_ymd_opt(2026, 11, 8).unwrap());

        // A month starting on a Monday starts on its first.
        let june = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(weeks(june)[0], june);
    }
}
