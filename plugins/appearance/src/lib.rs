//! Light or dark mode, for the whole desktop: a sun or a moon, and a click switches. The popover
//! (a secondary click) has both to pick from. It sets GNOME's `color-scheme` through dconf's own
//! D-Bus writer, the setting the settings portal hands to every app that asks, GTK and libadwaita
//! ones included. The bar follows it with `theme = "system"` (D-92).
//!
//! It reads the scheme back from the portal, so a change made anywhere else shows here too, and
//! nudges the island up for light and down for dark. Without dconf on the session bus it has
//! nothing to set, and hides.

use eave_api::ui::{self, Action, Icon, Item, Panel, Tone};
use eave_api::{Effect, NoConfig, Nudge, Registry, Subscription, Task, Widget};
use eave_dbus::portal::{self, ColorScheme};

const DCONF: &str = "ca.desrt.dconf";
const WRITER_PATH: &str = "/ca/desrt/dconf/Writer/user";
const WRITER: &str = "ca.desrt.dconf.Writer";
/// The key GNOME's settings and the portal backends that read them use for dark mode.
const KEY: &str = "/org/gnome/desktop/interface/color-scheme";

pub fn register(registry: &mut Registry) {
    registry.widget::<Appearance>("appearance");
}

/// Light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    /// What the setting says for it.
    fn value(self) -> &'static str {
        match self {
            Mode::Light => "prefer-light",
            Mode::Dark => "prefer-dark",
        }
    }
}

pub struct Appearance {
    /// Whether dconf is there to set the scheme with. `None` until it's been looked for.
    available: Option<bool>,
    /// The scheme as shown: what the portal last said, or what was just set, ahead of it.
    scheme: ColorScheme,
    /// What the portal last said, which only a change in nudges. `None` before it first says.
    heard: Option<ColorScheme>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Whether dconf is on the session bus.
    Available(bool),
    /// The portal's scheme, now or since it changed.
    Heard(ColorScheme),
    /// Switch to the other mode.
    Toggle,
    Set(Mode),
    /// Setting it went wrong: why.
    Failed(String),
    OpenPopover,
}

impl Widget for Appearance {
    type Config = NoConfig;
    type Message = Message;

    fn new(_: NoConfig) -> (Self, Task<Message>) {
        let appearance = Self {
            available: None,
            scheme: ColorScheme::NoPreference,
            heard: None,
        };
        (
            appearance,
            Task::future(available()).map(Message::Available),
        )
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Available(available) => {
                self.available = Some(available);
                Effect::none()
            }
            Message::Heard(scheme) => {
                self.scheme = scheme;
                let before = self.heard.replace(scheme);
                match (before, scheme) {
                    (Some(before), ColorScheme::Light) if before != scheme => {
                        Effect::none().nudge(Nudge::Up)
                    }
                    (Some(before), ColorScheme::Dark) if before != scheme => {
                        Effect::none().nudge(Nudge::Down)
                    }
                    _ => Effect::none(),
                }
            }
            Message::Toggle => self.set(match self.mode() {
                Mode::Light => Mode::Dark,
                Mode::Dark => Mode::Light,
            }),
            Message::Set(mode) => self.set(mode),
            Message::Failed(error) => {
                eprintln!("eave-appearance: couldn't set the color scheme: {error}");
                // Back to what the portal says it is.
                self.scheme = self.heard.unwrap_or_default();
                Effect::none()
            }
            Message::OpenPopover => Effect::none().open_popover(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(portal::color_scheme).map(Message::Heard)
    }

    fn item(&self) -> Item<Message> {
        if self.available != Some(true) {
            return Item::hidden();
        }
        let dark = self.mode() == Mode::Dark;
        Item::new(ui::icon(Icon::Appearance { dark }))
            .on_press(Message::Toggle)
            .on_secondary(Message::OpenPopover)
    }

    fn popover(&self) -> Option<Panel<Message>> {
        if self.available != Some(true) {
            return None;
        }
        let option = |mode: Mode, name: &str| {
            let dark = mode == Mode::Dark;
            let content = ui::row([ui::icon(Icon::Appearance { dark }), ui::label(name)]);
            (content, Message::Set(mode))
        };
        let current = match self.scheme {
            ColorScheme::Light => Some(0),
            ColorScheme::Dark => Some(1),
            ColorScheme::NoPreference => None,
        };
        let content = ui::column([
            ui::header("Appearance"),
            ui::switcher(
                [option(Mode::Light, "Light"), option(Mode::Dark, "Dark")],
                current,
            ),
            ui::label("For apps that follow the desktop, and the bar with theme = \"system\"")
                .tone(Tone::Dim),
        ]);
        Some(Panel::new(content))
    }

    fn actions(&self) -> Vec<Action<Message>> {
        if self.available != Some(true) {
            return Vec::new();
        }
        let words = ["appearance", "theme", "mode", "color scheme"];
        let icon = |dark| ui::icon(Icon::Appearance { dark });
        vec![
            ui::action("Light mode", Message::Set(Mode::Light))
                .id("light")
                .keywords(words.into_iter().chain(["light", "day", "bright"]))
                .leading(icon(false)),
            ui::action("Dark mode", Message::Set(Mode::Dark))
                .id("dark")
                .keywords(words.into_iter().chain(["dark", "night"]))
                .leading(icon(true)),
        ]
    }
}

impl Appearance {
    /// The mode shown: dark only when the desktop says so, since apps are light without a
    /// preference.
    fn mode(&self) -> Mode {
        match self.scheme {
            ColorScheme::Dark => Mode::Dark,
            ColorScheme::Light | ColorScheme::NoPreference => Mode::Light,
        }
    }

    /// Shows `mode` at once and sets it; the portal's word on it follows.
    fn set(&mut self, mode: Mode) -> Effect<Message> {
        self.scheme = match mode {
            Mode::Light => ColorScheme::Light,
            Mode::Dark => ColorScheme::Dark,
        };
        let task = Task::future(write(mode)).then(|result| match result {
            Ok(()) => Task::none(),
            Err(error) => Task::done(Message::Failed(error.to_string())),
        });
        Effect::task(task)
    }
}

/// Whether dconf is on the session bus, or can be started on it.
async fn available() -> bool {
    async fn look() -> eave_dbus::zbus::Result<bool> {
        let connection = eave_dbus::session().await?;
        let bus = eave_dbus::zbus::fdo::DBusProxy::new(&connection).await?;
        let names = bus.list_activatable_names().await?;
        if names.iter().any(|name| name.as_str() == DCONF) {
            return Ok(true);
        }
        let names = bus.list_names().await?;
        Ok(names.iter().any(|name| name.as_str() == DCONF))
    }
    look().await.unwrap_or(false)
}

/// Sets the desktop's color scheme to `mode`.
async fn write(mode: Mode) -> eave_dbus::zbus::Result<()> {
    let connection = eave_dbus::session().await?;
    connection
        .call_method(
            Some(DCONF),
            WRITER_PATH,
            Some(WRITER),
            "Change",
            &(changeset(KEY, mode.value()),),
        )
        .await?;
    Ok(())
}

/// One key set to a string, as dconf's writer takes it: a GVariant `a{smv}`, serialized. The
/// GVariant format is little-endian here, as on every machine eave runs on, and every offset in
/// it fits in a byte, since the key and value are short.
fn changeset(key: &str, value: &str) -> Vec<u8> {
    // The entry: the key, then the value aligned to 8 (a variant's alignment) as a maybe holding
    // a variant: the string, its type after a 0, and a 0 that says the maybe is set. Then where
    // the key ends.
    let mut entry = Vec::new();
    entry.extend_from_slice(key.as_bytes());
    entry.push(0);
    let key_end = entry.len();
    entry.resize(key_end.next_multiple_of(8), 0);
    entry.extend_from_slice(value.as_bytes());
    entry.extend_from_slice(b"\0\0s\0");
    entry.push(key_end as u8);
    // The array of that one entry, then where it ends.
    let mut array = entry;
    array.push(array.len() as u8);
    assert!(array.len() < 256, "offsets in one byte");
    array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_changeset_is_gvariant() {
        // a{smv}: {'/a/b': <'x'>}, as GLib serializes it: the key and its 0, padding to 8, the
        // value 'x', its 0, the variant's type 's', the maybe's 0, the key's end (5), then the
        // entry's end (13).
        let expected = [
            b'/', b'a', b'/', b'b', 0, 0, 0, 0, b'x', 0, 0, b's', 0, 5, 14,
        ];
        assert_eq!(changeset("/a/b", "x"), expected);
        // The real one, checked against GLib's own: the key ends at 42, the entry at 64.
        let real = changeset(KEY, Mode::Dark.value());
        assert_eq!(real.len(), 65);
        assert_eq!(&real[48..], b"prefer-dark\0\0s\0\x2a\x40");
    }

    fn appearance(scheme: ColorScheme) -> Appearance {
        let (mut appearance, _) = Appearance::new(NoConfig::default());
        appearance.update(Message::Available(true));
        appearance.update(Message::Heard(scheme));
        appearance
    }

    fn nudge(effect: Effect<Message>) -> Option<Nudge> {
        effect.into_parts().2.map(|request| request.nudge)
    }

    #[test]
    fn hidden_without_dconf() {
        let (mut appearance, _) = Appearance::new(NoConfig::default());
        assert!(appearance.item().into_parts().content.is_none());
        appearance.update(Message::Available(false));
        assert!(appearance.item().into_parts().content.is_none());
        assert!(appearance.popover().is_none());
        assert!(appearance.actions().is_empty());
    }

    #[test]
    fn a_click_switches_to_the_other_mode() {
        let mut appearance = appearance(ColorScheme::Light);
        appearance.update(Message::Toggle);
        assert_eq!(appearance.mode(), Mode::Dark);
        appearance.update(Message::Toggle);
        assert_eq!(appearance.mode(), Mode::Light);
        // No preference reads as light, as apps show it.
        let mut appearance = appearance_with_no_preference();
        appearance.update(Message::Toggle);
        assert_eq!(appearance.mode(), Mode::Dark);
    }

    fn appearance_with_no_preference() -> Appearance {
        appearance(ColorScheme::NoPreference)
    }

    #[test]
    fn a_change_heard_nudges_up_for_light_and_down_for_dark() {
        let mut appearance = appearance(ColorScheme::Light);
        assert_eq!(
            nudge(appearance.update(Message::Heard(ColorScheme::Dark))),
            Some(Nudge::Down)
        );
        assert_eq!(
            nudge(appearance.update(Message::Heard(ColorScheme::Dark))),
            None
        );
        assert_eq!(
            nudge(appearance.update(Message::Heard(ColorScheme::Light))),
            Some(Nudge::Up)
        );
    }

    #[test]
    fn a_failed_change_goes_back_to_what_the_portal_says() {
        let mut appearance = appearance(ColorScheme::Light);
        appearance.update(Message::Set(Mode::Dark));
        appearance.update(Message::Failed("no dconf".into()));
        assert_eq!(appearance.mode(), Mode::Light);
    }

    #[test]
    fn the_popover_marks_the_current_mode() {
        let current = |appearance: &Appearance| {
            let panel = appearance.popover().unwrap();
            let ui::Kind::Column(lines) = panel.into_node().into_kind() else {
                panic!("a column");
            };
            let mut lines = lines.into_iter();
            lines.next();
            match lines.next().unwrap().into_kind() {
                ui::Kind::Switcher { current, .. } => current,
                _ => panic!("a switcher"),
            }
        };
        assert_eq!(current(&appearance(ColorScheme::Light)), Some(0));
        assert_eq!(current(&appearance(ColorScheme::Dark)), Some(1));
        assert_eq!(current(&appearance_with_no_preference()), None);
    }
}
