use std::fmt::Debug;

use serde::de::DeserializeOwned;

use crate::ui::{Action, Bay, Item, Notice, Panel, Part};
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

    /// The dialog contents, if this widget has one: a panel in the middle of the screen, wider
    /// than a popover, for what needs room, like a conversation. It opens only when asked
    /// ([`Effect::open_dialog`]), never on a click. Returning `None` while it's open closes it.
    fn dialog(&self) -> Option<Panel<Self::Message>> {
        None
    }

    /// Notifications to show on screen now, newest first. Only for a notification engine: the
    /// widget that serves `org.freedesktop.Notifications` lists here what it received. Any other
    /// widget sends its notifications with `Notify` on the session bus, like any app.
    ///
    /// While there are any, the bar's right island turns into a panel of them, like a popover. The
    /// host takes each one away once its key is no longer in the list. Keep the notices you mean to show in your state, and drop
    /// one when it's dismissed or expires.
    fn notices(&self) -> Vec<Notice<Self::Message>> {
        Vec::new()
    }

    /// What the widget can do, for the dormer: the host's search dialog, which a keybind opens.
    /// Typing finds an action by its title, subtitle and keywords, and picking it sends its message
    /// to this widget, like a click would.
    ///
    /// The host reads them only while the dormer is open, after each of your messages, so they can
    /// follow your state: "Mute" while sound plays, "Unmute" while muted. Leave out what can't be
    /// done right now. Keep it pure and cheap, like the views.
    fn actions(&self) -> Vec<Action<Self::Message>> {
        Vec::new()
    }

    /// What this widget shows in another widget's [`bay`](crate::ui::bay), when the user attached
    /// it there with `parts = [...]` in config.toml. `None` for a bay or subject it has nothing for.
    ///
    /// Where a widget sits is the user's choice: in a zone, the host reads [`item`](Widget::item)
    /// and [`popover`](Widget::popover) and never this; as a part, it reads only this. Everything
    /// else works the same in both places, and effects land where the widget shows:
    /// `open_popover` opens the owner's popover, a nudge nudges the owner's island. A widget that
    /// fits both places can return its popover here (`Some(self.popover()?.into())`), or share
    /// its content without the header, since the owner already says what it's about.
    ///
    /// Asked only while that popover or dialog is on screen. Keep it pure and cheap, like the views.
    fn part(&self, bay: &Bay) -> Option<Part<Self::Message>> {
        let _ = bay;
        None
    }
}
