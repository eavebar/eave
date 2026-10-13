//! The screen's brightness, as a sun whose rays light up with the level. Scroll to change it.
//! The popover has a slider for the display and, when there is one, the keyboard's backlight.
//! Whenever the display's level goes up or down, from here, a brightness key or anywhere else,
//! the island gets a nudge that way, and the item shows the percent for a moment instead of the
//! sun. It shows the percent while hovered, too. Percents follow a perceptual curve
//! ([`curve`]), so the steps look even.

mod curve;
mod sysfs;

use eave_api::time::{Duration, every, sleep};
use eave_api::ui::{self, Icon, Item, Panel, Tone};
use eave_api::{Effect, Nudge, Registry, Subscription, Task, Widget};
use serde::Deserialize;

use sysfs::{Light, Lights};

/// How long the percent shows after the last change.
const LEVEL_SHOWN: Duration = Duration::from_millis(1500);
/// How often the keyboard's backlight is re-read while the popover is open: LEDs send no
/// uevent when they change.
const KEYBOARD_POLL: Duration = Duration::from_secs(2);

pub fn register(registry: &mut Registry) {
    registry.widget::<Brightness>("brightness");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Percent per scroll step.
    step: u32,
    /// The darkest percent eave will set. 0 turns some panels off.
    min: u32,
    /// Whether the popover shows the keyboard's backlight, when there is one.
    keyboard: bool,
    /// The backlight to use, by its name in `/sys/class/backlight`. The first one by default.
    device: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            step: 5,
            min: 1,
            keyboard: true,
            device: None,
        }
    }
}

/// A light and the percent it shows at.
#[derive(Debug, Clone, PartialEq)]
struct Level {
    light: Light,
    percent: u32,
}

impl Level {
    /// `light` as a percent. A reading of the raw level we set keeps the percent we set it to:
    /// on a coarse backlight several percents share a raw level, and the one shown shouldn't
    /// jump to another.
    fn read(light: Light, before: Option<&Level>) -> Self {
        let percent = match before {
            Some(before)
                if before.light.name == light.name
                    && curve::raw(before.percent, light.max) == light.raw =>
            {
                before.percent
            }
            _ => curve::percent(light.raw, light.max),
        };
        Self { light, percent }
    }
}

pub struct Brightness {
    config: Config,
    display: Option<Level>,
    keyboard: Option<Level>,
    open: bool,
    /// Levels being sent: one request at a time per slider, and the latest level dragged to
    /// while it was out, so a drag never queues up requests.
    sending: Vec<(Target, Option<u32>)>,
    /// The display and its percent as the kernel last reported them, which our own changes only
    /// show ahead of. A change in the percent is what nudges.
    heard: Option<(String, u32)>,
    /// Whether the item shows the percent instead of the sun, after a change.
    showing_level: bool,
    /// Counts changes, so only the latest one's timer puts the sun back.
    changes: u64,
}

/// What a slider sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Display,
    Keyboard,
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Lights),
    /// The keyboard's backlight, re-read while the popover is open.
    Keyboard(Option<Light>),
    /// Time to re-read the keyboard's backlight.
    Poll,
    Up,
    Down,
    /// A slider moved.
    Set(Target, u32),
    /// The level last sent for this target got there.
    Sent(Target),
    Opened,
    Closed,
    /// The percent has shown long enough after change number this.
    HideLevel(u64),
}

impl Widget for Brightness {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let brightness = Self {
            config: Config {
                step: config.step.clamp(1, 100),
                min: config.min.min(100),
                ..config
            },
            display: None,
            keyboard: None,
            open: false,
            sending: Vec::new(),
            heard: None,
            showing_level: false,
            changes: 0,
        };
        (brightness, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Read(lights) => self.read(lights),
            Message::Keyboard(light) => {
                self.read_keyboard(light);
                Effect::none()
            }
            Message::Poll => Task::future(async { sysfs::keyboard() })
                .map(Message::Keyboard)
                .into(),
            Message::Up => self.change(i64::from(self.config.step)),
            Message::Down => self.change(-i64::from(self.config.step)),
            Message::Set(target, percent) => {
                let Some(level) = self.level_mut(target) else {
                    return Effect::none();
                };
                level.percent = percent;
                self.send(target, percent).into()
            }
            Message::Sent(target) => {
                let Some(at) = self.sending.iter().position(|(t, _)| *t == target) else {
                    return Effect::none();
                };
                match self.sending[at].1.take() {
                    Some(percent) => self.set(target, percent).into(),
                    None => {
                        self.sending.remove(at);
                        Effect::none()
                    }
                }
            }
            Message::Opened => {
                self.open = true;
                Effect::none()
            }
            Message::Closed => {
                self.open = false;
                Effect::none()
            }
            Message::HideLevel(change) => {
                if change == self.changes {
                    self.showing_level = false;
                }
                Effect::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let lights = Subscription::run(sysfs::watch).map(Message::Read);
        if !(self.open && self.keyboard.is_some()) {
            return lights;
        }
        let keyboard = every(KEYBOARD_POLL).map(|_| Message::Poll);
        Subscription::batch([lights, keyboard])
    }

    fn item(&self) -> Item<Message> {
        let Some(display) = &self.display else {
            return Item::hidden();
        };
        let icon = ui::icon(Icon::Brightness {
            level: display.percent as f32 / 100.0,
        });
        let level = ui::counter(display.percent.into(), 0..=100, "%");
        let content = ui::alternate([icon, level], self.showing_level.into()).on_hover(1);
        Item::new(content).on_scroll(Message::Up, Message::Down)
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let display = self.display.as_ref()?;
        let sun = Icon::Brightness {
            level: display.percent as f32 / 100.0,
        };
        let min = self.config.min.min(display.percent);
        let mut lines = vec![
            ui::header("Brightness"),
            control(sun, Target::Display, display.percent, min),
            ui::label("Built-in display").tone(Tone::Dim),
        ];
        if let Some(keyboard) = &self.keyboard {
            lines.extend([
                ui::separator(),
                control(Icon::KeyboardLight, Target::Keyboard, keyboard.percent, 0),
                ui::label("Keyboard backlight").tone(Tone::Dim),
            ]);
        }
        let panel = Panel::new(ui::column(lines))
            .on_open(Message::Opened)
            .on_close(Message::Closed);
        Some(panel)
    }
}

/// A light's icon, its slider and its level.
fn control(icon: Icon, target: Target, percent: u32, min: u32) -> ui::Node<Message> {
    ui::row([
        ui::icon(icon),
        ui::slider(percent as f32, min as f32..=100.0, move |percent| {
            Message::Set(target, percent.round() as u32)
        }),
        ui::counter(percent.into(), 0..=100, "%"),
    ])
}

impl Brightness {
    fn level_mut(&mut self, target: Target) -> Option<&mut Level> {
        match target {
            Target::Display => self.display.as_mut(),
            Target::Keyboard => self.keyboard.as_mut(),
        }
    }

    /// Takes in the kernel's reading. A level that went up or down on the same display nudges
    /// that way. Our own changes come back here too, one reading per step, so each nudges once.
    fn read(&mut self, lights: Lights) -> Effect<Message> {
        let Lights {
            mut backlights,
            keyboard,
        } = lights;
        let chosen = match &self.config.device {
            Some(name) => backlights.iter().position(|light| light.name == *name),
            None => (!backlights.is_empty()).then_some(0),
        };
        let display = chosen.map(|at| backlights.swap_remove(at));
        let mut display = display.map(|light| Level::read(light, self.display.as_ref()));
        // A slider still being dragged keeps the level it was dragged to: readings lag behind it.
        if let (Some(ours), Some(theirs)) = (&self.display, &mut display)
            && ours.light.name == theirs.light.name
            && self.sending.iter().any(|(t, _)| *t == Target::Display)
        {
            theirs.percent = ours.percent;
        }
        self.read_keyboard(keyboard);

        let heard = display
            .as_ref()
            .map(|level| (level.light.name.clone(), level.percent));
        let before = std::mem::replace(&mut self.heard, heard.clone());
        self.display = display;
        let nudge = match (before, heard) {
            (Some((name, from)), Some((same, to))) if name == same && to > from => Nudge::Up,
            (Some((name, from)), Some((same, to))) if name == same && to < from => Nudge::Down,
            _ => return Effect::none(),
        };
        Effect::task(self.show_level()).nudge(nudge)
    }

    fn read_keyboard(&mut self, light: Option<Light>) {
        if !self.config.keyboard {
            return;
        }
        if self.sending.iter().any(|(t, _)| *t == Target::Keyboard) {
            return;
        }
        self.keyboard = light.map(|light| Level::read(light, self.keyboard.as_ref()));
    }

    /// Shows the percent instead of the sun, until a moment after the last change.
    fn show_level(&mut self) -> Task<Message> {
        self.showing_level = true;
        self.changes += 1;
        let change = self.changes;
        Task::future(sleep(LEVEL_SHOWN)).map(move |()| Message::HideLevel(change))
    }

    /// Changes the display's level by `delta` percent. The new level shows at once; the kernel's
    /// own reading follows and confirms it. Steps land on multiples of the step size, never below
    /// `min`.
    fn change(&mut self, delta: i64) -> Effect<Message> {
        let (step, min) = (i64::from(self.config.step), i64::from(self.config.min));
        let Some(display) = &mut self.display else {
            return Effect::none();
        };
        let current = i64::from(display.percent);
        let snapped = if delta > 0 {
            (current / step + 1) * step
        } else {
            (current + step - 1) / step * step - step
        };
        // Already below the floor (set elsewhere): scrolling down doesn't go further.
        let target = snapped.clamp(min.min(current), 100) as u32;
        if target == display.percent {
            return Effect::none();
        }
        display.percent = target;
        let send = self.send(Target::Display, target);
        Effect::task(Task::batch([send, self.show_level()]))
    }

    /// Sends `percent` for `target`, unless a level is already on its way: then it goes once that
    /// one is there, replacing any other still waiting.
    fn send(&mut self, target: Target, percent: u32) -> Task<Message> {
        if let Some((_, pending)) = self.sending.iter_mut().find(|(t, _)| *t == target) {
            *pending = Some(percent);
            return Task::none();
        }
        self.sending.push((target, None));
        self.set(target, percent)
    }

    fn set(&self, target: Target, percent: u32) -> Task<Message> {
        let level = match target {
            Target::Display => self.display.as_ref(),
            Target::Keyboard => self.keyboard.as_ref(),
        };
        let Some(light) = level.map(|level| level.light.clone()) else {
            return Task::done(Message::Sent(target));
        };
        let raw = curve::raw(percent, light.max);
        Task::future(async move { sysfs::set(&light, raw).await }).map(move |result| {
            if let Err(error) = result {
                eprintln!("eave-brightness: {error}");
            }
            Message::Sent(target)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysfs::Subsystem;

    const MAX: u32 = 96_000;

    fn backlight(percent: u32) -> Light {
        Light {
            subsystem: Subsystem::Backlight,
            name: "panel".into(),
            raw: curve::raw(percent, MAX),
            max: MAX,
        }
    }

    fn keyboard_light(raw: u32) -> Light {
        Light {
            subsystem: Subsystem::Leds,
            name: "kbd_backlight".into(),
            raw,
            max: 255,
        }
    }

    fn lights(percent: u32) -> Lights {
        Lights {
            backlights: vec![backlight(percent)],
            keyboard: None,
        }
    }

    fn brightness_at(percent: u32) -> Brightness {
        let (mut brightness, _) = Brightness::new(Config::default());
        brightness.update(Message::Read(lights(percent)));
        brightness
    }

    fn level(brightness: &Brightness) -> u32 {
        brightness.display.as_ref().unwrap().percent
    }

    fn nudge(effect: Effect<Message>) -> Option<Nudge> {
        effect.into_parts().2.map(|request| request.nudge)
    }

    fn shows_label(brightness: &Brightness) -> bool {
        let content = brightness.item().into_parts().content.unwrap();
        matches!(content.into_kind(), ui::Kind::Alternate { current: 1, .. })
    }

    #[test]
    fn options_parse() {
        let config: Config =
            toml::from_str("step = 10\nmin = 5\nkeyboard = false\ndevice = \"acpi_video0\"")
                .unwrap();
        assert_eq!(config.step, 10);
        assert_eq!(config.min, 5);
        assert!(!config.keyboard);
        assert_eq!(config.device.as_deref(), Some("acpi_video0"));
        assert!(toml::from_str::<Config>("mute = true").is_err());
    }

    #[test]
    fn steps_snap_and_respect_min() {
        let mut brightness = brightness_at(43);
        brightness.update(Message::Up);
        assert_eq!(level(&brightness), 45);
        brightness.update(Message::Up);
        assert_eq!(level(&brightness), 50);

        let mut brightness = brightness_at(100);
        brightness.update(Message::Up);
        assert_eq!(level(&brightness), 100);

        let mut brightness = brightness_at(5);
        brightness.update(Message::Down);
        assert_eq!(level(&brightness), 1, "down to min, not off");
        brightness.update(Message::Down);
        assert_eq!(level(&brightness), 1);

        // Already below the floor: scrolling down doesn't go further, up climbs back.
        let (mut brightness, _) = Brightness::new(Config {
            min: 10,
            ..Config::default()
        });
        brightness.update(Message::Read(lights(3)));
        brightness.update(Message::Down);
        assert_eq!(level(&brightness), 3);
        brightness.update(Message::Up);
        assert_eq!(level(&brightness), 5);
    }

    #[test]
    fn hidden_without_a_backlight() {
        let (mut brightness, _) = Brightness::new(Config::default());
        brightness.update(Message::Read(Lights::default()));
        assert!(brightness.item().into_parts().content.is_none());
        assert!(brightness.popover().is_none());
    }

    #[test]
    fn the_chosen_device_is_used() {
        let (mut brightness, _) = Brightness::new(Config {
            device: Some("second".into()),
            ..Config::default()
        });
        let mut second = backlight(30);
        second.name = "second".into();
        brightness.update(Message::Read(Lights {
            backlights: vec![backlight(80), second],
            keyboard: None,
        }));
        assert_eq!(brightness.display.as_ref().unwrap().light.name, "second");
        assert_eq!(level(&brightness), 30);
    }

    #[test]
    fn shows_a_sun_that_shows_the_level_on_hover() {
        let content = brightness_at(50).item().into_parts().content.unwrap();
        let ui::Kind::Alternate {
            mut options,
            current: 0,
            hovered: Some(1),
            grows: false,
        } = content.into_kind()
        else {
            panic!("the sun first");
        };
        assert!(matches!(
            options.remove(0).into_kind(),
            ui::Kind::Icon {
                icon: Icon::Brightness { level: 0.5 },
                ..
            }
        ));
        assert!(matches!(
            options.remove(0).into_kind(),
            ui::Kind::Counter { value: 50, .. }
        ));
    }

    #[test]
    fn readings_that_change_the_level_nudge() {
        let (mut brightness, _) = Brightness::new(Config::default());
        // The first reading is where it starts, not a change.
        assert_eq!(nudge(brightness.update(Message::Read(lights(40)))), None);
        // Scrolling shows the new level at once, and nudges when the kernel confirms it.
        assert_eq!(nudge(brightness.update(Message::Up)), None);
        brightness.update(Message::Sent(Target::Display));
        assert_eq!(
            nudge(brightness.update(Message::Read(lights(45)))),
            Some(Nudge::Up)
        );
        assert_eq!(nudge(brightness.update(Message::Read(lights(45)))), None);
        // A brightness key, elsewhere.
        assert_eq!(
            nudge(brightness.update(Message::Read(lights(30)))),
            Some(Nudge::Down)
        );
    }

    #[test]
    fn a_coarse_backlight_keeps_the_percent_it_was_set_to() {
        // 255 steps: 1 % and 3 % both land on raw 1.
        let coarse = |raw| Lights {
            backlights: vec![Light {
                raw,
                max: 255,
                ..backlight(0)
            }],
            keyboard: None,
        };
        let (mut brightness, _) = Brightness::new(Config::default());
        brightness.update(Message::Read(coarse(curve::raw(10, 255))));
        brightness.update(Message::Set(Target::Display, 3));
        brightness.update(Message::Sent(Target::Display));
        assert_eq!(
            nudge(brightness.update(Message::Read(coarse(1)))),
            Some(Nudge::Down)
        );
        assert_eq!(level(&brightness), 3, "not the 6 % raw 1 reads as");
    }

    #[test]
    fn a_change_shows_the_percent_for_a_moment() {
        let mut brightness = brightness_at(40);
        assert!(!shows_label(&brightness), "the sun, at first");
        brightness.update(Message::Read(lights(45)));
        assert!(shows_label(&brightness));
        // Another change before the first one's time is up keeps it showing.
        brightness.update(Message::Up);
        brightness.update(Message::HideLevel(1));
        assert!(shows_label(&brightness));
        brightness.update(Message::HideLevel(brightness.changes));
        assert!(!shows_label(&brightness));
    }

    #[test]
    fn a_drag_sends_one_level_at_a_time() {
        let mut brightness = brightness_at(50);
        brightness.update(Message::Set(Target::Display, 60));
        assert_eq!(level(&brightness), 60, "shown at once");
        assert_eq!(brightness.sending, [(Target::Display, None)]);
        brightness.update(Message::Set(Target::Display, 70));
        brightness.update(Message::Set(Target::Display, 80));
        assert_eq!(
            brightness.sending,
            [(Target::Display, Some(80))],
            "only the latest waits"
        );
        // A reading of an older level doesn't pull the slider back mid-drag.
        brightness.update(Message::Read(lights(60)));
        assert_eq!(level(&brightness), 80);
        brightness.update(Message::Sent(Target::Display));
        assert_eq!(
            brightness.sending,
            [(Target::Display, None)],
            "and goes next"
        );
        brightness.update(Message::Sent(Target::Display));
        assert!(brightness.sending.is_empty());
    }

    fn popover_lines(brightness: &Brightness) -> Vec<ui::Kind<Message>> {
        let ui::Kind::Column(children) = brightness.popover().unwrap().into_node().into_kind()
        else {
            panic!("a column");
        };
        children.into_iter().map(ui::Node::into_kind).collect()
    }

    #[test]
    fn keyboard_row_only_when_present_and_enabled() {
        let with_keyboard = Lights {
            keyboard: Some(keyboard_light(64)),
            ..lights(50)
        };
        let (mut brightness, _) = Brightness::new(Config::default());
        brightness.update(Message::Read(lights(50)));
        let lines = popover_lines(&brightness);
        assert!(matches!(lines[0], ui::Kind::Header { .. }));
        assert_eq!(lines.len(), 3, "header, display, its name");

        brightness.update(Message::Read(with_keyboard.clone()));
        let lines = popover_lines(&brightness);
        assert_eq!(lines.len(), 6);
        assert!(matches!(lines[3], ui::Kind::Separator));
        assert_eq!(brightness.keyboard.as_ref().unwrap().percent, 50);

        let (mut brightness, _) = Brightness::new(Config {
            keyboard: false,
            ..Config::default()
        });
        brightness.update(Message::Read(with_keyboard));
        assert_eq!(popover_lines(&brightness).len(), 3);
    }

    #[test]
    fn the_keyboard_is_polled_only_while_open() {
        let mut brightness = brightness_at(50);
        brightness.update(Message::Keyboard(Some(keyboard_light(255))));
        brightness.update(Message::Opened);
        assert!(brightness.open);
        assert_eq!(brightness.keyboard.as_ref().unwrap().percent, 100);
        brightness.update(Message::Closed);
        assert!(!brightness.open);
    }
}
