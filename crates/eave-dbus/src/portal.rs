//! The desktop's color scheme, as the settings portal tells it: `org.freedesktop.appearance`
//! `color-scheme` (D-92). Read once, then again on every `SettingChanged`, with no polling.

use futures_util::{Stream, StreamExt, stream};
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{MatchRule, MessageStream};

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// What the desktop prefers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    #[default]
    NoPreference,
    Dark,
    Light,
}

/// The color scheme now, then each time it changes. Without a portal, or one without the
/// setting, it says `NoPreference` once and ends.
pub fn color_scheme() -> impl Stream<Item = ColorScheme> {
    enum State {
        Start,
        Listening(Box<MessageStream>),
        Done,
    }
    stream::unfold(State::Start, |state| async move {
        match state {
            State::Start => match start().await {
                Ok((scheme, signals)) => Some((scheme, State::Listening(Box::new(signals)))),
                Err(error) => {
                    eprintln!("eave-dbus: no color scheme from the settings portal: {error}");
                    Some((ColorScheme::NoPreference, State::Done))
                }
            },
            State::Listening(mut signals) => loop {
                let Ok(message) = signals.next().await? else {
                    continue;
                };
                let Ok((namespace, key, value)) =
                    message.body().deserialize::<(String, String, OwnedValue)>()
                else {
                    continue;
                };
                if namespace == NAMESPACE && key == KEY {
                    return Some((scheme(&value), State::Listening(signals)));
                }
            },
            State::Done => None,
        }
    })
}

/// Listens for changes first, so one made while the setting is read isn't missed, then reads it.
async fn start() -> zbus::Result<(ColorScheme, MessageStream)> {
    let connection = crate::session().await?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(DESTINATION)?
        .interface(INTERFACE)?
        .member("SettingChanged")?
        .arg(0, NAMESPACE)?
        .build();
    let signals = MessageStream::for_match_rule(rule, &connection, Some(8)).await?;
    let read = |method| {
        connection.call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            method,
            &(NAMESPACE, KEY),
        )
    };
    // `ReadOne` since version 2 of the interface; older portals only have `Read`, which wraps
    // the value in one more variant.
    let reply = match read("ReadOne").await {
        Ok(reply) => reply,
        Err(_) => read("Read").await?,
    };
    let value: OwnedValue = reply.body().deserialize()?;
    Ok((scheme(&value), signals))
}

/// The portal's number, however many variants it's wrapped in: 1 is dark, 2 is light.
fn scheme(value: &Value<'_>) -> ColorScheme {
    match value {
        Value::Value(inner) => scheme(inner),
        Value::U32(1) => ColorScheme::Dark,
        Value::U32(2) => ColorScheme::Light,
        _ => ColorScheme::NoPreference,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portals_number_says_the_scheme_however_wrapped() {
        assert_eq!(scheme(&Value::U32(0)), ColorScheme::NoPreference);
        assert_eq!(scheme(&Value::U32(1)), ColorScheme::Dark);
        assert_eq!(scheme(&Value::U32(2)), ColorScheme::Light);
        let wrapped = Value::Value(Box::new(Value::U32(2)));
        assert_eq!(scheme(&wrapped), ColorScheme::Light);
        assert_eq!(scheme(&Value::from("dark")), ColorScheme::NoPreference);
    }
}
