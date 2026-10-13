//! Notifications, sent like any app's: `Notify` on `org.freedesktop.Notifications`, on the
//! session bus ([D-52](https://github.com/eaveshell/eave/blob/main/docs/decisions.md)). Whoever
//! owns that name shows them: eave's own engine (eave-notifications) or mako, dunst, swaync. With
//! no daemon, they're lost, as any app's would be.

use std::collections::HashMap;

use eave_dbus::zbus::{self, zvariant::Value};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// What a notification says.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// A name from the icon theme.
    pub icon: &'static str,
    pub summary: String,
    pub body: String,
    /// Shown until dismissed, never expiring, even under do not disturb.
    pub critical: bool,
}

/// Shows `note`, and returns its id.
pub async fn send(note: Note) -> zbus::Result<u32> {
    let connection = eave_dbus::session().await?;
    let urgency: u8 = if note.critical { 2 } else { 1 };
    let hints = HashMap::from([
        ("urgency", Value::from(urgency)),
        ("category", Value::from("device")),
    ]);
    let actions: Vec<&str> = Vec::new();
    let reply = connection
        .call_method(
            Some(NAME),
            PATH,
            Some(NAME),
            "Notify",
            &(
                "Battery",
                0u32,
                note.icon,
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
