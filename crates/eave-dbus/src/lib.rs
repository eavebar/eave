//! D-Bus for eave plugins ([D-7](https://github.com/eavebar/eave/blob/main/docs/decisions.md)).
//!
//! One system bus connection, shared by every widget. [`watch`] gives a widget a snapshot of a
//! service's state and a fresh one whenever the service reports a property change on an object
//! the snapshot depends on, with no polling:
//!
//! ```ignore
//! fn subscription(&self) -> Subscription<Message> {
//!     Subscription::run(|| eave_dbus::watch("org.freedesktop.UPower", snapshot)).map(Message::Battery)
//! }
//! ```

use std::collections::HashMap;
use std::time::Duration;

use futures_util::{FutureExt, StreamExt, stream};
use tokio::sync::OnceCell;
use zbus::message::Type as MessageType;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{Connection, MatchRule, MessageStream};

pub use futures_util::Stream;
pub use zbus;

/// The shared system bus connection, opened on first use.
pub async fn system() -> zbus::Result<Connection> {
    static SYSTEM: OnceCell<Connection> = OnceCell::const_new();
    SYSTEM.get_or_try_init(Connection::system).await.cloned()
}

/// What a snapshot function returns: the state, and the objects whose changes make it stale.
pub struct Snapshot<S> {
    pub state: S,
    pub watch: Vec<OwnedObjectPath>,
}

/// A snapshot of `service`, then a new one after every property change on the objects the last
/// snapshot listed. Unchanged snapshots aren't yielded, so a chatty service doesn't cause
/// redraws. Errors are logged and retried on the service's next signal. If the service isn't
/// installed at all, the stream ends.
pub fn watch<S, F, Fut>(service: &'static str, snapshot: F) -> impl Stream<Item = S>
where
    S: PartialEq + Clone,
    F: Fn(Connection) -> Fut,
    Fut: Future<Output = zbus::Result<Snapshot<S>>>,
{
    watching(service, snapshot, false)
}

/// Like [`watch`], but any signal from `service` makes the snapshot stale: objects coming and
/// going (`InterfacesAdded`, `InterfacesRemoved`) as well as every property change. For services
/// whose objects change often, like Wi-Fi networks, where listing them all would be the snapshot.
pub fn watch_all<S, F, Fut>(service: &'static str, snapshot: F) -> impl Stream<Item = S>
where
    S: PartialEq + Clone,
    F: Fn(Connection) -> Fut,
    Fut: Future<Output = zbus::Result<Snapshot<S>>>,
{
    watching(service, snapshot, true)
}

fn watching<S, F, Fut>(
    service: &'static str,
    snapshot: F,
    everything: bool,
) -> impl Stream<Item = S>
where
    S: PartialEq + Clone,
    F: Fn(Connection) -> Fut,
    Fut: Future<Output = zbus::Result<Snapshot<S>>>,
{
    struct State<S, F> {
        snapshot: F,
        signals: Option<(Connection, MessageStream)>,
        watched: Vec<OwnedObjectPath>,
        last: Option<S>,
        stale: bool,
    }

    let state = State {
        snapshot,
        signals: None,
        watched: Vec::new(),
        last: None,
        stale: true,
    };
    stream::unfold(state, move |mut state| async move {
        loop {
            if state.signals.is_none() {
                match subscribe(service, everything).await {
                    Ok(signals) => state.signals = Some(signals),
                    Err(error) => {
                        // No bus at all: nothing will ever change, so end quietly.
                        eprintln!("eave-dbus: {service}: {error}");
                        return None;
                    }
                }
            }
            let (connection, signals) = state.signals.as_mut()?;

            if state.stale {
                // Subscribed first, so a change during the snapshot still marks it stale.
                match (state.snapshot)(connection.clone()).await {
                    Ok(snapshot) => {
                        state.stale = false;
                        state.watched = snapshot.watch;
                        if state.last.as_ref() != Some(&snapshot.state) {
                            state.last = Some(snapshot.state.clone());
                            return Some((snapshot.state, state));
                        }
                    }
                    Err(error) if state.last.is_none() && is_not_installed(&error) => {
                        // Nothing provides it and nothing can start it: end, so the widget
                        // keeps its "absent" state instead of waiting forever.
                        eprintln!("eave-dbus: {service} isn't installed");
                        return None;
                    }
                    Err(error) => eprintln!("eave-dbus: {service}: {error}"),
                }
            }

            let message = signals.next().await?;
            let relevant = |message| everything || is_relevant(message, &state.watched);
            let mut stale = relevant(message);
            // A change usually comes as a burst of signals; take them all before re-reading.
            while let Some(Some(message)) = signals.next().now_or_never() {
                stale |= relevant(message);
            }
            if stale {
                // Some services update several objects one after another; let them settle.
                tokio::time::sleep(Duration::from_millis(50)).await;
                while let Some(Some(_)) = signals.next().now_or_never() {}
                state.stale = true;
            }
        }
    })
}

async fn subscribe(
    service: &'static str,
    everything: bool,
) -> zbus::Result<(Connection, MessageStream)> {
    let connection = system().await?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(service)?;
    let rule = if everything {
        rule.build()
    } else {
        rule.interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .build()
    };
    let signals = MessageStream::for_match_rule(rule, &connection, Some(64)).await?;
    Ok((connection, signals))
}

/// The bus doesn't know the service and can't activate it. A service that is installed but not
/// running yet (`NameHasNoOwner`) isn't this: its signals arrive once it starts.
fn is_not_installed(error: &zbus::Error) -> bool {
    match error {
        zbus::Error::FDO(error) => matches!(**error, zbus::fdo::Error::ServiceUnknown(_)),
        zbus::Error::MethodError(name, ..) => {
            name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown"
        }
        _ => false,
    }
}

/// The service is there, but we may not use it: its bus policy turns us away.
pub fn is_access_denied(error: &zbus::Error) -> bool {
    match error {
        zbus::Error::FDO(error) => matches!(**error, zbus::fdo::Error::AccessDenied(_)),
        zbus::Error::MethodError(name, ..) => {
            name.as_str() == "org.freedesktop.DBus.Error.AccessDenied"
        }
        _ => false,
    }
}

/// Nothing answers for the service right now: not installed, or not running.
pub fn is_unavailable(error: &zbus::Error) -> bool {
    let name = match error {
        zbus::Error::FDO(error) => {
            return matches!(
                **error,
                zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
            );
        }
        zbus::Error::MethodError(name, ..) => name.as_str(),
        _ => return false,
    };
    matches!(
        name,
        "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner"
    )
}

/// Every object `service` manages under `path`, with each interface's properties, in one call.
pub async fn managed_objects(
    connection: &Connection,
    service: &'static str,
    path: &'static str,
) -> zbus::Result<zbus::fdo::ManagedObjects> {
    let proxy = zbus::fdo::ObjectManagerProxy::builder(connection)
        .destination(service)?
        .path(path)?
        .build()
        .await?;
    Ok(proxy.get_managed_objects().await?)
}

fn is_relevant(message: zbus::Result<zbus::Message>, watched: &[OwnedObjectPath]) -> bool {
    let Ok(message) = message else {
        return true;
    };
    let header = message.header();
    header
        .path()
        .is_none_or(|path| watched.iter().any(|w| w.as_str() == path.as_str()))
}

/// Every property of `interface` on one object, read in one call.
pub async fn properties(
    connection: &Connection,
    service: &'static str,
    path: &str,
    interface: &'static str,
) -> zbus::Result<Properties> {
    let proxy = zbus::fdo::PropertiesProxy::builder(connection)
        .destination(service)?
        .path(ObjectPath::try_from(path)?)?
        .build()
        .await?;
    let interface = zbus::names::InterfaceName::try_from(interface)?;
    Ok(Properties(proxy.get_all(interface).await?))
}

/// Properties by name, read leniently: a missing or mistyped property reads as its default.
#[derive(Debug, Default)]
pub struct Properties(HashMap<String, OwnedValue>);

impl Properties {
    pub fn get<T>(&self, name: &str) -> T
    where
        T: TryFrom<OwnedValue> + Default,
    {
        self.0
            .get(name)
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| T::try_from(value).ok())
            .unwrap_or_default()
    }

    /// An object path property, `None` when unset (D-Bus uses `/` for "none").
    pub fn path(&self, name: &str) -> Option<OwnedObjectPath> {
        let path: OwnedObjectPath = self
            .0
            .get(name)
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| OwnedObjectPath::try_from(value).ok())?;
        (path.as_str() != "/").then_some(path)
    }

    pub fn value(&self, name: &str) -> Option<&OwnedValue> {
        self.0.get(name)
    }
}
