//! Type erasure, so the host can hold widgets from any plugin in one list.
//!
//! Plugins don't use this module directly: every [`Widget`] is an [`ErasedWidget`] through the
//! blanket impl below.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

pub use crate::effect::PopoverRequest;
use crate::ui::{Item, Notice, Panel};
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
    fn notices(&self) -> Vec<Notice<AnyMessage>>;
}

impl<W: Widget> ErasedWidget for W {
    fn update(&mut self, message: AnyMessage) -> Effect<AnyMessage> {
        // Messages are always routed back to the instance that produced them, so a mismatch is a host bug.
        let Some(message) = message.downcast::<W::Message>() else {
            debug_assert!(false, "message routed to the wrong widget");
            return Effect::none();
        };
        let (task, popover, nudge) = Widget::update(self, message).into_parts();
        let effect = Effect::task(task.map(AnyMessage::new));
        let effect = match popover {
            Some(PopoverRequest::Open) => effect.open_popover(),
            Some(PopoverRequest::Close) => effect.close_popover(),
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

    fn notices(&self) -> Vec<Notice<AnyMessage>> {
        Widget::notices(self)
            .into_iter()
            .map(|notice| notice.map(&AnyMessage::new))
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
    fn downcast_round_trips() {
        let message = AnyMessage::new(42u8);
        assert_eq!(message.clone().downcast::<u8>(), Some(42));
        assert_eq!(message.downcast::<u16>(), None);
    }
}
