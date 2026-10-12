//! Hyprland workspaces. Click one to go there, scroll to move between them. The host slides the
//! active mark from the old workspace to the new one.

use eave_api::ui::{self, Item, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use eave_hyprland::types::Workspace;
use eave_hyprland::{Event, Update};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Workspaces>("workspaces");
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Workspaces 1 to `persistent` are always shown, even when Hyprland has none open.
    persistent: u32,
}

pub struct Workspaces {
    persistent: u32,
    /// Sorted by id. Special workspaces (negative ids) are left out.
    shown: Vec<Shown>,
    /// What Hyprland has open, from which `shown` is derived.
    open: Vec<(i64, String)>,
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
            active: None,
        };
        workspaces.refresh();
        (workspaces, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Hyprland(Update::Snapshot(snapshot)) => {
                self.open = snapshot
                    .workspaces
                    .into_iter()
                    .map(|workspace| (workspace.id, workspace.name))
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
        Effect::none()
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
    let active: Workspace = eave_hyprland::query("activeworkspace").await?;
    Ok(Snapshot {
        workspaces,
        active: active.id,
    })
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
    fn follows_events() {
        let mut workspaces = bar(0);
        workspaces.update(Message::Hyprland(Update::Snapshot(Snapshot {
            workspaces: vec![
                Workspace {
                    id: 2,
                    name: "2".into(),
                    ..Workspace::default()
                },
                Workspace {
                    id: -98,
                    name: "special:scratch".into(),
                    ..Workspace::default()
                },
            ],
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
        event(
            &mut workspaces,
            Event::Workspace {
                id: 1,
                name: "1".into(),
            },
        );
        assert_eq!(ids(&workspaces), [1, 2]);
        assert_eq!(workspaces.active, Some(1));

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
