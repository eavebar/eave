//! The active keyboard layout. Click to switch to the next one, right click for the list of
//! layouts. Hidden when only one layout is configured, since there's nothing to switch.

use std::collections::BTreeMap;

use eave_api::ui::{self, Action, Icon, Item, Panel};
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
    /// The configured layouts, in Hyprland's order.
    layouts: Vec<Layout>,
    /// Which of `layouts` is active.
    active: Option<usize>,
    /// Long names by layout code and variant, read from the xkb rules once the list is first
    /// opened.
    names: Option<BTreeMap<(String, String), String>>,
}

/// One configured layout: its code (`us`) and variant (`intl`, or empty).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layout {
    code: String,
    variant: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Hyprland(Update<Option<Device>>),
    Refreshed(Option<Device>),
    Next,
    /// Opens the list of layouts.
    List,
    Opened,
    Names(BTreeMap<(String, String), String>),
    Switch(usize),
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
            layouts: Vec::new(),
            active: None,
            names: None,
        };
        (keyboard, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Hyprland(Update::Snapshot(device)) | Message::Refreshed(device) => {
                self.device = device.as_ref().map(|d| d.name.clone());
                self.switchable = device.as_ref().is_some_and(|d| d.layout.contains(','));
                self.layouts = device.as_ref().map(layouts).unwrap_or_default();
                self.active = device.as_ref().and_then(|d| d.active_layout_index);
                self.set_layout(device.map(|d| d.active_keymap).unwrap_or_default());
            }
            Message::Hyprland(Update::Event(Event::ActiveLayout { keyboard, layout })) => {
                if self
                    .device
                    .as_ref()
                    .is_none_or(|device| *device == keyboard)
                {
                    self.set_layout(layout);
                    // The event names the layout but not its place in the list.
                    return refresh();
                }
            }
            // The configured layouts may have changed.
            Message::Hyprland(Update::Event(Event::ConfigReloaded)) => return refresh(),
            Message::Hyprland(Update::Event(_)) => {}
            Message::Next => return switch("next".into()).into(),
            Message::List => return Effect::none().open_popover(),
            Message::Opened if self.names.is_none() => {
                return Task::future(async {
                    std::fs::read_to_string(XKB_RULES)
                        .map(|rules| names(&rules))
                        .unwrap_or_default()
                })
                .map(Message::Names)
                .into();
            }
            Message::Opened => {}
            Message::Names(names) => self.names = Some(names),
            Message::Switch(index) => {
                return Effect::task(switch(index.to_string())).close_popover();
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
        Item::new(ui::label(self.layout.clone()))
            .on_press(Message::Next)
            .on_secondary(Message::List)
    }

    fn popover(&self) -> Option<Panel<Message>> {
        if !self.switchable {
            return None;
        }
        let rows = self.layouts.iter().enumerate().map(|(index, layout)| {
            let active = self.active == Some(index);
            let key = (layout.code.clone(), layout.variant.clone());
            let name = self
                .names
                .as_ref()
                .and_then(|names| names.get(&key))
                .cloned()
                .unwrap_or_else(|| layout.code.clone());
            let code = if layout.variant.is_empty() {
                layout.code.clone()
            } else {
                format!("{} · {}", layout.code, layout.variant)
            };
            let row = ui::list_item(name)
                .leading(ui::badge(layout.code.to_uppercase()))
                .subtitle(code)
                .selected(active)
                .on_press(Message::Switch(index));
            if active {
                row.trailing(ui::icon(Icon::Check))
            } else {
                row
            }
        });
        let content = ui::column([ui::header("Keyboard"), ui::list(rows)]);
        Some(Panel::new(content).on_open(Message::Opened))
    }

    fn actions(&self) -> Vec<Action<Message>> {
        if !self.switchable {
            return Vec::new();
        }
        let words = ["keyboard", "layout", "language", "input"];
        let mut actions = vec![
            ui::action("Next keyboard layout", Message::Next)
                .subtitle(self.layout.clone())
                .keywords(words),
        ];
        for (index, layout) in self.layouts.iter().enumerate() {
            if self.active == Some(index) {
                continue;
            }
            let key = (layout.code.clone(), layout.variant.clone());
            // Long names once the popover has read them; codes until then.
            let name = self
                .names
                .as_ref()
                .and_then(|names| names.get(&key))
                .cloned()
                .unwrap_or_else(|| match layout.variant.as_str() {
                    "" => layout.code.clone(),
                    variant => format!("{} {variant}", layout.code),
                });
            actions.push(
                ui::action(format!("Switch to {name}"), Message::Switch(index))
                    .subtitle("Keyboard layout")
                    .keywords(
                        words
                            .into_iter()
                            .map(str::to_owned)
                            .chain([layout.code.clone(), layout.variant.clone()]),
                    )
                    .leading(ui::badge(layout.code.to_uppercase())),
            );
        }
        actions
    }
}

/// Where xkb lists every layout and variant with its long name.
const XKB_RULES: &str = "/usr/share/X11/xkb/rules/evdev.lst";

/// Long names by layout code and variant (empty for a layout's default), from xkb's
/// `evdev.lst`: `  us              English (US)` under `! layout`, and
/// `  intl            us: English (US, intl., with dead keys)` under `! variant`.
fn names(rules: &str) -> BTreeMap<(String, String), String> {
    let mut names = BTreeMap::new();
    let mut section = "";
    for line in rules.lines() {
        if let Some(name) = line.strip_prefix("! ") {
            section = name.trim();
            continue;
        }
        let Some((key, rest)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim();
        match section {
            "layout" => {
                names.insert((key.to_string(), String::new()), rest.to_string());
            }
            "variant" => {
                if let Some((code, name)) = rest.split_once(": ") {
                    names.insert((code.to_string(), key.to_string()), name.to_string());
                }
            }
            _ => {}
        }
    }
    names
}

/// The layouts a keyboard has, from its comma-separated layouts and variants.
fn layouts(device: &Device) -> Vec<Layout> {
    let mut variants = device.variant.split(',');
    device
        .layout
        .split(',')
        .map(|code| Layout {
            code: code.trim().to_string(),
            variant: variants.next().unwrap_or("").trim().to_string(),
        })
        .collect()
}

fn refresh() -> Effect<Message> {
    Task::future(async {
        snapshot().await.unwrap_or_else(|error| {
            eprintln!("eave-keyboard: {error}");
            None
        })
    })
    .map(Message::Refreshed)
    .into()
}

/// Switches the main keyboard to `to`: `next`, or a layout's index.
fn switch(to: String) -> Task<Message> {
    Task::future(async move {
        match eave_hyprland::request(&format!("switchxkblayout current {to}")).await {
            Ok(reply) if reply.trim() == "ok" => {}
            Ok(reply) => eprintln!("eave-keyboard: switchxkblayout: {}", reply.trim()),
            Err(error) => eprintln!("eave-keyboard: {error}"),
        }
    })
    .discard()
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
            ..Device::default()
        })
    }

    #[test]
    fn its_actions_switch_to_the_other_layouts() {
        let titles = |keyboard: &Keyboard| {
            keyboard
                .actions()
                .into_iter()
                .map(|action| action.title().to_owned())
                .collect::<Vec<_>>()
        };
        let (mut keyboard, _) = Keyboard::new(Config::default());
        keyboard.update(Message::Refreshed(device("us", "English (US)")));
        assert!(titles(&keyboard).is_empty(), "nothing to switch to");
        let mut two = device("us,it", "English (US)").unwrap();
        two.active_layout_index = Some(0);
        keyboard.update(Message::Refreshed(Some(two)));
        assert_eq!(titles(&keyboard), ["Next keyboard layout", "Switch to it"]);
    }

    #[test]
    fn reads_long_names_from_the_xkb_rules() {
        let rules = "! model\n  pc105  Generic 105-key PC\n! layout\n  us              English (US)\n  it              Italian\n! variant\n  intl            us: English (US, intl., with dead keys)\n! option\n  grp             Switching";
        let names = names(rules);
        assert_eq!(names[&("us".into(), String::new())], "English (US)");
        assert_eq!(names[&("it".into(), String::new())], "Italian");
        assert_eq!(
            names[&("us".into(), "intl".into())],
            "English (US, intl., with dead keys)"
        );
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn pairs_layouts_with_their_variants() {
        let device = Device {
            layout: "us,it,us".into(),
            variant: ",,intl".into(),
            ..Device::default()
        };
        let layouts = layouts(&device);
        assert_eq!(layouts.len(), 3);
        assert_eq!(layouts[2].variant, "intl");
        assert_eq!(layouts[1].variant, "");
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
