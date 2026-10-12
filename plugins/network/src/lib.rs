//! The primary network connection from NetworkManager: Wi-Fi name and signal, wired, or offline.
//! The popover has the details: signal and band, address, gateway, interface.

use std::collections::HashMap;

use eave_api::ui::{self, Item, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use eave_dbus::zbus::zvariant::{OwnedObjectPath, OwnedValue};
use eave_dbus::zbus::{self, Connection};
use eave_dbus::{Snapshot, properties};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Network>("network");
}

const SERVICE: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Show the Wi-Fi network name in the bar, not only the signal.
    show_name: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { show_name: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    state: State,
    /// The primary connection, if there is one.
    connection: Option<Link>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Offline,
    Connecting,
    /// Connected, but NetworkManager can't reach the internet through it.
    Limited,
    Online,
}

impl State {
    /// NetworkManager's `NMState`.
    fn from_nm(state: u32) -> Self {
        match state {
            70 => Self::Online,
            50 | 60 => Self::Limited,
            30 | 40 => Self::Connecting,
            _ => Self::Offline,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Link {
    /// The connection's name: the SSID for Wi-Fi.
    name: String,
    kind: Kind,
    interface: String,
    /// `192.168.1.15/24`.
    address: Option<String>,
    gateway: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
enum Kind {
    Wifi {
        /// Percent.
        strength: u8,
        /// MHz.
        frequency: u32,
    },
    Wired,
    Vpn,
    #[default]
    Other,
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
        Subscription::run(|| eave_dbus::watch(SERVICE, read)).map(Message::Read)
    }

    fn item(&self) -> Item<Message> {
        let tone = match self.status.state {
            State::Online => Tone::Normal,
            State::Limited => Tone::Warning,
            State::Connecting | State::Offline => Tone::Dim,
        };
        let content = match (self.status.state, &self.status.connection) {
            (State::Connecting, _) => ui::label("Connecting…"),
            (State::Offline, _) | (_, None) => ui::label("Offline"),
            (_, Some(link)) => match link.kind {
                Kind::Wifi { strength, .. } if self.show_name => ui::row([
                    ui::label(link.name.clone()),
                    ui::label(format!("{strength}%")).tone(Tone::Dim),
                ]),
                Kind::Wifi { strength, .. } => ui::label(format!("Wi-Fi {strength}%")),
                Kind::Wired => ui::label("Wired"),
                Kind::Vpn | Kind::Other => ui::label(link.name.clone()),
            },
        };
        Item::new(content.tone(tone))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let link = self.status.connection.as_ref()?;
        let mut lines = vec![ui::heading(link.name.clone())];
        if self.status.state == State::Limited {
            lines.push(ui::label("No internet access").tone(Tone::Warning));
        }
        if let Kind::Wifi {
            strength,
            frequency,
        } = link.kind
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
        detail("Address", &link.address);
        detail("Gateway", &link.gateway);
        detail(
            "Interface",
            &Some(link.interface.clone()).filter(|i| !i.is_empty()),
        );
        Some(Panel::new(ui::column(lines)))
    }
}

async fn read(connection: Connection) -> zbus::Result<Snapshot<Status>> {
    let props = |path: OwnedObjectPath, interface| {
        let connection = connection.clone();
        async move { properties(&connection, SERVICE, path.as_str(), interface).await }
    };

    let root = properties(&connection, SERVICE, ROOT, "org.freedesktop.NetworkManager").await?;
    let mut watch = vec![ROOT.try_into()?];
    let state = State::from_nm(root.get("State"));
    let Some(primary) = root.path("PrimaryConnection") else {
        return Ok(Snapshot {
            state: Status {
                state,
                connection: None,
            },
            watch,
        });
    };
    watch.push(primary.clone());

    let active = props(primary, "org.freedesktop.NetworkManager.Connection.Active").await?;
    let kind: String = active.get("Type");
    let mut link = Link {
        name: active.get("Id"),
        ..Link::default()
    };

    let devices: Vec<OwnedObjectPath> = active.get("Devices");
    if let Some(device_path) = devices.into_iter().next() {
        watch.push(device_path.clone());
        let device = props(device_path.clone(), "org.freedesktop.NetworkManager.Device").await?;
        link.interface = device.get("Interface");

        if kind == "802-11-wireless" {
            let wireless = props(
                device_path,
                "org.freedesktop.NetworkManager.Device.Wireless",
            )
            .await?;
            let (mut strength, mut frequency) = (0, 0);
            if let Some(ap_path) = wireless.path("ActiveAccessPoint") {
                watch.push(ap_path.clone());
                let ap = props(ap_path, "org.freedesktop.NetworkManager.AccessPoint").await?;
                strength = ap.get::<u8>("Strength").min(100);
                frequency = ap.get("Frequency");
            }
            link.kind = Kind::Wifi {
                strength,
                frequency,
            };
        }
    }
    link.kind = match kind.as_str() {
        "802-11-wireless" => link.kind,
        "802-3-ethernet" => Kind::Wired,
        "vpn" | "wireguard" => Kind::Vpn,
        _ => Kind::Other,
    };

    if let Some(ip4_path) = active.path("Ip4Config") {
        watch.push(ip4_path.clone());
        let ip4 = props(ip4_path, "org.freedesktop.NetworkManager.IP4Config").await?;
        link.address = first_address(ip4.value("AddressData"));
        link.gateway = Some(ip4.get::<String>("Gateway")).filter(|g| !g.is_empty());
    }

    Ok(Snapshot {
        state: Status {
            state,
            connection: Some(link),
        },
        watch,
    })
}

/// The first `address/prefix` in an `AddressData` property (`aa{sv}`).
fn first_address(value: Option<&OwnedValue>) -> Option<String> {
    let entries: Vec<HashMap<String, OwnedValue>> = value?.try_clone().ok()?.try_into().ok()?;
    let entry = entries.into_iter().next()?;
    let address: String = entry.get("address")?.try_clone().ok()?.try_into().ok()?;
    let prefix: u32 = entry
        .get("prefix")
        .and_then(|p| p.try_clone().ok())
        .and_then(|p| p.try_into().ok())
        .unwrap_or(0);
    Some(if prefix > 0 {
        format!("{address}/{prefix}")
    } else {
        address
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(state: State, connection: Option<Link>) -> Network {
        let (mut network, _) = Network::new(Config::default());
        network.update(Message::Read(Status { state, connection }));
        network
    }

    fn wifi() -> Link {
        Link {
            name: "home".into(),
            kind: Kind::Wifi {
                strength: 70,
                frequency: 5180,
            },
            interface: "wlan0".into(),
            address: Some("192.168.1.15/24".into()),
            gateway: Some("192.168.1.1".into()),
        }
    }

    #[test]
    fn maps_nm_states() {
        assert_eq!(State::from_nm(70), State::Online);
        assert_eq!(State::from_nm(60), State::Limited);
        assert_eq!(State::from_nm(40), State::Connecting);
        assert_eq!(State::from_nm(20), State::Offline);
    }

    #[test]
    fn popover_only_when_connected() {
        assert!(network(State::Offline, None).popover().is_none());
        assert!(network(State::Online, Some(wifi())).popover().is_some());
    }

    #[test]
    fn reads_address_data() {
        use eave_dbus::zbus::zvariant::Value;
        let mut entry = HashMap::new();
        entry.insert("address".to_string(), Value::from("10.0.0.2"));
        entry.insert("prefix".to_string(), Value::from(8u32));
        let value = OwnedValue::try_from(Value::from(vec![entry])).unwrap();
        assert_eq!(first_address(Some(&value)).as_deref(), Some("10.0.0.2/8"));
    }
}
