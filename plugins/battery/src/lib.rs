//! Battery level, as a battery icon filled to the charge: green while charging, red when nearly
//! empty. Hovering it shows the percentage instead, gulping while it charges. Read from the kernel's
//! `/sys/class/power_supply`, with every system battery combined into one. Hidden when there's no
//! battery (a desktop). The popover has the state, the time left and the power draw.

use eave_api::ui::{self, Icon, Item, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use serde::Deserialize;

mod sysfs;

pub fn register(registry: &mut Registry) {
    registry.widget::<Battery>("battery");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Percentage at or below which a discharging battery shows as a warning.
    warning: u8,
    /// Percentage at or below which it shows as critical.
    critical: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            warning: 20,
            critical: 10,
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
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Reading),
}

impl Widget for Battery {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let battery = Self {
            config,
            reading: Reading::default(),
        };
        (battery, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let Message::Read(reading) = message;
        self.reading = reading;
        Effect::none()
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

        let mut lines = vec![hero, meter, ui::section("Power")];
        let plugged = reading.ac_online.unwrap_or(matches!(
            reading.state,
            State::Charging | State::Full | State::NotCharging
        ));
        let source = if plugged { "Charger" } else { "Battery" };
        lines.push(ui::detail("Source", ui::label(source)));
        if reading.energy_rate > 0.05 {
            let direction = match reading.state {
                State::Charging => "Charging at",
                _ => "Drawing",
            };
            let watts = ui::label(format!("{:.1} W", reading.energy_rate));
            lines.push(ui::detail(direction, watts));
        }
        if let (Some(energy), Some(full)) = (reading.energy, reading.energy_full) {
            lines.push(ui::detail(
                "Energy",
                of(format!("{energy:.1}"), format!("of {full:.1} Wh")),
            ));
        }
        if let Some(limit) = limit {
            lines.push(ui::detail("Charge limit", ui::label(limit)));
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
        if !health.is_empty() {
            lines.push(ui::section("Health"));
            lines.extend(health);
        }
        Some(Panel::new(ui::column(lines)))
    }
}

/// A value and, dimmed after it, what it's out of: `36.5 of 57.8 Wh`.
fn of(value: String, whole: String) -> ui::Node<Message> {
    ui::row([ui::label(value), ui::label(whole).tone(Tone::Dim)])
}

impl Battery {
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
