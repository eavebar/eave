//! Type erasure, so the host can hold widgets from any plugin in one list.
//!
//! Plugins don't use this module directly: every [`Widget`] is an [`ErasedWidget`] through the
//! blanket impl below.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

pub use crate::effect::{DialogRequest, PopoverRequest};
use crate::ui::{Action, Item, Notice, Panel};
use crate::{Effect, NudgeRequest, Subscription, Task, Widget};

/// A message of some widget's own type. Cloning is cheap: the value sits behind an `Arc`.
#[derive(Clone)]
pub struct AnyMessage {
    value: Arc<dyn Any + Send + Sync>,
    type_name: &'static str,
}

impl AnyMessage {
    pub fn new<M: Any + Send + Sync>(message: M) -> Self {
        Self {
            value: Arc::new(message),
            type_name: std::any::type_name::<M>(),
        }
    }

    pub fn downcast<M: Any + Clone + Send + Sync>(self) -> Option<M> {
        let value = self.value.downcast::<M>().ok()?;
        Some(Arc::try_unwrap(value).unwrap_or_else(|shared| (*shared).clone()))
    }
}

impl fmt::Debug for AnyMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AnyMessage<{}>", self.type_name)
    }
}

pub trait ErasedWidget {
    fn update(&mut self, message: AnyMessage) -> Effect<AnyMessage>;
    fn subscription(&self) -> Subscription<AnyMessage>;
    fn item(&self) -> Item<AnyMessage>;
    fn popover(&self) -> Option<Panel<AnyMessage>>;
    fn dialog(&self) -> Option<Panel<AnyMessage>>;
    fn notices(&self) -> Vec<Notice<AnyMessage>>;
    fn actions(&self) -> Vec<Action<AnyMessage>>;
}

impl<W: Widget> ErasedWidget for W {
    fn update(&mut self, message: AnyMessage) -> Effect<AnyMessage> {
        // Messages are always routed back to the instance that produced them, so a mismatch is a host bug.
        let Some(message) = message.downcast::<W::Message>() else {
            debug_assert!(false, "message routed to the wrong widget");
            return Effect::none();
        };
        let effect = Widget::update(self, message);
        let dialog = effect.dialog_request();
        let (task, popover, nudge) = effect.into_parts();
        let effect = Effect::task(task.map(AnyMessage::new));
        let effect = match popover {
            Some(PopoverRequest::Open) => effect.open_popover(),
            Some(PopoverRequest::Close) => effect.close_popover(),
            None => effect,
        };
        let effect = match dialog {
            Some(DialogRequest::Open) => effect.open_dialog(),
            Some(DialogRequest::Close) => effect.close_dialog(),
            None => effect,
        };
        match nudge {
            Some(NudgeRequest {
                nudge,
                lasting: Some(lasting),
            }) => effect.nudge_lasting(nudge, lasting),
            Some(NudgeRequest { nudge, .. }) => effect.nudge(nudge),
            None => effect,
        }
    }

    fn subscription(&self) -> Subscription<AnyMessage> {
        Widget::subscription(self).map(AnyMessage::new::<W::Message>)
    }

    fn item(&self) -> Item<AnyMessage> {
        Widget::item(self).map(&AnyMessage::new)
    }

    fn popover(&self) -> Option<Panel<AnyMessage>> {
        Widget::popover(self).map(|panel| panel.map(&AnyMessage::new))
    }

    fn dialog(&self) -> Option<Panel<AnyMessage>> {
        Widget::dialog(self).map(|panel| panel.map(&AnyMessage::new))
    }

    fn notices(&self) -> Vec<Notice<AnyMessage>> {
        Widget::notices(self)
            .into_iter()
            .map(|notice| notice.map(&AnyMessage::new))
            .collect()
    }

    fn actions(&self) -> Vec<Action<AnyMessage>> {
        Widget::actions(self)
            .into_iter()
            .map(|action| action.map(&AnyMessage::new))
            .collect()
    }
}

pub type Constructor = fn(toml::Table) -> Result<(Box<dyn ErasedWidget>, Task<AnyMessage>), String>;

pub(crate) fn construct<W: Widget>(
    config: toml::Table,
) -> Result<(Box<dyn ErasedWidget>, Task<AnyMessage>), String> {
    let config: W::Config = config.try_into().map_err(|error: toml::de::Error| {
        error
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })?;
    let (widget, task) = W::new(config);
    Ok((Box::new(widget), task.map(AnyMessage::new)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_config_accepts_only_nothing() {
        let empty: Result<crate::NoConfig, _> = toml::Table::new().try_into();
        assert!(empty.is_ok());
        let mut extra = toml::Table::new();
        extra.insert("format".into(), "x".into());
        assert!(extra.try_into::<crate::NoConfig>().is_err());
    }

    #[test]
    fn actions_carry_the_widgets_messages() {
        struct Acting;
        impl Widget for Acting {
            type Config = crate::NoConfig;
            type Message = u8;
            fn new(_: Self::Config) -> (Self, Task<u8>) {
                (Acting, Task::none())
            }
            fn update(&mut self, _: u8) -> Effect<u8> {
                Effect::none()
            }
            fn item(&self) -> Item<u8> {
                Item::new(crate::ui::empty())
            }
            fn actions(&self) -> Vec<Action<u8>> {
                vec![crate::ui::action("Mute", 7).keywords(["sound"])]
            }
        }

        let actions = ErasedWidget::actions(&Acting);
        let parts = actions.into_iter().next().unwrap().into_parts();
        assert_eq!(parts.title, "Mute");
        assert_eq!(parts.keywords, ["sound"]);
        assert_eq!(parts.message.downcast::<u8>(), Some(7));
    }

    #[test]
    fn dialogs_survive_erasure() {
        struct Talking;
        impl Widget for Talking {
            type Config = crate::NoConfig;
            type Message = u8;
            fn new(_: Self::Config) -> (Self, Task<u8>) {
                (Talking, Task::none())
            }
            fn update(&mut self, message: u8) -> Effect<u8> {
                match message {
                    0 => Effect::none().open_dialog(),
                    _ => Effect::none().close_dialog().open_popover(),
                }
            }
            fn item(&self) -> Item<u8> {
                Item::new(crate::ui::empty())
            }
            fn dialog(&self) -> Option<Panel<u8>> {
                Some(Panel::new(crate::ui::label("Hello")))
            }
        }

        let mut talking = Talking;
        let effect = ErasedWidget::update(&mut talking, AnyMessage::new(0u8));
        assert_eq!(effect.dialog_request(), Some(DialogRequest::Open));
        let effect = ErasedWidget::update(&mut talking, AnyMessage::new(1u8));
        assert_eq!(effect.dialog_request(), Some(DialogRequest::Close));
        assert_eq!(effect.into_parts().1, Some(PopoverRequest::Open));
        assert!(ErasedWidget::dialog(&talking).is_some());
    }

    #[test]
    fn downcast_round_trips() {
        let message = AnyMessage::new(42u8);
        assert_eq!(message.clone().downcast::<u8>(), Some(42));
        assert_eq!(message.downcast::<u16>(), None);
    }
}
