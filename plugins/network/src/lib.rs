//! The primary network connection: a Wi-Fi icon with as many bars as the signal, a wired icon,
//! VPN, or a struck-through Wi-Fi icon when offline. Read from the kernel through `eave-net`, so it
//! works under any network manager. The popover has the details: network name, signal and band,
//! address, gateway, interface.

use eave_api::ui::{self, Icon, Item, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use eave_net::{Kind, Network as Status, State};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Network>("network");
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Show the Wi-Fi network name in the bar, next to the icon.
    show_name: bool,
}

pub struct Network {
    show_name: bool,
    status: Status,
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Status),
}

impl Widget for Network {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let network = Self {
            show_name: config.show_name,
            status: Status::default(),
        };
        (network, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let Message::Read(status) = message;
        self.status = status;
        Effect::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(eave_net::watch).map(Message::Read)
    }

    fn item(&self) -> Item<Message> {
        let tone = match self.status.state {
            State::Online => Tone::Normal,
            State::Connecting | State::Offline => Tone::Dim,
        };
        let content = match (self.status.state, &self.status.primary) {
            (State::Connecting | State::Offline, _) | (_, None) => {
                ui::icon(Icon::Wifi { strength: None })
            }
            (State::Online, Some(connection)) => match &connection.kind {
                Kind::Wifi { ssid, strength, .. } => {
                    let icon = ui::icon(Icon::Wifi {
                        strength: Some(f32::from(*strength) / 100.0),
                    });
                    if self.show_name && !ssid.is_empty() {
                        ui::row([icon, ui::label(ssid.clone())])
                    } else {
                        icon
                    }
                }
                Kind::Wired => ui::icon(Icon::Wired),
                Kind::Vpn => ui::label("VPN"),
                Kind::Other => ui::label(connection.interface.clone()),
            },
        };
        Item::new(content.tone(tone))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let connection = self.status.primary.as_ref()?;
        let heading = match &connection.kind {
            Kind::Wifi { ssid, .. } if !ssid.is_empty() => ssid.clone(),
            Kind::Wifi { .. } => "Wi-Fi".into(),
            Kind::Wired => "Wired".into(),
            Kind::Vpn => "VPN".into(),
            Kind::Other => connection.interface.clone(),
        };
        let mut lines = vec![ui::heading(heading)];
        if let Kind::Wifi {
            strength,
            frequency,
            ..
        } = connection.kind
        {
            let band = match frequency {
                0 => String::new(),
                f if f >= 5925 => " · 6 GHz".into(),
                f if f >= 4900 => " · 5 GHz".into(),
                _ => " · 2.4 GHz".into(),
            };
            lines.push(ui::label(format!("Signal {strength}%{band}")));
        }
        let mut detail = |name: &str, value: &Option<String>| {
            if let Some(value) = value {
                lines.push(ui::row([
                    ui::label(name.to_string()).tone(Tone::Dim),
                    ui::label(value.clone()),
                ]));
            }
        };
        detail("Address", &connection.address);
        detail("Gateway", &connection.gateway);
        detail(
            "Interface",
            &Some(connection.interface.clone()).filter(|i| !i.is_empty()),
        );
        Some(Panel::new(ui::column(lines)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eave_net::Connection;

    fn network(state: State, primary: Option<Connection>) -> Network {
        let (mut network, _) = Network::new(Config::default());
        network.update(Message::Read(Status { state, primary }));
        network
    }

    fn wifi() -> Connection {
        Connection {
            interface: "wlan0".into(),
            kind: Kind::Wifi {
                ssid: "home".into(),
                strength: 70,
                frequency: 5180,
            },
            address: Some("192.168.1.15/24".into()),
            gateway: Some("192.168.1.1".into()),
        }
    }

    #[test]
    fn popover_only_when_connected() {
        assert!(network(State::Offline, None).popover().is_none());
        assert!(network(State::Online, Some(wifi())).popover().is_some());
    }

    fn icon(network: &Network) -> Option<Icon> {
        match network.item().into_parts().content?.into_kind() {
            ui::Kind::Icon { icon, .. } => Some(icon),
            _ => None,
        }
    }

    #[test]
    fn shown_while_offline() {
        assert_eq!(
            icon(&network(State::Offline, None)),
            Some(Icon::Wifi { strength: None })
        );
    }

    #[test]
    fn wifi_shows_its_signal() {
        assert_eq!(
            icon(&network(State::Online, Some(wifi()))),
            Some(Icon::Wifi {
                strength: Some(0.7)
            })
        );
    }
}
