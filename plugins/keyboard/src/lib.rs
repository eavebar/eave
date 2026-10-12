//! The active keyboard layout. Click to switch to the next one. Hidden when only one layout is
//! configured, since there's nothing to switch.

use std::collections::BTreeMap;

use eave_api::ui::{self, Item};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use eave_hyprland::types::{Devices, Keyboard as Device};
use eave_hyprland::{Event, Update};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Keyboard>("keyboard");
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Shorter names for layouts, by the name Hyprland uses: `{ "English (US)" = "us" }`.
    aliases: BTreeMap<String, String>,
}

pub struct Keyboard {
    aliases: BTreeMap<String, String>,
    /// The keyboard whose layout we show. Other keyboards' layout events are ignored.
    device: Option<String>,
    switchable: bool,
    /// Already aliased.
    layout: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Hyprland(Update<Option<Device>>),
    Refreshed(Option<Device>),
    Next,
}

impl Widget for Keyboard {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let keyboard = Self {
            aliases: config.aliases,
            device: None,
            switchable: false,
            layout: String::new(),
        };
        (keyboard, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Hyprland(Update::Snapshot(device)) | Message::Refreshed(device) => {
                self.device = device.as_ref().map(|d| d.name.clone());
                self.switchable = device.as_ref().is_some_and(|d| d.layout.contains(','));
                self.set_layout(device.map(|d| d.active_keymap).unwrap_or_default());
            }
            Message::Hyprland(Update::Event(Event::ActiveLayout { keyboard, layout })) => {
                if self
                    .device
                    .as_ref()
                    .is_none_or(|device| *device == keyboard)
                {
                    self.set_layout(layout);
                }
            }
            // The configured layouts may have changed.
            Message::Hyprland(Update::Event(Event::ConfigReloaded)) => {
                return Task::future(async {
                    snapshot().await.unwrap_or_else(|error| {
                        eprintln!("eave-keyboard: {error}");
                        None
                    })
                })
                .map(Message::Refreshed)
                .into();
            }
            Message::Hyprland(Update::Event(_)) => {}
            Message::Next => {
                return Task::future(async {
                    match eave_hyprland::request("switchxkblayout current next").await {
                        Ok(reply) if reply.trim() == "ok" => {}
                        Ok(reply) => eprintln!("eave-keyboard: switchxkblayout: {}", reply.trim()),
                        Err(error) => eprintln!("eave-keyboard: {error}"),
                    }
                })
                .discard()
                .into();
            }
        }
        Effect::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(|| eave_hyprland::watch(snapshot)).map(Message::Hyprland)
    }

    fn item(&self) -> Item<Message> {
        if !self.switchable || self.layout.is_empty() {
            return Item::hidden();
        }
        Item::new(ui::label(self.layout.clone())).on_press(Message::Next)
    }
}

impl Keyboard {
    fn set_layout(&mut self, layout: String) {
        self.layout = self.aliases.get(&layout).cloned().unwrap_or(layout);
    }
}

async fn snapshot() -> std::io::Result<Option<Device>> {
    let devices: Devices = eave_hyprland::query("devices").await?;
    Ok(devices.main_keyboard())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(layouts: &str, active: &str) -> Option<Device> {
        Some(Device {
            name: "main".into(),
            layout: layouts.into(),
            active_keymap: active.into(),
            main: true,
        })
    }

    #[test]
    fn shows_aliased_layout_of_the_main_keyboard() {
        let config: Config = toml::from_str(r#"aliases = { "English (US)" = "us" }"#).unwrap();
        let (mut keyboard, _) = Keyboard::new(config);
        keyboard.update(Message::Hyprland(Update::Snapshot(device(
            "us,it",
            "English (US)",
        ))));
        assert_eq!(keyboard.layout, "us");

        keyboard.update(Message::Hyprland(Update::Event(Event::ActiveLayout {
            keyboard: "other".into(),
            layout: "German".into(),
        })));
        assert_eq!(keyboard.layout, "us");

        keyboard.update(Message::Hyprland(Update::Event(Event::ActiveLayout {
            keyboard: "main".into(),
            layout: "Italian".into(),
        })));
        assert_eq!(keyboard.layout, "Italian");
    }

    #[test]
    fn hidden_with_a_single_layout() {
        let (mut keyboard, _) = Keyboard::new(Config::default());
        keyboard.update(Message::Hyprland(Update::Snapshot(device(
            "us",
            "English (US)",
        ))));
        assert!(keyboard.item().into_parts().content.is_none());
    }
}
