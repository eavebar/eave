//! Battery level, as a battery icon filled to the charge: green while charging, red when nearly
//! empty. Hovering it shows the percentage instead. Read from the kernel's
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
            charging: self.reading.state == State::Charging,
        });
        Item::new(ui::hover(icon, ui::label(format!("{percent}%"))).tone(self.tone()))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        if !self.reading.present {
            return None;
        }
        let reading = &self.reading;
        let status = match reading.state {
            State::Charging if reading.time_to_full > 0 => {
                format!("Charging, full in {}", duration(reading.time_to_full))
            }
            State::Charging => "Charging".into(),
            State::Discharging if reading.time_to_empty > 0 => {
                format!("{} left", duration(reading.time_to_empty))
            }
            State::Discharging => "On battery".into(),
            State::Full => "Fully charged".into(),
            State::NotCharging => "Plugged in, not charging".into(),
            State::Unknown => "Unknown state".into(),
        };

        let mut lines = vec![
            ui::heading(format!("Battery {}%", self.percent())),
            ui::label(status),
        ];
        if reading.energy_rate > 0.05 {
            let direction = match reading.state {
                State::Charging => "Charging at",
                _ => "Drawing",
            };
            lines.push(
                ui::label(format!("{direction} {:.1} W", reading.energy_rate)).tone(Tone::Dim),
            );
        }
        Some(Panel::new(ui::column(lines)))
    }
}

impl Battery {
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
