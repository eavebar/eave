//! Lock, log out, suspend, hibernate, reboot and shut down, and reboot into another system on a
//! dual boot machine (D-131). The power symbol in the bar, a popover of what this machine can do,
//! and every one of them in the dormer.
//!
//! ```toml
//! [[right]]
//! widget = "power"
//! # Which to show; all that this machine can do by default.
//! show = ["lock", "logout", "suspend", "hibernate", "reboot", "poweroff", "boot"]
//! # Systems to reboot into, besides the ones found on their own (see `boot`).
//! boot = [{ name = "Windows", efi = "Windows Boot Manager" }]
//! ```
//!
//! Everything goes through logind on the system bus, but log out, which is Hyprland's exit. Lock
//! asks logind to lock the session, which whatever listens for that does (hypridle's `lock_cmd`).
//! In the popover, log out, reboot and shut down ask to be pressed again; in the dormer, picking
//! one is asking already.

mod boot;
mod logind;

use eave_api::time::{Duration, sleep};
use eave_api::ui::{self, Action, Icon, Item, Node, Panel, Tone};
use eave_api::{Effect, Registry, Task, Widget};
use eave_dbus::zbus;
use serde::Deserialize;

use boot::{Configured, Target, Via};
use logind::Can;

/// How long an armed row waits for its second press.
const ARMED: Duration = Duration::from_secs(4);

pub fn register(registry: &mut Registry) {
    registry.widget::<Power>("power");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Which kinds to show, of those the machine can do.
    show: Vec<Kind>,
    /// Systems to reboot into, besides the boot loader's entries and macOS, which are found.
    boot: Vec<Configured>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show: Kind::ALL.to_vec(),
            boot: Vec::new(),
        }
    }
}

/// What `show` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Lock,
    Logout,
    Suspend,
    Hibernate,
    Reboot,
    Poweroff,
    /// Every reboot into another system.
    Boot,
}

impl Kind {
    const ALL: [Kind; 7] = [
        Kind::Lock,
        Kind::Logout,
        Kind::Suspend,
        Kind::Hibernate,
        Kind::Reboot,
        Kind::Poweroff,
        Kind::Boot,
    ];
}

/// One thing to do. `Boot` is an index into the targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Lock,
    LogOut,
    Suspend,
    Hibernate,
    Reboot,
    PowerOff,
    Boot(usize),
}

impl Choice {
    fn kind(self) -> Kind {
        match self {
            Choice::Lock => Kind::Lock,
            Choice::LogOut => Kind::Logout,
            Choice::Suspend => Kind::Suspend,
            Choice::Hibernate => Kind::Hibernate,
            Choice::Reboot => Kind::Reboot,
            Choice::PowerOff => Kind::Poweroff,
            Choice::Boot(_) => Kind::Boot,
        }
    }

    /// Whether a press in the popover only arms it: those that close everything you have open.
    fn asks_again(self) -> bool {
        !matches!(self, Choice::Lock | Choice::Suspend | Choice::Hibernate)
    }

    fn icon(self) -> Icon {
        match self {
            Choice::Lock => Icon::Lock,
            Choice::LogOut => Icon::LogOut,
            Choice::Suspend | Choice::Hibernate => Icon::Sleep,
            Choice::Reboot | Choice::Boot(_) => Icon::Refresh,
            Choice::PowerOff => Icon::Power,
        }
    }

    /// What it's doing, while it does.
    fn doing(self) -> &'static str {
        match self {
            Choice::Lock => "Locking…",
            Choice::LogOut => "Logging out…",
            Choice::Suspend => "Suspending…",
            Choice::Hibernate => "Hibernating…",
            Choice::Reboot | Choice::Boot(_) => "Rebooting…",
            Choice::PowerOff => "Shutting down…",
        }
    }

    /// Closes the popover when picked: the screen is about to be covered or go dark.
    fn closes(self) -> bool {
        matches!(self, Choice::Lock | Choice::Suspend | Choice::Hibernate)
    }
}

/// What logind said, and the systems found to reboot into.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    can: Can,
    targets: Vec<Target>,
}

pub struct Power {
    config: Config,
    found: Found,
    /// The row pressed once, and which arming it is, so an older one's timer leaves it be.
    armed: Option<(Choice, u64)>,
    armings: u64,
    /// The one under way.
    busy: Option<Choice>,
    /// The last that went wrong, and why.
    failed: Option<(Choice, String)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Found(Found),
    Opened,
    Closed,
    /// A row in the popover: arms it, or does it if it's armed or needs no asking.
    Press(Choice),
    /// Do it now: the dormer, `eave action`, a key.
    Run(Choice),
    /// An arming's time is up.
    Disarm(u64),
    Finished(Choice, Result<(), String>),
}

impl Widget for Power {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let task = find(config.boot.clone());
        let power = Self {
            config,
            found: Found::default(),
            armed: None,
            armings: 0,
            busy: None,
            failed: None,
        };
        (power, task)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Found(found) => {
                if found.targets != self.found.targets {
                    self.armed = None;
                }
                self.found = found;
                Effect::none()
            }
            Message::Opened => Effect::task(find(self.config.boot.clone())),
            Message::Closed => {
                self.armed = None;
                self.failed = None;
                Effect::none()
            }
            Message::Press(choice) if self.busy.is_some() || !self.offers(choice) => Effect::none(),
            Message::Press(choice) if choice.asks_again() && !self.is_armed(choice) => {
                self.armings += 1;
                let arming = self.armings;
                self.armed = Some((choice, arming));
                self.failed = None;
                Effect::task(Task::future(sleep(ARMED)).map(move |_| Message::Disarm(arming)))
            }
            Message::Press(choice) | Message::Run(choice) => self.run(choice),
            Message::Disarm(arming) => {
                if self.armed.is_some_and(|(_, armed)| armed == arming) {
                    self.armed = None;
                }
                Effect::none()
            }
            Message::Finished(choice, result) => {
                self.busy = None;
                if let Err(why) = result {
                    eprintln!("eave-power: {}: {why}", self.title(choice));
                    self.failed = Some((choice, why));
                }
                Effect::none()
            }
        }
    }

    fn item(&self) -> Item<Message> {
        if self.choices().is_empty() {
            return Item::hidden();
        }
        Item::new(ui::icon(Icon::Power))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let choices = self.choices();
        if choices.is_empty() {
            return None;
        }
        let rows = |filter: fn(&Choice) -> bool| -> Vec<Node<Message>> {
            choices
                .iter()
                .copied()
                .filter(filter)
                .map(|choice| self.row(choice))
                .collect()
        };
        let session =
            rows(|choice| matches!(choice, Choice::Lock | Choice::Suspend | Choice::Hibernate));
        let power =
            rows(|choice| matches!(choice, Choice::LogOut | Choice::Reboot | Choice::PowerOff));
        let systems = rows(|choice| matches!(choice, Choice::Boot(_)));
        let mut lines = vec![ui::header("Power")];
        for (index, group) in [session, power].into_iter().enumerate() {
            if group.is_empty() {
                continue;
            }
            if index > 0 && lines.len() > 1 {
                lines.push(ui::separator());
            }
            lines.push(ui::list(group));
        }
        if !systems.is_empty() {
            lines.push(ui::section("Reboot into"));
            lines.push(ui::list(systems));
        }
        let panel = Panel::new(ui::column(lines))
            .on_open(Message::Opened)
            .on_close(Message::Closed);
        Some(panel)
    }

    fn actions(&self) -> Vec<Action<Message>> {
        self.choices()
            .into_iter()
            .map(|choice| {
                ui::action(self.title(choice), Message::Run(choice))
                    .id(self.id(choice))
                    .keywords(self.keywords(choice))
                    .leading(ui::icon(choice.icon()))
            })
            .collect()
    }
}

impl Power {
    /// What to show, in order: what the machine can do and config.toml lets show.
    fn choices(&self) -> Vec<Choice> {
        let can = &self.found.can;
        let mut choices = vec![Choice::Lock];
        if can.suspend {
            choices.push(Choice::Suspend);
        }
        if can.hibernate {
            choices.push(Choice::Hibernate);
        }
        choices.push(Choice::LogOut);
        if can.reboot {
            choices.push(Choice::Reboot);
        }
        if can.power_off {
            choices.push(Choice::PowerOff);
        }
        if can.reboot {
            choices.extend((0..self.found.targets.len()).map(Choice::Boot));
        }
        choices.retain(|choice| self.config.show.contains(&choice.kind()));
        choices
    }

    fn offers(&self, choice: Choice) -> bool {
        self.choices().contains(&choice)
    }

    fn is_armed(&self, choice: Choice) -> bool {
        self.armed.is_some_and(|(armed, _)| armed == choice)
    }

    fn target(&self, choice: Choice) -> Option<&Target> {
        match choice {
            Choice::Boot(index) => self.found.targets.get(index),
            _ => None,
        }
    }

    /// Its name in the dormer, and in the popover but for systems, which are under "Reboot into".
    fn title(&self, choice: Choice) -> String {
        match choice {
            Choice::Lock => "Lock".into(),
            Choice::LogOut => "Log out".into(),
            Choice::Suspend => "Suspend".into(),
            Choice::Hibernate => "Hibernate".into(),
            Choice::Reboot => "Reboot".into(),
            Choice::PowerOff => "Shut down".into(),
            Choice::Boot(_) => match self.target(choice) {
                Some(target) => format!("Reboot to {}", target.name),
                None => "Reboot".into(),
            },
        }
    }

    fn id(&self, choice: Choice) -> String {
        match choice {
            Choice::Lock => "lock".into(),
            Choice::LogOut => "logout".into(),
            Choice::Suspend => "suspend".into(),
            Choice::Hibernate => "hibernate".into(),
            Choice::Reboot => "reboot".into(),
            Choice::PowerOff => "poweroff".into(),
            Choice::Boot(_) => {
                let name = self.target(choice).map_or("", |target| &target.name);
                format!("boot-{}", slug(name))
            }
        }
    }

    fn keywords(&self, choice: Choice) -> Vec<String> {
        let words: &[&str] = match choice {
            Choice::Lock => &["lock screen", "session", "away"],
            Choice::LogOut => &["logout", "sign out", "exit", "quit", "hyprland", "session"],
            Choice::Suspend => &["sleep", "standby"],
            Choice::Hibernate => &["sleep", "disk"],
            Choice::Reboot => &["restart"],
            Choice::PowerOff => &["shutdown", "power off", "turn off"],
            Choice::Boot(_) => &["restart", "boot", "dual boot", "other system"],
        };
        let mut words: Vec<String> = words.iter().map(|&word| word.to_owned()).collect();
        if let Some(target) = self.target(choice) {
            words.push(target.name.to_lowercase());
        }
        words
    }

    fn row(&self, choice: Choice) -> Node<Message> {
        let armed = self.is_armed(choice);
        let title = match (choice, self.target(choice)) {
            (_, _) if armed => format!("{} now?", self.title(choice)),
            (Choice::Boot(_), Some(target)) => target.name.clone(),
            _ => self.title(choice),
        };
        let failed = self
            .failed
            .as_ref()
            .filter(|(failed, _)| *failed == choice)
            .map(|(_, why)| why.clone());
        let subtitle = match (armed, self.busy == Some(choice), failed) {
            (true, _, _) => Some("Press again".to_owned()),
            (_, true, _) => Some(choice.doing().to_owned()),
            (_, _, Some(why)) => Some(why),
            _ => None,
        };
        let mut row = ui::list_item(title)
            .leading(ui::icon(choice.icon()))
            .on_press(Message::Press(choice));
        if let Some(subtitle) = subtitle {
            row = row.subtitle(subtitle);
        }
        let failed = self
            .failed
            .as_ref()
            .is_some_and(|(failed, _)| *failed == choice);
        match armed || failed {
            true => row.tone(Tone::Critical),
            false => row,
        }
    }

    fn run(&mut self, choice: Choice) -> Effect<Message> {
        if self.busy.is_some() || !self.offers(choice) {
            return Effect::none();
        }
        self.armed = None;
        self.failed = None;
        self.busy = Some(choice);
        let target = self.target(choice).cloned();
        let task = Task::future(perform(choice, target))
            .map(move |result| Message::Finished(choice, result));
        match choice.closes() {
            true => Effect::task(task).close_popover(),
            false => Effect::task(task),
        }
    }
}

/// Asks logind what it can do, and looks for systems to reboot into.
fn find(configured: Vec<Configured>) -> Task<Message> {
    Task::future(async move {
        let can = logind::can().await.unwrap_or_else(|error| {
            eprintln!("eave-power: can't ask logind: {error}");
            Can::default()
        });
        let running = boot::running_entry();
        let targets = boot::targets(
            &configured,
            &can.entries,
            running.as_deref(),
            boot::asahi(),
            |unit| async move {
                eave_dbus::systemd::system::unit(&unit)
                    .await
                    .is_ok_and(|unit| unit.exists())
            },
        )
        .await;
        Message::Found(Found { can, targets })
    })
}

async fn perform(choice: Choice, target: Option<Target>) -> Result<(), String> {
    let result = match choice {
        Choice::Lock => logind::lock().await,
        Choice::LogOut => {
            return eave_hyprland::dispatch("hl.dsp.exit()", "exit")
                .await
                .map_err(|error| error.to_string());
        }
        Choice::Suspend => logind::manager("Suspend").await,
        Choice::Hibernate => logind::manager("Hibernate").await,
        Choice::Reboot => logind::manager("Reboot").await,
        Choice::PowerOff => logind::manager("PowerOff").await,
        Choice::Boot(_) => {
            let Some(target) = target else {
                return Err("that system is gone".into());
            };
            match &target.via {
                Via::Entry(entry) => logind::boot_into(entry).await,
                Via::Unit(unit) => eave_dbus::systemd::system::start(unit).await,
            }
            .map_err(|error| format!("can't pick {}: {}", target.name, why(error)))?;
            logind::manager("Reboot").await
        }
    };
    result.map_err(why)
}

/// What went wrong, as the service put it.
fn why(error: zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(message), _) => message,
        error => error.to_string(),
    }
}

/// `name` as an action id: lowercase letters and digits, dashes between words.
fn slug(name: &str) -> String {
    let mut slug = String::new();
    for word in name.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_lowercase());
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    fn power(config: &str) -> Power {
        let config: Config = toml::from_str(config).unwrap();
        let mut power = Power::new(config).0;
        let can = Can {
            suspend: true,
            hibernate: false,
            reboot: true,
            power_off: true,
            entries: Vec::new(),
        };
        let targets = vec![Target {
            name: "macOS".into(),
            via: Via::Unit(boot::MACOS_UNIT.into()),
        }];
        let _ = power.update(Message::Found(Found { can, targets }));
        power
    }

    fn titles(power: &Power) -> Vec<String> {
        power
            .actions()
            .into_iter()
            .map(|action| action.into_parts().title)
            .collect()
    }

    #[test]
    fn shows_what_logind_can_do() {
        let power = power("");
        assert_eq!(
            titles(&power),
            [
                "Lock",
                "Suspend",
                "Log out",
                "Reboot",
                "Shut down",
                "Reboot to macOS"
            ]
        );
        let ids: Vec<_> = power
            .actions()
            .into_iter()
            .filter_map(|action| action.into_parts().id)
            .collect();
        assert_eq!(
            ids,
            [
                "lock",
                "suspend",
                "logout",
                "reboot",
                "poweroff",
                "boot-macos"
            ]
        );
    }

    #[test]
    fn show_keeps_to_what_it_names() {
        let power = power(r#"show = ["lock", "boot"]"#);
        assert_eq!(titles(&power), ["Lock", "Reboot to macOS"]);
    }

    #[test]
    fn nothing_to_reboot_into_without_reboot() {
        let mut power = power("");
        let _ = power.update(Message::Found(Found {
            can: Can::default(),
            targets: power.found.targets.clone(),
        }));
        assert_eq!(titles(&power), ["Lock", "Log out"]);
    }

    #[test]
    fn reboot_asks_to_be_pressed_again() {
        let mut power = power("");
        let _ = power.update(Message::Press(Choice::Reboot));
        assert!(power.is_armed(Choice::Reboot));
        assert_eq!(power.busy, None);
        let _ = power.update(Message::Press(Choice::Reboot));
        assert_eq!(power.armed, None);
        assert_eq!(power.busy, Some(Choice::Reboot));
    }

    #[test]
    fn lock_does_not_ask() {
        let mut power = power("");
        let _ = power.update(Message::Press(Choice::Lock));
        assert_eq!(power.busy, Some(Choice::Lock));
    }

    #[test]
    fn the_dormer_does_not_ask() {
        let mut power = power("");
        let _ = power.update(Message::Run(Choice::Boot(0)));
        assert_eq!(power.busy, Some(Choice::Boot(0)));
    }

    #[test]
    fn an_older_timer_leaves_a_newer_arming_be() {
        let mut power = power("");
        let _ = power.update(Message::Press(Choice::Reboot));
        let first = power.armings;
        let _ = power.update(Message::Press(Choice::PowerOff));
        let _ = power.update(Message::Disarm(first));
        assert!(power.is_armed(Choice::PowerOff));
        let _ = power.update(Message::Disarm(power.armings));
        assert_eq!(power.armed, None);
    }

    #[test]
    fn closing_the_popover_disarms() {
        let mut power = power("");
        let _ = power.update(Message::Press(Choice::PowerOff));
        let _ = power.update(Message::Closed);
        assert_eq!(power.armed, None);
        let _ = power.update(Message::Press(Choice::PowerOff));
        assert_eq!(power.busy, None, "armed again, not done");
    }

    #[test]
    fn a_failure_shows_until_closed() {
        let mut power = power("");
        let _ = power.update(Message::Run(Choice::Reboot));
        let _ = power.update(Message::Finished(
            Choice::Reboot,
            Err("Operation inhibited".into()),
        ));
        assert_eq!(power.busy, None);
        assert!(power.failed.is_some());
        let _ = power.update(Message::Closed);
        assert!(power.failed.is_none());
    }

    #[test]
    fn config_reads_boot_targets() {
        let config: Config = toml::from_str(
            r#"boot = [{ name = "Windows", efi = "Windows Boot Manager" }, { name = "Arch", grub = "Arch Linux" }]"#,
        )
        .unwrap();
        assert_eq!(config.boot.len(), 2);
        assert_eq!(config.boot[0].efi.as_deref(), Some("Windows Boot Manager"));
        assert!(toml::from_str::<Config>(r#"show = ["sleep"]"#).is_err());
    }

    #[test]
    fn slugs_are_ids() {
        assert_eq!(slug("macOS"), "macos");
        assert_eq!(slug("Windows 11 (Pro)"), "windows-11-pro");
    }
}
