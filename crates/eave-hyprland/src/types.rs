//! Replies to [`query`](crate::query), with only the fields eave uses. Missing fields default, so
//! older or newer Hyprland versions still parse.

use serde::Deserialize;

/// From `workspaces` and `activeworkspace`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Workspace {
    /// Negative for special workspaces (scratchpads).
    pub id: i64,
    pub name: String,
    pub monitor: String,
    pub windows: u32,
}

/// From `activewindow`. Hyprland answers `{}` when no window has focus, which parses as empty.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Window {
    pub class: String,
    pub title: String,
}

/// From `clients`: every window Hyprland manages.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Client {
    /// Like `0x5d1f2a0`. Events give the same address without the `0x`.
    pub address: String,
    pub workspace: WorkspaceRef,
    pub class: String,
    pub title: String,
}

/// A window's workspace, as `clients` names it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WorkspaceRef {
    pub id: i64,
    pub name: String,
}

/// From `devices`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Devices {
    pub keyboards: Vec<Keyboard>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Keyboard {
    pub name: String,
    /// Configured layouts, comma-separated, like `us,it`.
    pub layout: String,
    /// Each layout's variant, comma-separated in the same order, empty for the default one.
    pub variant: String,
    /// The active layout's long name, like `English (US)`.
    pub active_keymap: String,
    /// The active layout's place in `layout`.
    pub active_layout_index: Option<usize>,
    /// The keyboard that `switchxkblayout current` acts on.
    pub main: bool,
}

impl Devices {
    /// The main keyboard, or the first one if none is marked main.
    pub fn main_keyboard(self) -> Option<Keyboard> {
        let mut keyboards = self.keyboards;
        let index = keyboards.iter().position(|k| k.main).unwrap_or(0);
        (index < keyboards.len()).then(|| keyboards.swap_remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_replies() {
        let workspaces: Vec<Workspace> = serde_json::from_str(
            r#"[{"id":1,"name":"1","monitor":"eDP-1","monitorID":0,"windows":2,
                 "hasfullscreen":false,"lastwindow":"0x1","lastwindowtitle":"x","ispersistent":false}]"#,
        )
        .unwrap();
        assert_eq!((workspaces[0].id, workspaces[0].windows), (1, 2));

        let clients: Vec<Client> = serde_json::from_str(
            r#"[{"address":"0x5d1f2a0","mapped":true,"workspace":{"id":3,"name":"3"},"class":"kitty"}]"#,
        )
        .unwrap();
        assert_eq!(
            (clients[0].address.as_str(), clients[0].workspace.id),
            ("0x5d1f2a0", 3)
        );

        let none: Window = serde_json::from_str("{}").unwrap();
        assert!(none.title.is_empty());

        let devices: Devices = serde_json::from_str(
            r#"{"mice":[],"keyboards":[
                {"address":"0x1","name":"power-button","layout":"us","active_keymap":"English (US)","main":false},
                {"address":"0x2","name":"apple-spi-keyboard","layout":"us,it","active_keymap":"Italian","main":true}]}"#,
        )
        .unwrap();
        let main = devices.main_keyboard().unwrap();
        assert_eq!(
            (main.name.as_str(), main.active_keymap.as_str()),
            ("apple-spi-keyboard", "Italian")
        );
    }
}
