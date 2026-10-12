//! The focused window's title. Hidden when nothing has focus.

use eave_api::ui::{self, Item};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use eave_hyprland::types::Window as Focused;
use eave_hyprland::{Event, Update};
use serde::Deserialize;

pub fn register(registry: &mut Registry) {
    registry.widget::<Window>("window");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Longer titles are cut, with an ellipsis.
    max_chars: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { max_chars: 60 }
    }
}

pub struct Window {
    max_chars: usize,
    /// Already shortened.
    title: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Hyprland(Update<Focused>),
}

impl Widget for Window {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let window = Self {
            max_chars: config.max_chars.max(1),
            title: String::new(),
        };
        (window, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let title = match message {
            Message::Hyprland(Update::Snapshot(focused)) => focused.title,
            Message::Hyprland(Update::Event(Event::ActiveWindow { title, .. })) => title,
            Message::Hyprland(Update::Event(_)) => return Effect::none(),
        };
        self.title = shorten(title.trim(), self.max_chars);
        Effect::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(|| eave_hyprland::watch(snapshot)).map(Message::Hyprland)
    }

    fn item(&self) -> Item<Message> {
        if self.title.is_empty() {
            Item::hidden()
        } else {
            Item::new(ui::label(self.title.clone()))
        }
    }
}

async fn snapshot() -> std::io::Result<Focused> {
    eave_hyprland::query("activewindow").await
}

fn shorten(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        None => text.to_string(),
        Some(_) => {
            let cut = text
                .char_indices()
                .nth(max_chars - 1)
                .map_or(text.len(), |(i, _)| i);
            format!("{}…", text[..cut].trim_end())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_by_characters() {
        assert_eq!(shorten("kitty", 5), "kitty");
        assert_eq!(shorten("kitty — fish", 7), "kitty…");
        assert_eq!(shorten("ééééé", 3), "éé…");
    }

    #[test]
    fn follows_focus() {
        let (mut window, _) = Window::new(Config::default());
        window.update(Message::Hyprland(Update::Event(Event::ActiveWindow {
            class: "kitty".into(),
            title: "fish".into(),
        })));
        assert_eq!(window.title, "fish");
        window.update(Message::Hyprland(Update::Event(Event::ActiveWindow {
            class: String::new(),
            title: String::new(),
        })));
        assert!(window.item().into_parts().content.is_none());
    }
}
