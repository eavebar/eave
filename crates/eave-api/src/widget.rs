use std::fmt::Debug;

use serde::de::DeserializeOwned;

use crate::ui::{Item, Panel};
use crate::{Effect, Subscription, Task};

/// Something that sits in the bar, with an optional popover.
///
/// It follows the same Elm shape as the host: state lives in `Self`, changes arrive as messages in
/// [`update`](Widget::update), and the views are pure functions of state. `update` must not block:
/// do I/O in a [`Task`] or a [`Subscription`].
pub trait Widget: Sized + 'static {
    /// Options from this widget's entry in `config.toml`, minus the `widget` key.
    ///
    /// The entry may be empty, so use `#[serde(default)]` for options that have one.
    type Config: DeserializeOwned;
    type Message: Clone + Debug + Send + Sync + 'static;

    fn new(config: Self::Config) -> (Self, Task<Self::Message>);

    fn update(&mut self, message: Self::Message) -> Effect<Self::Message>;

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }

    /// What the widget shows in the bar.
    fn item(&self) -> Item<Self::Message>;

    /// The popover contents, if this widget has one.
    ///
    /// When it returns `Some` and the item has no `on_press`, a click on the item opens and
    /// closes the popover. Returning `None` while the popover is open closes it.
    fn popover(&self) -> Option<Panel<Self::Message>> {
        None
    }
}
