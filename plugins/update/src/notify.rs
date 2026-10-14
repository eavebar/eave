//! Notifications, sent like any app's: `Notify` on `org.freedesktop.Notifications`, on the
//! session bus ([D-52](https://github.com/eaveshell/eave/blob/main/docs/decisions.md)). A check
//! that finds updates offers Update on its notification, and the daemon's `ActionInvoked` says
//! when it's picked.

use std::collections::HashMap;

use eave_dbus::Stream;
use eave_dbus::zbus::message::Type as MessageType;
use eave_dbus::zbus::{self, MatchRule, MessageStream, zvariant::Value};
use futures_util::{StreamExt, stream};

use crate::summary::{Note, Urgency};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// The key of the action that updates.
pub const UPDATE: &str = "update";

/// Shows `note`, and returns its id.
pub async fn send(note: Note) -> zbus::Result<u32> {
    let connection = eave_dbus::session().await?;
    let urgency: u8 = match note.urgency {
        Urgency::Low => 0,
        Urgency::Normal => 1,
        Urgency::Critical => 2,
    };
    let hints = HashMap::from([("urgency", Value::from(urgency))]);
    let actions = match note.offer_update {
        true => vec![UPDATE, "Update"],
        false => Vec::new(),
    };
    let reply = connection
        .call_method(
            Some(NAME),
            PATH,
            Some(NAME),
            "Notify",
            &(
                "eave",
                0u32,
                "system-software-update",
                note.summary.as_str(),
                note.body.as_str(),
                actions,
                hints,
                -1i32,
            ),
        )
        .await?;
    reply.body().deserialize()
}

/// Takes the notification `id` away.
pub async fn close(id: u32) -> zbus::Result<()> {
    let connection = eave_dbus::session().await?;
    connection
        .call_method(Some(NAME), PATH, Some(NAME), "CloseNotification", &(id,))
        .await?;
    Ok(())
}

/// What happened to a notification.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Its action with this key was picked.
    Invoked(u32, String),
    Closed(u32),
}

/// Every notification's actions picked and closings, from whichever daemon shows them. Ends with
/// no session bus.
pub fn events() -> impl Stream<Item = Event> {
    stream::once(subscribe())
        .filter_map(|subscribed| async move {
            subscribed
                .inspect_err(|error| eprintln!("eave: update: notifications: {error}"))
                .ok()
        })
        .flatten()
        .filter_map(|message| async move {
            let message = message.ok()?;
            let header = message.header();
            match header.member()?.as_str() {
                "ActionInvoked" => {
                    let (id, key): (u32, String) = message.body().deserialize().ok()?;
                    Some(Event::Invoked(id, key))
                }
                "NotificationClosed" => {
                    let (id, _): (u32, u32) = message.body().deserialize().ok()?;
                    Some(Event::Closed(id))
                }
                _ => None,
            }
        })
}

async fn subscribe() -> zbus::Result<MessageStream> {
    let connection = eave_dbus::session().await?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface(NAME)?
        .path(PATH)?
        .build();
    MessageStream::for_match_rule(rule, &connection, Some(8)).await
}
