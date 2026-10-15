//! Shortcuts (D-91): finding the keys a popover's rows name ([`eave_api::ui::Node::shortcut`]) and
//! the ones the dormer's actions name, giving each the key the user set in config.toml or else the
//! plugin's, telling whether a key press is one, and the labels on their keycaps.

use std::collections::BTreeMap;

use eave_api::ui::{Binding, Key, Kind, Node, Shortcut};
use iced::keyboard::{self, key::Named};

/// A widget entry's `keys` in config.toml: a shortcut's name and the key the user gives it, or
/// `None` for none.
pub type Keys = BTreeMap<String, Option<Shortcut>>;

/// Every row and button in `node` with a shortcut and something to press, in order. Those inside
/// tabs are left out: only one page shows, and a key shouldn't press what's hidden.
pub fn collect<M: Clone>(node: &Node<M>, out: &mut Vec<(Shortcut, M)>) {
    match node.kind() {
        Kind::ListItem {
            shortcut:
                Some(Binding {
                    key: Some(shortcut),
                    ..
                }),
            on_press: Some(message),
            ..
        }
        | Kind::Button {
            shortcut:
                Some(Binding {
                    key: Some(shortcut),
                    ..
                }),
            on_press: message,
            ..
        } => out.push((*shortcut, message.clone())),
        Kind::Header {
            leading, trailing, ..
        } => leading
            .iter()
            .chain(trailing)
            .for_each(|child| collect(child, out)),
        Kind::Row(children)
        | Kind::Column(children)
        | Kind::List(children)
        | Kind::Grid {
            cells: children, ..
        } => children.iter().for_each(|child| collect(child, out)),
        Kind::Center(content)
        | Kind::Page { content, .. }
        | Kind::Display(content)
        | Kind::Scroll { content, .. } => collect(content, out),
        _ => {}
    }
}

/// The same rows as [`collect`], to change their keys, with what they send.
fn bindings<'a, M>(node: &'a mut Node<M>, out: &mut Vec<(&'a mut Binding, &'a M)>) {
    match node.kind_mut() {
        Kind::ListItem {
            shortcut: Some(binding),
            on_press: Some(message),
            ..
        }
        | Kind::Button {
            shortcut: Some(binding),
            on_press: message,
            ..
        } => out.push((binding, message)),
        Kind::Header {
            leading, trailing, ..
        } => leading
            .iter_mut()
            .chain(trailing)
            .for_each(|child| bindings(child, out)),
        Kind::Row(children)
        | Kind::Column(children)
        | Kind::List(children)
        | Kind::Grid {
            cells: children, ..
        } => children.iter_mut().for_each(|child| bindings(child, out)),
        Kind::Center(content)
        | Kind::Page { content, .. }
        | Kind::Display(content)
        | Kind::Scroll { content, .. } => bindings(content, out),
        _ => {}
    }
}

/// Gives the rows and buttons of a popover's panel their keys: the user's where they set one,
/// else the plugin's. Where two want the same key, the user's wins, then the first. A row that
/// loses has no key, so it shows none either. `keys` says whose `keys` a row's are by what it
/// sends: a panel can hold parts of other widgets (D-109), each with its own.
pub fn resolve_panel<'k, M>(node: &mut Node<M>, keys: &dyn Fn(&M) -> &'k Keys) {
    let mut rows = Vec::new();
    bindings(node, &mut rows);
    let wanted: Vec<_> = rows
        .iter()
        .map(|(row, message)| wanted(&row.id, row.key, keys(message)))
        .collect();
    for ((row, _), (key, _)) in rows.into_iter().zip(settle(&wanted)) {
        row.key = key;
    }
}

/// An action that may have a key in the dormer: its name, the plugin's key for it, and its
/// widget's `keys`.
pub struct Candidate<'a> {
    pub id: Option<&'a str>,
    pub default: Option<Shortcut>,
    pub keys: &'a Keys,
    /// Its widget's name, for the warnings.
    pub widget: &'a str,
}

/// Gives the dormer's actions, in config order, their keys. Like [`resolve_panel`], across every
/// widget: the user's keys come first, then the plugins', each in config order. A key with no
/// Ctrl, Alt or Super is dropped, since the dormer's field takes those, and so is one on an
/// action with no name. Also says what was dropped, and why.
pub fn resolve_actions(candidates: &[Candidate]) -> (Vec<Option<Shortcut>>, Vec<String>) {
    let mut warnings = Vec::new();
    let wanted: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            let Some(id) = candidate.id else {
                if let Some(key) = candidate.default {
                    warnings.push(format!(
                        "{}: an action without an id can't have a key ({}), dropping it",
                        candidate.widget,
                        text(&key)
                    ));
                }
                return None;
            };
            let (key, user) = wanted(id, candidate.default, candidate.keys)?;
            if types(&key) {
                warnings.push(format!(
                    "{}: `{id}` can't be {} in the dormer, it types into the search; \
                     add Ctrl, Alt or Super",
                    candidate.widget,
                    text(&key)
                ));
                return None;
            }
            Some((key, user))
        })
        .collect();
    let settled = settle(&wanted);
    for (index, (key, lost_to)) in settled.iter().enumerate() {
        if let (None, Some(winner)) = (key, lost_to) {
            let (loser, winner) = (&candidates[index], &candidates[*winner]);
            warnings.push(format!(
                "{}: `{}` loses {} in the dormer to {}'s `{}`",
                loser.widget,
                loser.id.unwrap_or_default(),
                wanted[index].map(|(key, _)| text(&key)).unwrap_or_default(),
                winner.widget,
                winner.id.unwrap_or_default(),
            ));
        }
    }
    (settled.into_iter().map(|(key, _)| key).collect(), warnings)
}

/// The key `id` wants: the user's, if `keys` names it, else `default`. True when it's the user's.
fn wanted(id: &str, default: Option<Shortcut>, keys: &Keys) -> Option<(Shortcut, bool)> {
    match keys.get(id) {
        Some(user) => user.map(|key| (key, true)),
        None => default.map(|key| (key, false)),
    }
}

/// Who gets each key: the user's over a plugin's, then the first. For each, the key it gets, or
/// the place of the one that took it.
fn settle(wanted: &[Option<(Shortcut, bool)>]) -> Vec<(Option<Shortcut>, Option<usize>)> {
    wanted
        .iter()
        .enumerate()
        .map(|(index, want)| {
            let Some((key, user)) = *want else {
                return (None, None);
            };
            let winner = wanted.iter().enumerate().position(|(other, theirs)| {
                theirs.is_some_and(|(theirs, their_user)| {
                    theirs == key
                        && other != index
                        && (their_user && !user || their_user == user && other < index)
                })
            });
            match winner {
                Some(winner) => (None, Some(winner)),
                None => (Some(key), None),
            }
        })
        .collect()
}

/// A shortcut as config.toml writes it, `ctrl+shift+k`: modifiers (`ctrl`, `alt`, `shift`,
/// `super`) then one key (a character, `enter`, `space`, `tab`, `up`, `down`, `left`, `right`),
/// joined by `+`, in any case.
pub fn parse(text: &str) -> Result<Shortcut, String> {
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    // `ctrl++` is Ctrl and the plus key.
    if text.trim().ends_with("++") {
        parts.truncate(parts.len() - 2);
        parts.push("+");
    }
    let (last, modifiers) = parts.split_last().ok_or("it's empty")?;
    let lower = last.to_lowercase();
    let key = match lower.as_str() {
        "enter" | "return" => Key::Enter,
        "space" => Key::Space,
        "tab" => Key::Tab,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "esc" | "escape" => return Err("Esc always closes, it can't be a shortcut".into()),
        _ => {
            let mut chars = lower.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Char(c),
                _ => return Err(format!("`{last}` isn't a key")),
            }
        }
    };
    let mut shortcut = eave_api::ui::shortcut(key);
    for modifier in modifiers {
        shortcut = match modifier.to_lowercase().as_str() {
            "ctrl" | "control" => shortcut.ctrl(),
            "alt" => shortcut.alt(),
            "shift" => shortcut.shift(),
            "super" | "logo" | "mod4" => shortcut.logo(),
            _ => return Err(format!("`{modifier}` isn't a modifier")),
        };
    }
    Ok(shortcut)
}

/// A shortcut as config.toml writes it, for messages: `ctrl+k`.
fn text(shortcut: &Shortcut) -> String {
    labels(shortcut).join("+").to_lowercase()
}

/// Whether pressing `key` with `modifiers` held is `shortcut`. A letter counts in either case.
pub fn matches(shortcut: &Shortcut, key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    let named = |name| *key == keyboard::Key::Named(name);
    let pressed = match shortcut.key {
        Key::Char(c) => match key {
            keyboard::Key::Character(typed) => typed
                .chars()
                .flat_map(char::to_lowercase)
                .eq(c.to_lowercase()),
            _ => false,
        },
        Key::Enter => named(Named::Enter),
        Key::Space => named(Named::Space),
        Key::Tab => named(Named::Tab),
        Key::Up => named(Named::ArrowUp),
        Key::Down => named(Named::ArrowDown),
        Key::Left => named(Named::ArrowLeft),
        Key::Right => named(Named::ArrowRight),
    };
    pressed
        && modifiers.control() == shortcut.ctrl
        && modifiers.alt() == shortcut.alt
        && modifiers.shift() == shortcut.shift
        && modifiers.logo() == shortcut.logo
}

/// Whether `shortcut` is a key that types something, with no Ctrl, Alt or Super: a text field with
/// the focus keeps it.
pub fn types(shortcut: &Shortcut) -> bool {
    !(shortcut.ctrl || shortcut.alt || shortcut.logo)
}

/// The keycaps for `shortcut`, modifiers first: `["Ctrl", "K"]`.
pub fn labels(shortcut: &Shortcut) -> Vec<String> {
    let modifiers = [
        (shortcut.ctrl, "Ctrl"),
        (shortcut.alt, "Alt"),
        (shortcut.shift, "Shift"),
        (shortcut.logo, "Super"),
    ];
    let key = match shortcut.key {
        Key::Char(c) => c.to_uppercase().collect(),
        Key::Enter => "Enter".to_owned(),
        Key::Space => "Space".to_owned(),
        Key::Tab => "Tab".to_owned(),
        Key::Up => "↑".to_owned(),
        Key::Down => "↓".to_owned(),
        Key::Left => "←".to_owned(),
        Key::Right => "→".to_owned(),
    };
    modifiers
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| name.to_owned())
        .chain([key])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eave_api::ui::{self, shortcut};
    use keyboard::Modifiers;

    fn char_key(c: &str) -> keyboard::Key {
        keyboard::Key::Character(c.into())
    }

    #[test]
    fn collects_rows_that_can_be_pressed_but_not_inside_tabs() {
        let panel: Node<u8> = ui::column([
            ui::header("Title"),
            ui::scroll(ui::list([
                ui::list_item("one")
                    .shortcut("one", shortcut('1'))
                    .on_press(1),
                ui::list_item("no press").shortcut("two", shortcut('2')),
                ui::list_item("no key").on_press(3),
            ])),
            ui::tabs([(
                "Tab",
                ui::list([ui::list_item("hidden")
                    .shortcut("four", shortcut('4'))
                    .on_press(4)]),
            )]),
            ui::list([ui::list_item("ten")
                .shortcut("ten", shortcut('0'))
                .on_press(10)]),
        ]);
        let mut out = Vec::new();
        collect(&panel, &mut out);
        assert_eq!(out, vec![(shortcut('1'), 1), (shortcut('0'), 10)]);
    }

    #[test]
    fn buttons_have_keys_too_even_in_a_header() {
        let show = || ui::button(ui::label("Show"), 5).shortcut("show", shortcut('t').ctrl());
        let mut panel: Node<u8> = ui::column([
            ui::header("Chat").trailing(show()),
            ui::row([ui::button(ui::label("No key"), 6), show()]),
        ]);
        let mut out = Vec::new();
        collect(&panel, &mut out);
        assert_eq!(
            out,
            vec![(shortcut('t').ctrl(), 5), (shortcut('t').ctrl(), 5)]
        );
        // The user's key wins, and the second loses the one it shares.
        let user = keys(&[("show", Some(shortcut('s').alt()))]);
        resolve_panel(&mut panel, &|_| &user);
        let mut out = Vec::new();
        collect(&panel, &mut out);
        assert_eq!(out, vec![(shortcut('s').alt(), 5)]);
    }

    fn keys(pairs: &[(&str, Option<Shortcut>)]) -> Keys {
        pairs
            .iter()
            .map(|(id, key)| (id.to_string(), *key))
            .collect()
    }

    fn resolved(panel: &mut Node<u8>, user: &Keys) -> Vec<(Shortcut, u8)> {
        resolve_panel(panel, &|_| user);
        let mut out = Vec::new();
        collect(panel, &mut out);
        out
    }

    #[test]
    fn the_users_keys_replace_the_plugins() {
        let row = |id, key, message| {
            ui::list_item("row")
                .shortcut(id, shortcut(key))
                .on_press(message)
        };
        let mut panel = ui::list([row("a", '1', 1), row("b", '2', 2), row("c", '3', 3)]);
        let user = keys(&[("a", Some(shortcut('1').alt())), ("b", None)]);
        assert_eq!(
            resolved(&mut panel, &user),
            [(shortcut('1').alt(), 1), (shortcut('3'), 3)]
        );
    }

    #[test]
    fn a_key_two_rows_want_goes_to_the_users_then_the_first() {
        let row = |id, key, message| {
            ui::list_item("row")
                .shortcut(id, shortcut(key))
                .on_press(message)
        };
        let mut panel = ui::list([row("a", '1', 1), row("b", '1', 2), row("c", '3', 3)]);
        assert_eq!(
            resolved(&mut panel, &Keys::new()),
            [(shortcut('1'), 1), (shortcut('3'), 3)]
        );

        let mut panel = ui::list([row("a", '1', 1), row("b", '2', 2), row("c", '3', 3)]);
        let user = keys(&[("c", Some(shortcut('1')))]);
        assert_eq!(
            resolved(&mut panel, &user),
            [(shortcut('2'), 2), (shortcut('1'), 3)]
        );
    }

    #[test]
    fn each_row_takes_its_own_widgets_keys() {
        // Rows that send 10 and up are a part's, with keys of its own (D-109).
        let row = |id, key, message| {
            ui::list_item("row")
                .shortcut(id, shortcut(key))
                .on_press(message)
        };
        let owner = keys(&[("liked", Some(shortcut('9')))]);
        let part = keys(&[("liked", Some(shortcut('l').ctrl()))]);
        let mut panel = ui::column([
            row("next", '1', 1),
            ui::list([row("liked", '1', 10), row("next", '2', 11)]),
        ]);
        resolve_panel(&mut panel, &|message| {
            if *message < 10 { &owner } else { &part }
        });
        let mut out = Vec::new();
        collect(&panel, &mut out);
        assert_eq!(
            out,
            [
                (shortcut('1'), 1),
                (shortcut('l').ctrl(), 10),
                (shortcut('2'), 11)
            ]
        );
    }

    #[test]
    fn dormer_keys_need_a_modifier_and_go_by_config_order() {
        let none = Keys::new();
        let volume = keys(&[("mute", Some(shortcut('m').ctrl()))]);
        let candidate = |widget, id, default: Option<Shortcut>, keys| Candidate {
            id,
            default,
            keys,
            widget,
        };
        let (got, warnings) = resolve_actions(&[
            candidate("media", Some("next"), Some(shortcut('n').ctrl()), &none),
            candidate("claude", Some("new"), Some(shortcut('n').ctrl()), &none),
            candidate("claude", Some("mute"), Some(shortcut('m').ctrl()), &none),
            candidate("volume", Some("mute"), None, &volume),
            candidate("apps", Some("plain"), Some(shortcut('p')), &none),
            candidate("apps", None, Some(shortcut('q').alt()), &none),
        ]);
        assert_eq!(
            got,
            [
                Some(shortcut('n').ctrl()),
                None,
                None,
                Some(shortcut('m').ctrl()),
                None,
                None,
            ]
        );
        assert_eq!(warnings.len(), 4, "{warnings:?}");
    }

    #[test]
    fn parses_keys_as_config_toml_writes_them() {
        assert_eq!(parse("1"), Ok(shortcut('1')));
        assert_eq!(parse("Ctrl+Shift+K"), Ok(shortcut('k').ctrl().shift()));
        assert_eq!(parse("super + enter"), Ok(shortcut(Key::Enter).logo()));
        assert_eq!(parse("alt+down"), Ok(shortcut(Key::Down).alt()));
        assert_eq!(parse("ctrl++"), Ok(shortcut('+').ctrl()));
        assert!(parse("").is_err());
        assert!(parse("esc").is_err());
        assert!(parse("hyper+k").is_err());
        assert!(parse("ctrl+kk").is_err());
    }

    #[test]
    fn matches_the_key_and_exactly_its_modifiers() {
        let one = shortcut('1');
        assert!(matches(&one, &char_key("1"), Modifiers::empty()));
        assert!(!matches(&one, &char_key("2"), Modifiers::empty()));
        assert!(!matches(&one, &char_key("1"), Modifiers::CTRL));

        let ctrl_k = shortcut('k').ctrl();
        assert!(matches(&ctrl_k, &char_key("k"), Modifiers::CTRL));
        assert!(matches(&ctrl_k, &char_key("K"), Modifiers::CTRL));
        assert!(!matches(&ctrl_k, &char_key("k"), Modifiers::empty()));
        assert!(!matches(
            &ctrl_k,
            &char_key("k"),
            Modifiers::CTRL | Modifiers::ALT
        ));

        let up = shortcut(Key::Up);
        assert!(matches(
            &up,
            &keyboard::Key::Named(Named::ArrowUp),
            Modifiers::empty()
        ));
        assert!(!matches(&up, &char_key("k"), Modifiers::empty()));
    }

    #[test]
    fn only_keys_without_ctrl_alt_or_super_type() {
        assert!(types(&shortcut('1')));
        assert!(types(&shortcut('a').shift()));
        assert!(!types(&shortcut('k').ctrl()));
        assert!(!types(&shortcut('k').logo()));
    }

    #[test]
    fn labels_put_modifiers_first() {
        assert_eq!(labels(&shortcut('1')), ["1"]);
        assert_eq!(labels(&shortcut('k').ctrl()), ["Ctrl", "K"]);
        assert_eq!(
            labels(&shortcut(Key::Enter).shift().ctrl()),
            ["Ctrl", "Shift", "Enter"]
        );
        assert_eq!(labels(&shortcut(Key::Down)), ["↓"]);
    }
}
