//! Hyprland workspaces that have windows in them, plus the one you are on. Click one to go there, scroll to move between them. The host slides the
//! active mark from the old workspace to the new one, and the island gets a nudge the way it went.

use eave_api::ui::{self, Item, Tone};
use eave_api::{Effect, Nudge, Registry, Subscription, Task, Widget};
use std::collections::HashMap;

use eave_hyprland::types::{Client, Workspace};
use eave_hyprland::{Event, Update};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Workspaces>("workspaces");
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Workspaces 1 to `persistent` are always shown, even when empty or not open.
    persistent: u32,
}

pub struct Workspaces {
    persistent: u32,
    /// Sorted by id. Special workspaces (negative ids) are left out.
    shown: Vec<Shown>,
    /// What Hyprland has open, from which `shown` is derived.
    open: Vec<(i64, String)>,
    /// Every window's workspace, by address without the `0x`. Only workspaces in here, and the
    /// active one, are shown.
    windows: HashMap<String, i64>,
    active: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
struct Shown {
    id: i64,
    name: String,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    workspaces: Vec<Workspace>,
    clients: Vec<Client>,
    active: i64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Hyprland(Update<Snapshot>),
    Go(i64),
    Previous,
    Next,
}

impl Widget for Workspaces {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let mut workspaces = Self {
            persistent: config.persistent,
            shown: Vec::new(),
            open: Vec::new(),
            windows: HashMap::new(),
            active: None,
        };
        workspaces.refresh();
        (workspaces, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let before = self.active;
        match message {
            Message::Hyprland(Update::Snapshot(snapshot)) => {
                self.open = snapshot
                    .workspaces
                    .into_iter()
                    .map(|workspace| (workspace.id, workspace.name))
                    .collect();
                self.windows = snapshot
                    .clients
                    .into_iter()
                    .map(|client| (address(&client.address), client.workspace.id))
                    .collect();
                self.active = Some(snapshot.active);
            }
            Message::Hyprland(Update::Event(event)) => self.apply(event),
            Message::Go(id) => return focus(id.to_string()),
            // Relative to the monitor, over the workspaces it has open.
            Message::Previous => return focus("\"m-1\"".into()),
            Message::Next => return focus("\"m+1\"".into()),
        }
        self.refresh();
        // Ids follow the order workspaces are shown in, so a higher one is further right.
        match (before, self.active) {
            (Some(from), Some(to)) if to > from => Effect::none().nudge(Nudge::Right),
            (Some(from), Some(to)) if to < from => Effect::none().nudge(Nudge::Left),
            _ => Effect::none(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(|| eave_hyprland::watch(snapshot)).map(Message::Hyprland)
    }

    fn item(&self) -> Item<Message> {
        if self.shown.is_empty() {
            return Item::hidden();
        }
        let current = self.shown.iter().position(|w| Some(w.id) == self.active);
        let options = self.shown.iter().enumerate().map(|(index, workspace)| {
            let label = ui::label(workspace.name.clone());
            let label = if Some(index) == current {
                label.tone(Tone::Accent)
            } else {
                label
            };
            (label, Message::Go(workspace.id))
        });
        Item::new(ui::switcher(options, current)).on_scroll(Message::Previous, Message::Next)
    }
}

impl Workspaces {
    fn apply(&mut self, event: Event) {
        match event {
            Event::Workspace { id, name } => {
                self.active = Some(id);
                self.insert(id, name);
            }
            Event::FocusedMonitor { workspace, .. } => self.active = Some(workspace),
            Event::CreateWorkspace { id, name } => self.insert(id, name),
            Event::DestroyWorkspace { id } => self.open.retain(|(open, _)| *open != id),
            Event::RenameWorkspace { id, name } => {
                if let Some(open) = self.open.iter_mut().find(|(open, _)| *open == id) {
                    open.1 = name;
                }
            }
            Event::OpenWindow { address, workspace } => {
                let id = self
                    .open
                    .iter()
                    .find(|(_, name)| *name == workspace)
                    .map(|(id, _)| *id)
                    .or_else(|| workspace.parse().ok());
                if let Some(id) = id {
                    self.windows.insert(address, id);
                }
            }
            Event::CloseWindow { address } => {
                self.windows.remove(&address);
            }
            Event::MoveWindow { address, workspace } => {
                self.windows.insert(address, workspace);
            }
            _ => {}
        }
    }

    fn insert(&mut self, id: i64, name: String) {
        if !self.open.iter().any(|(open, _)| *open == id) {
            self.open.push((id, name));
        }
    }

    /// Rebuilds what the bar shows, so `item` only reads.
    fn refresh(&mut self) {
        let mut shown: Vec<Shown> = self
            .open
            .iter()
            .filter(|(id, _)| *id > 0)
            .filter(|(id, _)| Some(*id) == self.active || self.windows.values().any(|w| w == id))
            .map(|(id, name)| Shown {
                id: *id,
                name: name.clone(),
            })
            .collect();
        for id in 1..=i64::from(self.persistent) {
            if !shown.iter().any(|w| w.id == id) {
                shown.push(Shown {
                    id,
                    name: id.to_string(),
                });
            }
        }
        shown.sort_by_key(|w| w.id);
        self.shown = shown;
    }
}

async fn snapshot() -> std::io::Result<Snapshot> {
    let workspaces = eave_hyprland::query("workspaces").await?;
    let clients = eave_hyprland::query("clients").await?;
    let active: Workspace = eave_hyprland::query("activeworkspace").await?;
    Ok(Snapshot {
        workspaces,
        clients,
        active: active.id,
    })
}

/// `clients` gives addresses as `0x5d1f2a0`, events as `5d1f2a0`.
fn address(address: &str) -> String {
    address.trim_start_matches("0x").to_string()
}

/// Goes to `workspace`, a Lua value: a number, or a quoted relative target like `"m+1"`.
fn focus(workspace: String) -> Effect<Message> {
    Task::future(async move {
        let lua = format!("hl.dsp.focus({{ workspace = {workspace} }})");
        let legacy = format!("workspace {}", workspace.trim_matches('"'));
        if let Err(error) = eave_hyprland::dispatch(&lua, &legacy).await {
            eprintln!("eave-workspaces: {error}");
        }
    })
    .discard()
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(persistent: u32) -> Workspaces {
        Workspaces::new(Config { persistent }).0
    }

    fn ids(workspaces: &Workspaces) -> Vec<i64> {
        workspaces.shown.iter().map(|w| w.id).collect()
    }

    fn event(workspaces: &mut Workspaces, event: Event) {
        workspaces.update(Message::Hyprland(Update::Event(event)));
    }

    #[test]
    fn switching_nudges_the_way_it_went() {
        let mut workspaces = bar(0);
        let mut go = |id: i64| {
            let event = Event::Workspace {
                id,
                name: id.to_string(),
            };
            workspaces
                .update(Message::Hyprland(Update::Event(event)))
                .into_parts()
                .2
        };
        // The first one is where it starts, not a move.
        assert_eq!(go(1), None);
        assert_eq!(go(3), Some(Nudge::Right));
        assert_eq!(go(3), None);
        assert_eq!(go(2), Some(Nudge::Left));
    }

    fn workspace(id: i64, name: &str) -> Workspace {
        Workspace {
            id,
            name: name.into(),
            ..Workspace::default()
        }
    }

    fn client(address: &str, id: i64) -> Client {
        Client {
            address: address.into(),
            workspace: eave_hyprland::types::WorkspaceRef {
                id,
                name: id.to_string(),
            },
        }
    }

    #[test]
    fn follows_events() {
        let mut workspaces = bar(0);
        workspaces.update(Message::Hyprland(Update::Snapshot(Snapshot {
            workspaces: vec![workspace(2, "2"), workspace(-98, "special:scratch")],
            clients: vec![client("0xa", 2), client("0xb", -98)],
            active: 2,
        })));
        assert_eq!(ids(&workspaces), [2]);

        event(
            &mut workspaces,
            Event::CreateWorkspace {
                id: 1,
                name: "1".into(),
            },
        );
        assert_eq!(ids(&workspaces), [2], "empty workspaces stay hidden");
        event(
            &mut workspaces,
            Event::Workspace {
                id: 1,
                name: "1".into(),
            },
        );
        assert_eq!(workspaces.active, Some(1));
        assert_eq!(ids(&workspaces), [1, 2], "unless they are active");

        event(
            &mut workspaces,
            Event::OpenWindow {
                address: "c".into(),
                workspace: "1".into(),
            },
        );
        assert_eq!(ids(&workspaces), [1, 2]);

        event(&mut workspaces, Event::DestroyWorkspace { id: 2 });
        event(
            &mut workspaces,
            Event::RenameWorkspace {
                id: 1,
                name: "web".into(),
            },
        );
        assert_eq!(
            workspaces.shown,
            [Shown {
                id: 1,
                name: "web".into()
            }]
        );
    }

    #[test]
    fn hides_workspaces_once_their_windows_leave() {
        let mut workspaces = bar(0);
        workspaces.update(Message::Hyprland(Update::Snapshot(Snapshot {
            workspaces: vec![workspace(1, "1"), workspace(3, "web")],
            clients: vec![client("0xa", 1), client("0xb", 3)],
            active: 1,
        })));
        assert_eq!(ids(&workspaces), [1, 3]);

        event(
            &mut workspaces,
            Event::MoveWindow {
                address: "b".into(),
                workspace: 1,
            },
        );
        assert_eq!(ids(&workspaces), [1]);

        event(
            &mut workspaces,
            Event::OpenWindow {
                address: "c".into(),
                workspace: "web".into(),
            },
        );
        assert_eq!(ids(&workspaces), [1, 3]);

        event(
            &mut workspaces,
            Event::CloseWindow {
                address: "c".into(),
            },
        );
        assert_eq!(ids(&workspaces), [1]);
    }

    #[test]
    fn persistent_workspaces_fill_the_gaps() {
        let mut workspaces = bar(3);
        assert_eq!(ids(&workspaces), [1, 2, 3]);
        event(
            &mut workspaces,
            Event::CreateWorkspace {
                id: 7,
                name: "7".into(),
            },
        );
        assert_eq!(ids(&workspaces), [1, 2, 3]);
        event(
            &mut workspaces,
            Event::OpenWindow {
                address: "a".into(),
                workspace: "7".into(),
            },
        );
        assert_eq!(ids(&workspaces), [1, 2, 3, 7]);
    }

    #[test]
    fn hidden_until_hyprland_answers() {
        assert!(bar(0).item().into_parts().content.is_none());
    }

    #[test]
    fn rejects_unknown_options() {
        let mut table = toml::Table::new();
        table.insert("persistant".into(), 5.into());
        assert!(table.try_into::<Config>().is_err());
    }
}
