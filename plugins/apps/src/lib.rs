//! Installed apps, in the dormer: each one an action that launches it (D-84). It shows nothing in
//! the bar, so it goes in config.toml's `[[dormer]]`:
//!
//! ```toml
//! [[dormer]]
//! widget = "apps"
//! terminal = "foot"   # optional: what runs apps that want a terminal; without it, they're left out
//! ```
//!
//! The apps are read from their desktop entries when the bar starts, and again whenever one is
//! added, changed or removed. Hyprland starts them, so they don't stop with the bar.

mod desktop;

use std::os::unix::ffi::OsStrExt;

use eave_api::ui::{self, Action, Item};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use futures_util::{Stream, stream};
use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
use serde::Deserialize;

use crate::desktop::App;

pub fn register(registry: &mut Registry) {
    registry.widget::<Apps>("apps");
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// What runs an app that wants a terminal, before its command: `foot`, or `alacritty -e`.
    terminal: Option<String>,
}

pub struct Apps {
    terminal: Option<String>,
    apps: Vec<App>,
    /// The desktop entries are being read.
    scanning: bool,
    /// They changed again while being read: read them once more after.
    stale: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Scanned(Vec<App>),
    /// A desktop entry was added, changed or removed.
    Changed,
    /// Launch the app with this desktop file id.
    Launch(String),
}

impl Widget for Apps {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let apps = Self {
            terminal: config
                .terminal
                .filter(|terminal| !terminal.trim().is_empty()),
            apps: Vec::new(),
            scanning: true,
            stale: false,
        };
        (apps, scan())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Scanned(apps) => {
                self.apps = apps;
                self.scanning = std::mem::take(&mut self.stale);
                if self.scanning {
                    return Effect::task(scan());
                }
            }
            Message::Changed if self.scanning => self.stale = true,
            Message::Changed => {
                self.scanning = true;
                return Effect::task(scan());
            }
            Message::Launch(id) => {
                if let Some(command) = self.command(&id) {
                    return Effect::task(Task::future(launch(command)).discard());
                }
            }
        }
        Effect::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(changes).map(|()| Message::Changed)
    }

    fn item(&self) -> Item<Message> {
        Item::hidden()
    }

    fn actions(&self) -> Vec<Action<Message>> {
        self.apps
            .iter()
            .filter(|app| !app.terminal || self.terminal.is_some())
            .map(|app| {
                // Found by the program's name too: `code` for Visual Studio Code.
                let program = desktop::arguments(&app.exec)
                    .and_then(|arguments| arguments.into_iter().next())
                    .and_then(|program| Some(program.rsplit('/').next()?.to_owned()));
                ui::action(app.name.clone(), Message::Launch(app.id.clone()))
                    .subtitle(app.generic.clone().unwrap_or_else(|| "App".into()))
                    .keywords(app.keywords.iter().cloned().chain(program))
                    .leading(ui::app_icon(app.id.clone()))
            })
            .collect()
    }
}

impl Apps {
    /// The command line that launches the app `id`, if it has one that can run here.
    fn command(&self, id: &str) -> Option<String> {
        let app = self.apps.iter().find(|app| app.id == id)?;
        let arguments = desktop::arguments(&app.exec).filter(|arguments| !arguments.is_empty())?;
        let terminal = match (app.terminal, &self.terminal) {
            (true, None) => return None,
            (true, Some(terminal)) => Some(terminal.as_str()),
            (false, _) => None,
        };
        Some(desktop::command(&arguments, terminal))
    }
}

/// Reads every desktop entry, off the UI thread.
fn scan() -> Task<Message> {
    let read = async {
        tokio::task::spawn_blocking(|| desktop::scan(&desktop::data_dirs(), &desktop::desktops()))
            .await
            .unwrap_or_default()
    };
    Task::perform(read, Message::Scanned)
}

/// Has Hyprland run `command`.
async fn launch(command: String) {
    let quoted = command
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('\n', r"\n");
    let lua = format!("hl.dsp.exec_cmd(\"{quoted}\")");
    if let Err(error) = eave_hyprland::dispatch(&lua, &format!("exec {command}")).await {
        eprintln!("eave: apps: can't launch `{command}`: {error}");
    }
}

/// Something in an `applications` directory changed. Watched on a thread of its own, since
/// inotify reads block. Not their subdirectories: few apps install there.
fn changes() -> impl Stream<Item = ()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let spawned = std::thread::Builder::new()
        .name("eave-apps".into())
        .spawn(move || {
            if let Err(error) = watch(&sender) {
                eprintln!("eave: apps: not watching for new apps: {error}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("eave: apps: not watching for new apps: {error}");
    }
    stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|()| ((), receiver))
    })
}

/// Blocks, sending after every change, until nobody listens or there's nothing to watch.
fn watch(changed: &tokio::sync::mpsc::UnboundedSender<()>) -> rustix::io::Result<()> {
    const EVENTS: WatchFlags = WatchFlags::CLOSE_WRITE
        .union(WatchFlags::MOVED_TO)
        .union(WatchFlags::MOVED_FROM)
        .union(WatchFlags::DELETE)
        .union(WatchFlags::ONLYDIR);
    let fd = inotify::init(CreateFlags::CLOEXEC)?;
    let mut watching = false;
    for dir in desktop::data_dirs() {
        let applications = dir.join("applications");
        watching |= inotify::add_watch(&fd, applications.as_os_str().as_bytes(), EVENTS).is_ok();
    }
    if !watching {
        return Ok(());
    }
    let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
    let mut reader = inotify::Reader::new(&fd, &mut buffer);
    loop {
        reader.next()?;
        if changed.send(()).is_err() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps(terminal: Option<&str>) -> Apps {
        let (mut widget, _) = Apps::new(Config {
            terminal: terminal.map(str::to_owned),
        });
        let entry = |id: &str, name: &str, exec: &str, terminal| App {
            id: id.into(),
            name: name.into(),
            generic: None,
            keywords: vec!["editor".into()],
            exec: exec.into(),
            terminal,
        };
        let _ = widget.update(Message::Scanned(vec![
            entry("code", "Visual Studio Code", "/usr/bin/code %F", false),
            entry("htop", "htop", "htop", true),
        ]));
        widget
    }

    fn titles(widget: &Apps) -> Vec<String> {
        widget
            .actions()
            .into_iter()
            .map(|action| action.title().to_owned())
            .collect()
    }

    #[test]
    fn each_app_is_an_action_found_by_its_program_too() {
        let widget = apps(None);
        assert_eq!(titles(&widget), ["Visual Studio Code"]);
        let parts = widget.actions().remove(0).into_parts();
        assert_eq!(parts.keywords, ["editor", "code"]);
        assert!(parts.leading.is_some());
        assert!(matches!(parts.message, Message::Launch(id) if id == "code"));
    }

    #[test]
    fn terminal_apps_need_a_terminal() {
        assert_eq!(apps(None).command("htop"), None);
        let widget = apps(Some("foot"));
        assert_eq!(titles(&widget), ["Visual Studio Code", "htop"]);
        assert_eq!(widget.command("htop").as_deref(), Some("foot htop"));
        assert_eq!(widget.command("code").as_deref(), Some("/usr/bin/code"));
        assert_eq!(widget.command("nothing"), None);
    }

    #[test]
    fn changes_while_reading_are_read_again_once() {
        let mut widget = apps(None);
        let _ = widget.update(Message::Changed);
        assert!(widget.scanning);
        let _ = widget.update(Message::Changed);
        let _ = widget.update(Message::Changed);
        assert!(widget.stale);
        let _ = widget.update(Message::Scanned(Vec::new()));
        assert!(widget.scanning && !widget.stale);
        let _ = widget.update(Message::Scanned(Vec::new()));
        assert!(!widget.scanning);
    }

    #[test]
    fn takes_only_its_own_options() {
        let config: Result<Config, _> = toml::from_str("terminal = \"foot\"");
        assert!(config.is_ok());
        let config: Result<Config, _> = toml::from_str("shell = \"zsh\"");
        assert!(config.is_err());
    }
}
