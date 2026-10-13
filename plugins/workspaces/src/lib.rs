//! Hyprland workspaces that have windows in them, plus the one you are on. Each shows its name
//! and the icons of its apps: two at most, then `+n` for the windows they leave out. Resting the
//! pointer on one shows an icon for every window. The ones you aren't on are dimmed. Click one to
//! go there, right-click it for a list of its windows, scroll to move between them. Switching
//! nudges the island the way it went, over as long as Hyprland's own workspace animation takes.

use std::time::Duration;

use eave_api::ui::{self, Action, Item, Node, Panel, Tone};
use eave_api::{Effect, Nudge, Registry, Subscription, Task, Widget};

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
    /// Every window, oldest first. Only workspaces with one, and the active one, are shown.
    windows: Vec<Window>,
    active: Option<i64>,
    /// The focused window's address, without the `0x`.
    focused: Option<String>,
    /// The workspace whose windows the popover lists.
    listing: Option<i64>,
    /// How long Hyprland takes to animate a workspace switch, if it does.
    lasting: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
struct Window {
    /// Without the `0x`.
    address: String,
    workspace: i64,
    class: String,
    title: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Shown {
    id: i64,
    name: String,
    /// The class of each of its windows, oldest first.
    classes: Vec<String>,
}

/// Icons a workspace shows before the rest go into `+n`.
const MAX_ICONS: usize = 2;

#[derive(Debug, Clone)]
pub struct Snapshot {
    workspaces: Vec<Workspace>,
    clients: Vec<Client>,
    active: i64,
    lasting: Option<Duration>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Hyprland(Update<Snapshot>),
    Go(i64),
    Previous,
    Next,
    /// List a workspace's windows in the popover.
    List(i64),
    /// Focus a window, by address without the `0x`.
    Focus(String),
    /// How long Hyprland's workspace animation now takes.
    Lasting(Option<Duration>),
}

impl Widget for Workspaces {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let mut workspaces = Self {
            persistent: config.persistent,
            shown: Vec::new(),
            open: Vec::new(),
            windows: Vec::new(),
            active: None,
            focused: None,
            listing: None,
            lasting: None,
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
                self.focused = snapshot
                    .clients
                    .iter()
                    .find(|client| client.focus_history_id == Some(0))
                    .map(|client| address(&client.address));
                self.windows = snapshot
                    .clients
                    .into_iter()
                    .map(|client| Window {
                        address: address(&client.address),
                        workspace: client.workspace.id,
                        class: client.class,
                        title: client.title,
                    })
                    .collect();
                self.active = Some(snapshot.active);
                self.lasting = snapshot.lasting;
            }
            // Its animations may have changed.
            Message::Hyprland(Update::Event(Event::ConfigReloaded)) => {
                return Task::future(lasting()).map(Message::Lasting).into();
            }
            Message::Hyprland(Update::Event(event)) => self.apply(event),
            Message::Lasting(lasting) => self.lasting = lasting,
            Message::Go(id) => return focus(id.to_string()),
            // Relative to the monitor, over the workspaces it has open.
            Message::Previous => return focus("\"m-1\"".into()),
            Message::Next => return focus("\"m+1\"".into()),
            Message::List(id) => {
                self.listing = Some(id);
                return Effect::none().open_popover();
            }
            Message::Focus(address) => return focus_window(address).close_popover(),
        }
        self.refresh();
        // Ids follow the order workspaces are shown in, so a higher one is further right.
        let nudge = match (before, self.active) {
            (Some(from), Some(to)) if to > from => Nudge::Right,
            (Some(from), Some(to)) if to < from => Nudge::Left,
            _ => return Effect::none(),
        };
        // In step with Hyprland sliding the workspace in.
        match self.lasting {
            Some(lasting) => Effect::none().nudge_lasting(nudge, lasting),
            None => Effect::none().nudge(nudge),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(|| eave_hyprland::watch(snapshot)).map(Message::Hyprland)
    }

    fn item(&self) -> Item<Message> {
        if self.shown.is_empty() {
            return Item::hidden();
        }
        let buttons = self.shown.iter().map(|workspace| {
            let (collapsed, expanded) = workspace.views();
            let content = match expanded {
                Some(expanded) => ui::expand(collapsed, expanded),
                None => collapsed,
            };
            let button = ui::button(content, Message::Go(workspace.id))
                .on_secondary(Message::List(workspace.id));
            if Some(workspace.id) == self.active {
                button
            } else {
                button.tone(Tone::Dim)
            }
        });
        Item::new(ui::row(buttons)).on_scroll(Message::Previous, Message::Next)
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let id = self.listing?;
        let workspace = self.shown.iter().find(|workspace| workspace.id == id)?;
        let rows: Vec<_> = self
            .windows
            .iter()
            .filter(|window| window.workspace == id)
            .map(|window| {
                let title = if window.title.is_empty() {
                    &window.class
                } else {
                    &window.title
                };
                ui::list_item(title.clone())
                    .leading(ui::app_icon(window.class.clone()))
                    .subtitle(window.class.clone())
                    .selected(self.focused.as_ref() == Some(&window.address))
                    .on_press(Message::Focus(window.address.clone()))
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        let count = match rows.len() {
            1 => "1 window".to_string(),
            n => format!("{n} windows"),
        };
        let header = ui::header(format!("Workspace {}", workspace.name))
            .trailing(ui::label(count).tone(Tone::Dim));
        Some(Panel::new(ui::column([header, ui::list(rows)])))
    }

    /// Every window to switch to, then every workspace to go to, then the ones on either side.
    fn actions(&self) -> Vec<Action<Message>> {
        let mut actions: Vec<_> = self
            .windows
            .iter()
            .filter(|window| self.focused.as_ref() != Some(&window.address))
            .map(|window| {
                let title = if window.title.is_empty() {
                    &window.class
                } else {
                    &window.title
                };
                let workspace = self
                    .open
                    .iter()
                    .find(|(id, _)| *id == window.workspace)
                    .map_or_else(|| window.workspace.to_string(), |(_, name)| name.clone());
                ui::action(title.clone(), Message::Focus(window.address.clone()))
                    .subtitle(format!("Window on workspace {workspace}"))
                    .keywords([window.class.clone(), "window".into(), "switch".into()])
                    .leading(ui::app_icon(window.class.clone()))
            })
            .collect();
        actions.extend(
            self.shown
                .iter()
                .filter(|workspace| Some(workspace.id) != self.active)
                .map(|workspace| {
                    ui::action(
                        format!("Workspace {}", workspace.name),
                        Message::Go(workspace.id),
                    )
                    .keywords(["go", "switch"])
                }),
        );
        actions.extend([
            ui::action("Previous workspace", Message::Previous).keywords(["left", "back"]),
            ui::action("Next workspace", Message::Next).keywords(["right", "forward"]),
        ]);
        actions
    }
}

impl Shown {
    /// What it shows normally, and what it shows when the pointer rests on it, if that's more:
    /// the name and one icon per app, two at most, with `+n` for the windows they leave out; and
    /// the name and one icon per window.
    fn views(&self) -> (Node<Message>, Option<Node<Message>>) {
        let name = || ui::label(self.name.clone());
        let mut apps: Vec<&String> = Vec::new();
        for class in &self.classes {
            if !apps.contains(&class) {
                apps.push(class);
            }
        }
        apps.truncate(MAX_ICONS);
        let hidden = self.classes.len() - apps.len();
        let mut collapsed = vec![name()];
        collapsed.extend(apps.into_iter().map(|class| ui::app_icon(class.clone())));
        if hidden > 0 {
            collapsed.push(ui::label(format!("+{hidden}")));
        }
        let expanded = (hidden > 0).then(|| {
            ui::row(
                std::iter::once(name())
                    .chain(self.classes.iter().map(|class| ui::app_icon(class.clone()))),
            )
        });
        (ui::row(collapsed), expanded)
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
            Event::OpenWindow {
                address,
                workspace,
                class,
                title,
            } => {
                let id = self
                    .open
                    .iter()
                    .find(|(_, name)| *name == workspace)
                    .map(|(id, _)| *id)
                    .or_else(|| workspace.parse().ok());
                if let Some(id) = id {
                    self.windows.retain(|window| window.address != address);
                    self.windows.push(Window {
                        address,
                        workspace: id,
                        class,
                        title,
                    });
                }
            }
            Event::CloseWindow { address } => {
                self.windows.retain(|window| window.address != address);
            }
            Event::MoveWindow { address, workspace } => {
                if let Some(window) = self.window(&address) {
                    window.workspace = workspace;
                }
            }
            Event::WindowTitle { address, title } => {
                if let Some(window) = self.window(&address) {
                    window.title = title;
                }
            }
            Event::FocusedWindow { address } => {
                self.focused = Some(address).filter(|address| !address.is_empty());
            }
            _ => {}
        }
    }

    fn window(&mut self, address: &str) -> Option<&mut Window> {
        self.windows
            .iter_mut()
            .find(|window| window.address == address)
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
                classes: self.classes(*id),
            })
            .filter(|w| Some(w.id) == self.active || !w.classes.is_empty())
            .collect();
        for id in 1..=i64::from(self.persistent) {
            if !shown.iter().any(|w| w.id == id) {
                shown.push(Shown {
                    id,
                    name: id.to_string(),
                    classes: self.classes(id),
                });
            }
        }
        shown.sort_by_key(|w| w.id);
        self.shown = shown;
    }

    fn classes(&self, workspace: i64) -> Vec<String> {
        self.windows
            .iter()
            .filter(|window| window.workspace == workspace)
            .map(|window| window.class.clone())
            .collect()
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
        lasting: lasting().await,
    })
}

/// How long Hyprland takes to slide a workspace in. Without it, nudges keep their own pace.
async fn lasting() -> Option<Duration> {
    eave_hyprland::animation("workspacesIn")
        .await
        .unwrap_or_else(|error| {
            eprintln!("eave-workspaces: {error}");
            None
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

/// Focuses the window at `address`, which takes you to its workspace.
fn focus_window(address: String) -> Effect<Message> {
    Task::future(async move {
        let lua = format!("hl.dsp.focus({{ window = \"address:0x{address}\" }})");
        let legacy = format!("focuswindow address:0x{address}");
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

    #[test]
    fn its_actions_switch_windows_and_workspaces() {
        let mut workspaces = bar(0);
        workspaces.open = vec![(1, "1".into()), (2, "web".into())];
        workspaces.shown = vec![
            Shown {
                id: 1,
                name: "1".into(),
                classes: Vec::new(),
            },
            Shown {
                id: 2,
                name: "web".into(),
                classes: Vec::new(),
            },
        ];
        workspaces.active = Some(1);
        let window = |address: &str, workspace, title: &str| Window {
            address: address.into(),
            workspace,
            class: "firefox".into(),
            title: title.into(),
        };
        workspaces.windows = vec![window("a", 1, "Here"), window("b", 2, "")];
        workspaces.focused = Some("a".into());
        let actions: Vec<_> = workspaces
            .actions()
            .into_iter()
            .map(|action| action.into_parts())
            .map(|parts| (parts.title, parts.subtitle))
            .collect();
        assert_eq!(
            actions,
            [
                ("firefox".into(), Some("Window on workspace web".into())),
                ("Workspace web".into(), None),
                ("Previous workspace".into(), None),
                ("Next workspace".into(), None),
            ]
        );
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
                .map(|request| request.nudge)
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
            class: "kitty".into(),
            title: String::new(),
            focus_history_id: None,
        }
    }

    fn open(address: &str, workspace: &str, class: &str) -> Event {
        Event::OpenWindow {
            address: address.into(),
            workspace: workspace.into(),
            class: class.into(),
            title: format!("{class} {address}"),
        }
    }

    #[test]
    fn follows_events() {
        let mut workspaces = bar(0);
        workspaces.update(Message::Hyprland(Update::Snapshot(Snapshot {
            workspaces: vec![workspace(2, "2"), workspace(-98, "special:scratch")],
            clients: vec![client("0xa", 2), client("0xb", -98)],
            active: 2,
            lasting: None,
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

        event(&mut workspaces, open("c", "1", "kitty"));
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
                name: "web".into(),
                classes: vec!["kitty".into()],
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
            lasting: None,
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

        event(&mut workspaces, open("c", "web", "kitty"));
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
        event(&mut workspaces, open("a", "7", "kitty"));
        assert_eq!(ids(&workspaces), [1, 2, 3, 7]);
    }

    /// What a workspace shows: labels as text, app icons as `[class]`.
    fn describe(node: Node<Message>) -> String {
        match node.into_kind() {
            ui::Kind::Label { text, .. } => text,
            ui::Kind::AppIcon { app_id, .. } => format!("[{app_id}]"),
            ui::Kind::Row(children) => children
                .into_iter()
                .map(describe)
                .collect::<Vec<_>>()
                .join(" "),
            _ => "?".into(),
        }
    }

    fn views(workspace: &Shown) -> (String, Option<String>) {
        let (collapsed, expanded) = workspace.views();
        (describe(collapsed), expanded.map(describe))
    }

    #[test]
    fn two_icons_then_the_windows_they_leave_out() {
        let mut workspaces = bar(3);
        for (address, class) in [
            ("a", "kitty"),
            ("b", "firefox"),
            ("c", "kitty"),
            ("d", "slack"),
            ("e", "kitty"),
        ] {
            event(&mut workspaces, open(address, "1", class));
        }
        event(&mut workspaces, open("f", "2", "kitty"));
        event(&mut workspaces, open("g", "3", "foot"));
        event(&mut workspaces, open("h", "3", "foot"));

        assert_eq!(
            views(&workspaces.shown[0]),
            (
                "1 [kitty] [firefox] +3".into(),
                Some("1 [kitty] [firefox] [kitty] [slack] [kitty]".into())
            )
        );
        assert_eq!(views(&workspaces.shown[1]), ("2 [kitty]".into(), None));
        assert_eq!(
            views(&workspaces.shown[2]),
            ("3 [foot] +1".into(), Some("3 [foot] [foot]".into()))
        );
    }

    #[test]
    fn lists_windows_with_live_titles() {
        let mut workspaces = bar(2);
        event(&mut workspaces, open("a", "1", "kitty"));
        event(&mut workspaces, open("b", "2", "firefox"));
        assert!(workspaces.popover().is_none(), "nothing listed yet");

        let effect = workspaces.update(Message::List(1));
        assert_eq!(
            effect.into_parts().1,
            Some(eave_api::erased::PopoverRequest::Open)
        );
        event(
            &mut workspaces,
            Event::WindowTitle {
                address: "a".into(),
                title: "~/code, eave".into(),
            },
        );
        let titles: Vec<_> = workspaces
            .windows
            .iter()
            .map(|w| w.title.as_str())
            .collect();
        assert_eq!(titles, ["~/code, eave", "firefox b"]);
        assert!(workspaces.popover().is_some());

        event(
            &mut workspaces,
            Event::CloseWindow {
                address: "a".into(),
            },
        );
        assert!(workspaces.popover().is_none(), "closes once it's empty");
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
