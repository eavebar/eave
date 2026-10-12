//! Socket2 events. Hyprland sends one `NAME>>DATA` line per event, with comma-separated fields.
//! The last field may itself contain commas (window titles), so it gets the rest of the line.

/// The events eave uses. Everything else on the socket is dropped while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The focused monitor's active workspace changed (`workspacev2`).
    Workspace {
        id: i64,
        name: String,
    },
    /// Focus moved to another monitor, and with it to that monitor's workspace (`focusedmonv2`).
    FocusedMonitor {
        monitor: String,
        workspace: i64,
    },
    CreateWorkspace {
        id: i64,
        name: String,
    },
    DestroyWorkspace {
        id: i64,
    },
    MoveWorkspace {
        id: i64,
        monitor: String,
    },
    RenameWorkspace {
        id: i64,
        name: String,
    },
    /// Both empty when no window has focus.
    ActiveWindow {
        class: String,
        title: String,
    },
    /// A keyboard switched layout. `layout` is the long name, like `English (US)`.
    ActiveLayout {
        keyboard: String,
        layout: String,
    },
    ConfigReloaded,
    /// Sent by the `event` dispatcher, e.g. from a bind (`custom>>DATA`).
    Custom(String),
}

impl Event {
    pub fn parse(line: &str) -> Option<Self> {
        let (name, data) = line.split_once(">>")?;
        let fields = |n| data.splitn(n, ',').collect::<Vec<_>>();
        let id = |field: &str| field.parse::<i64>().ok();

        Some(match name {
            "workspacev2" => match fields(2)[..] {
                [workspace, name] => Self::Workspace {
                    id: id(workspace)?,
                    name: name.into(),
                },
                _ => return None,
            },
            "focusedmonv2" => match fields(2)[..] {
                [monitor, workspace] => Self::FocusedMonitor {
                    monitor: monitor.into(),
                    workspace: id(workspace)?,
                },
                _ => return None,
            },
            "createworkspacev2" => match fields(2)[..] {
                [workspace, name] => Self::CreateWorkspace {
                    id: id(workspace)?,
                    name: name.into(),
                },
                _ => return None,
            },
            "destroyworkspacev2" => Self::DestroyWorkspace {
                id: id(fields(2)[0])?,
            },
            "moveworkspacev2" => match fields(3)[..] {
                [workspace, _name, monitor] => Self::MoveWorkspace {
                    id: id(workspace)?,
                    monitor: monitor.into(),
                },
                _ => return None,
            },
            "renameworkspace" => match fields(2)[..] {
                [workspace, name] => Self::RenameWorkspace {
                    id: id(workspace)?,
                    name: name.into(),
                },
                _ => return None,
            },
            "activewindow" => match fields(2)[..] {
                [class, title] => Self::ActiveWindow {
                    class: class.into(),
                    title: title.into(),
                },
                _ => return None,
            },
            "activelayout" => match fields(2)[..] {
                [keyboard, layout] => Self::ActiveLayout {
                    keyboard: keyboard.into(),
                    layout: layout.into(),
                },
                _ => return None,
            },
            "configreloaded" => Self::ConfigReloaded,
            "custom" => Self::Custom(data.into()),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_events() {
        assert_eq!(
            Event::parse("workspacev2>>3,3"),
            Some(Event::Workspace {
                id: 3,
                name: "3".into()
            })
        );
        assert_eq!(
            Event::parse("focusedmonv2>>eDP-1,2"),
            Some(Event::FocusedMonitor {
                monitor: "eDP-1".into(),
                workspace: 2
            })
        );
        assert_eq!(
            Event::parse("destroyworkspacev2>>4,4"),
            Some(Event::DestroyWorkspace { id: 4 })
        );
        assert_eq!(
            Event::parse("moveworkspacev2>>5,web,DP-2"),
            Some(Event::MoveWorkspace {
                id: 5,
                monitor: "DP-2".into()
            })
        );
        assert_eq!(
            Event::parse("createworkspacev2>>-98,special:scratch"),
            Some(Event::CreateWorkspace {
                id: -98,
                name: "special:scratch".into()
            })
        );
    }

    #[test]
    fn titles_keep_their_commas() {
        assert_eq!(
            Event::parse("activewindow>>firefox,Hello, world — Mozilla Firefox"),
            Some(Event::ActiveWindow {
                class: "firefox".into(),
                title: "Hello, world — Mozilla Firefox".into()
            })
        );
        assert_eq!(
            Event::parse("activewindow>>,"),
            Some(Event::ActiveWindow {
                class: String::new(),
                title: String::new()
            })
        );
    }

    #[test]
    fn ignores_unknown_and_malformed_lines() {
        assert_eq!(Event::parse("openwindow>>abc,1,kitty,kitty"), None);
        assert_eq!(Event::parse("workspacev2>>three,3"), None);
        assert_eq!(Event::parse("workspacev2>>3"), None);
        assert_eq!(Event::parse("garbage"), None);
        assert_eq!(
            Event::parse("configreloaded>>"),
            Some(Event::ConfigReloaded)
        );
        assert_eq!(
            Event::parse("custom>>eave-click"),
            Some(Event::Custom("eave-click".into()))
        );
    }
}
