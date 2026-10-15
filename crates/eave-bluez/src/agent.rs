//! Pairing. BlueZ asks for what a pairing needs (a passkey to confirm, a PIN to type, a code to
//! show) through an agent: an object the caller serves on the bus, which BlueZ calls back while it
//! pairs. eave serves one and registers it only to pair through [`pair`], so it answers only for
//! the device eave is pairing, never another program's. Each question goes out on [`requests`]
//! and waits for the plugin's [`answer`], or gives up after a minute.

use std::fmt;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use futures_util::stream;
use tokio::sync::{oneshot, watch};
use zbus::zvariant::OwnedObjectPath;

use crate::{DEVICE, Device, SERVICE, Stream};

const AGENT: &str = "/eave/bluez/agent";
/// It can show a code and take one typed, so BlueZ picks the safest way the device can do.
const CAPABILITY: &str = "KeyboardDisplay";
/// How long a question waits for its answer. Devices give up sooner, mostly.
const WAIT: Duration = Duration::from_secs(60);

/// What BlueZ asks while pairing a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Whether the device shows this passkey too.
    Confirm {
        device: OwnedObjectPath,
        passkey: u32,
    },
    /// A code to type on the device, a keyboard, then Enter.
    Display {
        device: OwnedObjectPath,
        code: String,
    },
    /// A PIN: the one the device shows, or came with (often 0000).
    PinCode { device: OwnedObjectPath },
    /// The six digits the device shows.
    Passkey { device: OwnedObjectPath },
    /// Whether to let the device pair.
    Authorize { device: OwnedObjectPath },
}

impl Request {
    pub fn device(&self) -> &OwnedObjectPath {
        match self {
            Self::Confirm { device, .. }
            | Self::Display { device, .. }
            | Self::PinCode { device }
            | Self::Passkey { device }
            | Self::Authorize { device } => device,
        }
    }
}

/// The answer to a [`Request`].
#[derive(Clone, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    /// A PIN or passkey, typed.
    Text(String),
}

impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yes => f.write_str("Yes"),
            Self::No => f.write_str("No"),
            Self::Text(_) => f.write_str("Text(…)"),
        }
    }
}

/// The device being paired, while it is.
static PAIRING: Mutex<Option<OwnedObjectPath>> = Mutex::new(None);
/// Where the answer to the question asked goes.
static REPLY: Mutex<Option<oneshot::Sender<Answer>>> = Mutex::new(None);
/// The question asked, until it's answered.
static REQUEST: LazyLock<watch::Sender<Option<Request>>> =
    LazyLock::new(|| watch::Sender::new(None));

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn pairing(device: &OwnedObjectPath) -> bool {
    lock(&PAIRING).as_ref() == Some(device)
}

/// Takes the question back: answered, cancelled or over.
fn clear() {
    lock(&REPLY).take();
    REQUEST.send_replace(None);
}

/// The question BlueZ asks now, if any, then again whenever that changes.
pub fn requests() -> impl Stream<Item = Option<Request>> {
    let receiver = REQUEST.subscribe();
    stream::unfold((receiver, true), |(mut receiver, first)| async move {
        if !first {
            receiver.changed().await.ok()?;
        }
        let request = receiver.borrow_and_update().clone();
        Some((request, (receiver, false)))
    })
}

/// Answers the question asked. Does nothing if there's none.
pub fn answer(answer: Answer) {
    if let Some(reply) = lock(&REPLY).take() {
        let _ = reply.send(answer);
    }
}

/// Stops answering for `device`: the question it waits on is turned down.
pub(crate) fn cancel(device: &OwnedObjectPath) {
    if pairing(device) {
        clear();
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

fn rejected() -> AgentError {
    AgentError::Rejected("eave only answers for the device it's pairing".into())
}

/// Asks the plugin `request`, and waits for the answer.
async fn ask(request: Request) -> Result<Answer, AgentError> {
    if !pairing(request.device()) {
        return Err(rejected());
    }
    let (reply, answered) = oneshot::channel();
    *lock(&REPLY) = Some(reply);
    REQUEST.send_replace(Some(request));
    let answer = tokio::time::timeout(WAIT, answered).await;
    clear();
    match answer {
        Ok(Ok(Answer::No)) => Err(AgentError::Rejected("turned down".into())),
        Ok(Ok(answer)) => Ok(answer),
        _ => Err(AgentError::Canceled("no answer".into())),
    }
}

/// Shows `request`, which needs no answer, while the device is being paired.
fn show(request: Request) -> Result<(), AgentError> {
    if !pairing(request.device()) {
        return Err(rejected());
    }
    REQUEST.send_replace(Some(request));
    Ok(())
}

fn yes(answer: Answer) -> Result<(), AgentError> {
    match answer {
        Answer::Yes => Ok(()),
        _ => Err(AgentError::Rejected("turned down".into())),
    }
}

fn text(answer: Answer) -> Result<String, AgentError> {
    match answer {
        Answer::Text(text) if !text.is_empty() => Ok(text),
        _ => Err(AgentError::Rejected("nothing typed".into())),
    }
}

struct Agent;

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {
        clear();
    }

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        text(ask(Request::PinCode { device }).await?)
    }

    fn display_pin_code(&self, device: OwnedObjectPath, pincode: String) -> Result<(), AgentError> {
        show(Request::Display {
            device,
            code: pincode,
        })
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        let typed = text(ask(Request::Passkey { device }).await?)?;
        typed
            .trim()
            .parse()
            .map_err(|_| AgentError::Rejected("not a passkey".into()))
    }

    fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        _entered: u16,
    ) -> Result<(), AgentError> {
        show(Request::Display {
            device,
            code: format!("{passkey:06}"),
        })
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        yes(ask(Request::Confirm { device, passkey }).await?)
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        yes(ask(Request::Authorize { device }).await?)
    }

    /// A profile of the device being paired wants to connect: it's being paired to connect.
    fn authorize_service(&self, device: OwnedObjectPath, _uuid: String) -> Result<(), AgentError> {
        if pairing(&device) {
            Ok(())
        } else {
            Err(rejected())
        }
    }

    fn cancel(&self) {
        clear();
    }
}

/// Pairs `device`, answering BlueZ through the agent while it does.
pub(crate) async fn pair(device: &Device) -> zbus::Result<()> {
    let connection = eave_dbus::system().await?;
    // Served once per connection; `at` says false when it already is.
    connection.object_server().at(AGENT, Agent).await?;
    // Registered every time, so it holds even after BlueZ restarts. BlueZ asks the agent of
    // whoever called Pair, so it isn't made the default, and other programs' pairings never
    // reach it.
    let agent = OwnedObjectPath::try_from(AGENT)?;
    match connection
        .call_method(
            Some(SERVICE),
            "/org/bluez",
            Some("org.bluez.AgentManager1"),
            "RegisterAgent",
            &(agent, CAPABILITY),
        )
        .await
    {
        Ok(_) => {}
        Err(zbus::Error::MethodError(name, ..)) if name.as_str().ends_with("AlreadyExists") => {}
        Err(error) => return Err(error),
    }

    *lock(&PAIRING) = Some(device.path.clone());
    let result = connection
        .call_method(Some(SERVICE), &device.path, Some(DEVICE), "Pair", &())
        .await;
    *lock(&PAIRING) = None;
    clear();
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_only_for_the_device_being_paired() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let buds = OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA").unwrap();
            let other = OwnedObjectPath::try_from("/org/bluez/hci0/dev_BB").unwrap();
            *lock(&PAIRING) = Some(buds.clone());
            assert!(
                ask(Request::Authorize { device: other }).await.is_err(),
                "another device is turned down"
            );

            let asked = Request::Confirm {
                device: buds.clone(),
                passkey: 123456,
            };
            let question = tokio::spawn(ask(asked.clone()));
            // The question goes out, then the answer comes back.
            let mut requests = std::pin::pin!(requests());
            use futures_util::StreamExt;
            let mut seen = requests.next().await.unwrap();
            while seen.is_none() {
                seen = requests.next().await.unwrap();
            }
            assert_eq!(seen, Some(asked));
            answer(Answer::Yes);
            assert_eq!(question.await.unwrap().unwrap(), Answer::Yes);
            assert_eq!(*REQUEST.borrow(), None, "taken back once answered");

            *lock(&PAIRING) = None;
        });
    }

    #[test]
    fn typed_answers_stay_out_of_logs() {
        assert!(!format!("{:?}", Answer::Text("0000".into())).contains("0000"));
    }
}
