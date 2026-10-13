//! Battery level, as a battery icon filled to the charge: green while charging, red when nearly
//! empty. Hovering it shows the percentage instead, gulping while it charges. Read from the kernel's
//! `/sys/class/power_supply`, with every system battery combined into one. Hidden when there's no
//! battery (a desktop). The popover has the state, the time left and the power draw.
//!
//! It sends a notification when the battery runs low and when it's done charging ([`notify`]).

use eave_api::ui::{self, Icon, Item, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use serde::Deserialize;

mod notify;
mod sysfs;

pub fn register(registry: &mut Registry) {
    registry.widget::<Battery>("battery");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Percentage at or below which a discharging battery shows as a warning.
    warning: u8,
    /// Percentage at or below which it shows as critical, and a notification says so.
    critical: u8,
    /// Whether to send notifications: battery low, and charged.
    notify: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            warning: 20,
            critical: 10,
            notify: true,
        }
    }
}

/// Every battery combined, read as one snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    present: bool,
    percentage: f64,
    state: State,
    /// Seconds, 0 when unknown.
    time_to_empty: i64,
    time_to_full: i64,
    /// Watts.
    energy_rate: f64,
    /// Watt-hours stored now, and when full.
    energy: Option<f64>,
    energy_full: Option<f64>,
    /// Full capacity over what it was designed for.
    health: Option<f64>,
    cycles: Option<u32>,
    /// °C.
    temperature: Option<f64>,
    /// The kernel's word for the battery's health, like `Good`.
    condition: Option<String>,
    /// The charge it stops at, in percent, when that's under 100.
    charge_limit: Option<u8>,
    /// Whether a charger is plugged in, when the machine says.
    ac_online: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Unknown,
    Charging,
    Discharging,
    Full,
    /// Plugged in, but not charging (charge limit reached, or waiting).
    NotCharging,
}

impl State {
    /// The kernel's `status` attribute.
    fn from_sysfs(status: &str) -> Self {
        match status {
            "Charging" => Self::Charging,
            "Discharging" => Self::Discharging,
            "Full" => Self::Full,
            "Not charging" => Self::NotCharging,
            _ => Self::Unknown,
        }
    }
}

pub struct Battery {
    config: Config,
    reading: Reading,
    /// Warned that the battery is low, since the charger was last plugged in.
    warned: bool,
    /// The low battery notification's id, once the server gave it.
    warning: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Reading),
    /// The low battery notification went up, with this id.
    Warned(u32),
}

/// A notification to send or take away.
#[derive(Debug, Clone, PartialEq)]
enum Alert {
    /// Running low, at this percent.
    Low(u8),
    /// The charger is in: the low battery notification can go.
    Plugged,
    /// Done charging, at this percent.
    Charged(u8),
}

impl Widget for Battery {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let battery = Self {
            config,
            reading: Reading::default(),
            warned: false,
            warning: None,
        };
        (battery, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Read(reading) => {
                let before = std::mem::replace(&mut self.reading, reading);
                match self.alert(&before) {
                    Some(alert) => Effect::task(self.send(alert)),
                    None => Effect::none(),
                }
            }
            // Plugged in before the server answered: it can go right away.
            Message::Warned(id) if !self.warned => Effect::task(close(id)),
            Message::Warned(id) => {
                self.warning = Some(id);
                Effect::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(sysfs::watch).map(Message::Read)
    }

    fn item(&self) -> Item<Message> {
        if !self.reading.present {
            return Item::hidden();
        }
        let percent = self.percent();
        let icon = ui::icon(Icon::Battery {
            charge: f32::from(percent) / 100.0,
            charging: self.charging(),
        });
        let label = ui::label(format!("{percent}%"));
        let label = if self.charging() {
            ui::gulp(label)
        } else {
            label
        };
        Item::new(ui::hover(icon, label).tone(self.tone()))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        if !self.reading.present {
            return None;
        }
        let reading = &self.reading;
        let percent = self.percent();
        let limit = reading.charge_limit.map(|limit| format!("{limit}%"));
        let (when, what) = match reading.state {
            State::Charging if reading.time_to_full > 0 => {
                (duration(reading.time_to_full), "until full".to_string())
            }
            State::Charging => ("Charging".into(), String::new()),
            State::Discharging if reading.time_to_empty > 0 => {
                (duration(reading.time_to_empty), "until empty".to_string())
            }
            State::Discharging => ("On battery".into(), String::new()),
            State::Full => ("Fully charged".into(), String::new()),
            State::NotCharging => (
                "Not charging".into(),
                match &limit {
                    Some(limit) => format!("held at the {limit} limit"),
                    None => "plugged in".into(),
                },
            ),
            State::Unknown => ("Unknown state".into(), String::new()),
        };
        let mut side = vec![ui::label(when)];
        if !what.is_empty() {
            side.push(ui::label(what).tone(Tone::Dim));
        }
        let hero = ui::row([
            ui::display(ui::counter(percent.into(), 0..=100, "%")),
            ui::fill(),
            ui::column(side),
        ])
        .tone(self.tone());
        let mut meter = ui::meter(f32::from(percent) / 100.0).tone(self.tone());
        if let Some(limit) = reading.charge_limit {
            meter = meter.limit(f32::from(limit) / 100.0);
        }

        let mut power = Vec::new();
        let plugged = reading.ac_online.unwrap_or(matches!(
            reading.state,
            State::Charging | State::Full | State::NotCharging
        ));
        let source = if plugged { "Charger" } else { "Battery" };
        power.push(ui::detail("Source", ui::label(source)));
        if reading.energy_rate > 0.05 {
            let direction = match reading.state {
                State::Charging => "Charging at",
                _ => "Drawing",
            };
            let watts = ui::label(format!("{:.1} W", reading.energy_rate));
            power.push(ui::detail(direction, watts));
        }
        if let (Some(energy), Some(full)) = (reading.energy, reading.energy_full) {
            power.push(ui::detail(
                "Energy",
                of(format!("{energy:.1}"), format!("of {full:.1} Wh")),
            ));
        }
        if let Some(limit) = limit {
            power.push(ui::detail("Charge limit", ui::label(limit)));
        }

        let mut health = Vec::new();
        if let Some(fraction) = reading.health {
            let capacity = format!("{:.0}%", fraction * 100.0);
            health.push(ui::detail("Capacity", of(capacity, "of design".into())));
        }
        if let Some(cycles) = reading.cycles {
            health.push(ui::detail("Cycles", ui::label(cycles.to_string())));
        }
        if let Some(temperature) = reading.temperature {
            health.push(ui::detail(
                "Temperature",
                ui::label(format!("{temperature:.0} °C")),
            ));
        }
        if let Some(condition) = &reading.condition {
            let tone = if condition == "Good" {
                Tone::Good
            } else {
                Tone::Warning
            };
            health.push(ui::detail(
                "Condition",
                ui::label(condition.clone()).tone(tone),
            ));
        }
        let details = if health.is_empty() {
            ui::column(power)
        } else {
            ui::tabs([("Power", ui::column(power)), ("Health", ui::column(health))])
        };
        Some(Panel::new(ui::column([hero, meter, details])))
    }
}

/// A value and, dimmed after it, what it's out of: `36.5 of 57.8 Wh`.
fn of(value: String, whole: String) -> ui::Node<Message> {
    ui::row([ui::label(value), ui::label(whole).tone(Tone::Dim)])
}

impl Battery {
    /// What to tell the user now that the reading went from `before` to the current one.
    fn alert(&mut self, before: &Reading) -> Option<Alert> {
        let reading = &self.reading;
        if !self.config.notify || !reading.present {
            return None;
        }
        let percent = self.percent();
        match reading.state {
            // Once per discharge, also when the bar starts on a low battery.
            State::Discharging if percent <= self.config.critical && !self.warned => {
                self.warned = true;
                Some(Alert::Low(percent))
            }
            State::Charging | State::Full | State::NotCharging if self.warned => {
                self.warned = false;
                Some(Alert::Plugged)
            }
            // Only right after charging: plugging in a full battery isn't news.
            _ if before.state == State::Charging && self.charged() => Some(Alert::Charged(percent)),
            _ => None,
        }
    }

    /// Full, or held at its charge limit.
    fn charged(&self) -> bool {
        match self.reading.state {
            State::Full => true,
            State::NotCharging => self
                .reading
                .charge_limit
                .is_some_and(|limit| self.percent() >= limit),
            _ => false,
        }
    }

    fn send(&mut self, alert: Alert) -> Task<Message> {
        let note = match alert {
            Alert::Plugged => return self.warning.take().map_or_else(Task::none, close),
            Alert::Low(percent) => {
                let left = match self.reading.time_to_empty {
                    0 => String::new(),
                    seconds => format!(", about {}", duration(seconds)),
                };
                notify::Note {
                    icon: "battery-caution",
                    summary: "Battery low".into(),
                    body: format!("{percent}% left{left}. Plug in the charger."),
                    critical: true,
                }
            }
            Alert::Charged(percent) => notify::Note {
                icon: "battery-full-charged",
                summary: "Battery charged".into(),
                body: match self.reading.charge_limit {
                    Some(limit) if limit < 100 => format!("Charged to the {limit}% limit."),
                    _ => format!("{percent}%. You can unplug the charger."),
                },
                critical: false,
            },
        };
        let low = matches!(alert, Alert::Low(_));
        Task::future(async move {
            match notify::send(note).await {
                Ok(id) => low.then_some(Message::Warned(id)),
                Err(error) => {
                    eprintln!("eave-battery: notification not sent: {error}");
                    None
                }
            }
        })
        .and_then(Task::done)
    }

    fn charging(&self) -> bool {
        self.reading.state == State::Charging
    }

    fn percent(&self) -> u8 {
        self.reading.percentage.round().clamp(0.0, 100.0) as u8
    }

    fn tone(&self) -> Tone {
        let percent = self.percent();
        match self.reading.state {
            State::Charging => Tone::Good,
            State::Full | State::NotCharging => Tone::Normal,
            _ if percent <= self.config.critical => Tone::Critical,
            _ if percent <= self.config.warning => Tone::Warning,
            _ => Tone::Normal,
        }
    }
}

fn close(id: u32) -> Task<Message> {
    Task::future(async move {
        if let Err(error) = notify::close(id).await {
            eprintln!("eave-battery: notification not closed: {error}");
        }
    })
    .discard()
}

/// `2 h 05 min`, or `14 min` under an hour.
fn duration(seconds: i64) -> String {
    let minutes = (seconds + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, minutes) => format!("{hours} h {minutes:02} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percentage: f64, state: State) -> Battery {
        let (mut battery, _) = Battery::new(Config::default());
        battery.update(Message::Read(Reading {
            present: true,
            percentage,
            state,
            ..Reading::default()
        }));
        battery
    }

    #[test]
    fn warns_only_while_discharging() {
        assert_eq!(battery(50.0, State::Discharging).tone(), Tone::Normal);
        assert_eq!(battery(20.6, State::Discharging).tone(), Tone::Normal);
        assert_eq!(battery(19.6, State::Discharging).tone(), Tone::Warning);
        assert_eq!(battery(8.0, State::Discharging).tone(), Tone::Critical);
        assert_eq!(battery(8.0, State::Charging).tone(), Tone::Good);
        assert_eq!(battery(8.0, State::NotCharging).tone(), Tone::Normal);
    }

    #[test]
    fn shows_the_percentage_on_hover() {
        let content = battery(42.0, State::Discharging)
            .item()
            .into_parts()
            .content;
        let ui::Kind::Alternate {
            mut options,
            current: 0,
            hovered: Some(1),
            grows: false,
        } = content.unwrap().into_kind()
        else {
            panic!("the icon, with the percentage on hover");
        };
        assert!(matches!(
            options.pop().unwrap().into_kind(),
            ui::Kind::Label { text, .. } if text == "42%"
        ));
    }

    #[test]
    fn the_percentage_gulps_while_charging() {
        let content = battery(42.0, State::Charging).item().into_parts().content;
        let ui::Kind::Alternate { mut options, .. } = content.unwrap().into_kind() else {
            panic!("the icon, with the percentage on hover");
        };
        let ui::Kind::Gulp(label) = options.pop().unwrap().into_kind() else {
            panic!("a gulping percentage");
        };
        assert!(matches!(label.into_kind(), ui::Kind::Label { text, .. } if text == "42%"));
    }

    #[test]
    fn hidden_without_a_battery() {
        let (battery, _) = Battery::new(Config::default());
        assert!(battery.item().into_parts().content.is_none());
        assert!(battery.popover().is_none());
    }

    fn read(battery: &mut Battery, percentage: f64, state: State) -> Option<Alert> {
        let before = std::mem::replace(
            &mut battery.reading,
            Reading {
                present: true,
                percentage,
                state,
                ..Reading::default()
            },
        );
        battery.alert(&before)
    }

    #[test]
    fn warns_once_when_low() {
        let mut widget = battery(12.0, State::Discharging);
        assert_eq!(read(&mut widget, 11.0, State::Discharging), None);
        assert_eq!(
            read(&mut widget, 10.0, State::Discharging),
            Some(Alert::Low(10))
        );
        assert_eq!(read(&mut widget, 9.0, State::Discharging), None);
        assert_eq!(read(&mut widget, 9.0, State::Unknown), None);
        assert_eq!(
            read(&mut widget, 9.0, State::Charging),
            Some(Alert::Plugged)
        );
        assert_eq!(read(&mut widget, 30.0, State::Charging), None);
    }

    #[test]
    fn warns_when_starting_low() {
        let (mut widget, _) = Battery::new(Config::default());
        assert_eq!(
            read(&mut widget, 6.0, State::Discharging),
            Some(Alert::Low(6))
        );
    }

    #[test]
    fn says_when_charged() {
        let mut widget = battery(99.0, State::Charging);
        assert_eq!(
            read(&mut widget, 100.0, State::Full),
            Some(Alert::Charged(100))
        );
        assert_eq!(read(&mut widget, 100.0, State::Full), None);
        // Plugging in a battery that's already full isn't news.
        let mut full = battery(100.0, State::Discharging);
        assert_eq!(read(&mut full, 100.0, State::Full), None);
    }

    #[test]
    fn says_when_held_at_the_limit() {
        let mut widget = battery(79.0, State::Charging);
        let before = std::mem::replace(
            &mut widget.reading,
            Reading {
                present: true,
                percentage: 80.0,
                state: State::NotCharging,
                charge_limit: Some(80),
                ..Reading::default()
            },
        );
        assert_eq!(widget.alert(&before), Some(Alert::Charged(80)));
    }

    #[test]
    fn stays_quiet_when_told() {
        let (mut widget, _) = Battery::new(Config {
            notify: false,
            ..Config::default()
        });
        assert_eq!(read(&mut widget, 5.0, State::Discharging), None);
    }

    #[test]
    fn formats_durations() {
        assert_eq!(duration(885), "15 min");
        assert_eq!(duration(7500), "2 h 05 min");
    }

    #[test]
    fn maps_sysfs_states() {
        assert_eq!(State::from_sysfs("Charging"), State::Charging);
        assert_eq!(State::from_sysfs("Not charging"), State::NotCharging);
        assert_eq!(State::from_sysfs("Unknown"), State::Unknown);
    }
}
