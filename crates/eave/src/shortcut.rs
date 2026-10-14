//! Keys a popover's rows name ([`eave_api::ui::Node::shortcut`]): finding them in a panel, telling
//! whether a key press is one, and the labels on their keycaps (D-88).

use eave_api::ui::{Key, Kind, Node, Shortcut};
use iced::keyboard::{self, key::Named};

/// Every row in `node` with a shortcut and something to press, in order. Rows inside tabs are
/// left out: only one page shows, and a key shouldn't press a row that's hidden.
pub fn collect<M: Clone>(node: &Node<M>, out: &mut Vec<(Shortcut, M)>) {
    match node.kind() {
        Kind::ListItem {
            shortcut: Some(shortcut),
            on_press: Some(message),
            ..
        } => out.push((*shortcut, message.clone())),
        Kind::Row(children)
        | Kind::Column(children)
        | Kind::List(children)
        | Kind::Grid {
            cells: children, ..
        } => children.iter().for_each(|child| collect(child, out)),
        Kind::Center(content) | Kind::Display(content) | Kind::Scroll { content, .. } => {
            collect(content, out)
        }
        _ => {}
    }
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
                ui::list_item("one").shortcut(shortcut('1')).on_press(1),
                ui::list_item("no press").shortcut(shortcut('2')),
                ui::list_item("no key").on_press(3),
            ])),
            ui::tabs([(
                "Tab",
                ui::list([ui::list_item("hidden").shortcut(shortcut('4')).on_press(4)]),
            )]),
            ui::list([ui::list_item("ten").shortcut(shortcut('0')).on_press(10)]),
        ]);
        let mut out = Vec::new();
        collect(&panel, &mut out);
        assert_eq!(out, vec![(shortcut('1'), 1), (shortcut('0'), 10)]);
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
