//! Hyprland IPC for eave plugins ([D-6](https://github.com/francescomengarelli/eave/blob/main/docs/decisions.md)).
//!
//! Hyprland has two Unix sockets: `.socket2.sock` streams events, `.socket.sock` answers one
//! request per connection. Every widget that calls [`watch`] shares one socket2 connection, which
//! is opened the first time someone watches and reconnects if Hyprland restarts.
//!
//! A widget subscribes with a snapshot function, so it gets its initial state and then only the
//! events after it, with nothing lost in between:
//!
//! ```ignore
//! fn subscription(&self) -> Subscription<Message> {
//!     Subscription::run(|| eave_hyprland::watch(snapshot)).map(Message::Hyprland)
//! }
//!
//! async fn snapshot() -> std::io::Result<Vec<Workspace>> {
//!     eave_hyprland::query("workspaces").await
//! }
//! ```

mod event;
pub mod types;

use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use futures_util::stream;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, broadcast};

pub use event::Event;
pub use futures_util::Stream;

/// What [`watch`] yields.
#[derive(Debug, Clone)]
pub enum Update<S> {
    /// Fresh state from the snapshot function. Comes first, and again whenever events may have
    /// been missed (Hyprland restarted, or this watcher fell behind).
    Snapshot(S),
    Event(Event),
}

/// A snapshot, then every event after it. Never yields errors: failures are logged, and the
/// snapshot is retried on the next sign of life from Hyprland.
pub fn watch<S, F, Fut>(snapshot: F) -> impl Stream<Item = Update<S>>
where
    F: Fn() -> Fut,
    Fut: Future<Output = io::Result<S>>,
{
    struct State<F> {
        snapshot: F,
        events: Option<broadcast::Receiver<Signal>>,
        stale: bool,
    }

    let state = State {
        snapshot,
        events: None,
        stale: true,
    };
    stream::unfold(state, |mut state| async move {
        // Subscribed before the snapshot is taken, so no event can fall in between.
        let events = state.events.get_or_insert_with(|| bus().subscribe());
        loop {
            if state.stale {
                match (state.snapshot)().await {
                    Ok(snapshot) => {
                        state.stale = false;
                        return Some((Update::Snapshot(snapshot), state));
                    }
                    Err(error) => eprintln!("eave-hyprland: snapshot failed: {error}"),
                }
            }
            match events.recv().await {
                // A pending snapshot will include this event, so it's dropped.
                Ok(Signal::Event(event)) if !state.stale => {
                    return Some((Update::Event(event), state));
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => state.stale = true,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// Sends a request on socket1 and returns the raw reply.
pub async fn request(command: &str) -> io::Result<String> {
    let mut socket = UnixStream::connect(socket_path(".socket.sock")?).await?;
    socket.write_all(command.as_bytes()).await?;
    let mut reply = String::new();
    socket.read_to_string(&mut reply).await?;
    Ok(reply)
}

/// Asks for JSON (`j/<what>`), like `query::<Vec<types::Workspace>>("workspaces")`.
pub async fn query<T: DeserializeOwned>(what: &str) -> io::Result<T> {
    let reply = request(&format!("j/{what}")).await?;
    serde_json::from_str(&reply).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Which config syntax this Hyprland speaks, found out on first use.
static SYNTAX: AtomicU8 = AtomicU8::new(UNKNOWN);
const UNKNOWN: u8 = 0;
const LUA: u8 = 1;
const LEGACY: u8 = 2;

/// Sends `lua` to a Hyprland with a Lua config (0.56 and later) or `legacy` to an older one,
/// both as whole socket1 requests, and expects `ok` back.
async fn request_either(lua: &str, legacy: &str) -> io::Result<()> {
    let run = async |command: &str| {
        let reply = request(command).await?;
        match reply.trim() {
            "ok" => Ok(()),
            error => Err(io::Error::other(format!("{command}: {error}"))),
        }
    };
    match SYNTAX.load(Ordering::Relaxed) {
        LUA => run(lua).await,
        LEGACY => run(legacy).await,
        _ => match run(lua).await {
            Ok(()) => {
                SYNTAX.store(LUA, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => {
                run(legacy).await?;
                SYNTAX.store(LEGACY, Ordering::Relaxed);
                Ok(())
            }
        },
    }
}

/// Runs a dispatcher, given in both syntaxes: Lua for Hyprland 0.56 and later, like
/// `hl.dsp.focus({ workspace = 3 })`, and the older one, like `workspace 3`. The first call finds
/// out which one this Hyprland speaks, and later calls use only that one.
pub async fn dispatch(lua: &str, legacy: &str) -> io::Result<()> {
    request_either(&format!("dispatch {lua}"), &format!("dispatch {legacy}")).await
}

/// The [`Event::Custom`] data sent on every primary click while [`report_clicks`] is on.
pub const CLICK: &str = "eave-click";

/// Turns on or off a non-consuming bind on the left mouse button that sends
/// `Event::Custom(CLICK)` on every click anywhere, which still reaches whatever was clicked.
/// Calls may overlap: whichever was made last wins.
pub async fn report_clicks(on: bool) -> io::Result<()> {
    // Replaces any bind left by an earlier eave that didn't get to remove it.
    const ON: &str = "eval if eave_click then pcall(function() eave_click:remove() end) end \
        eave_click = hl.bind(\"mouse:272\", hl.dsp.event(\"eave-click\"), { non_consuming = true })";
    const OFF: &str =
        "eval if eave_click then pcall(function() eave_click:remove() end) eave_click = nil end";
    static WANTED: AtomicBool = AtomicBool::new(false);
    /// What Hyprland has, if we set it. Held across the request, so requests don't interleave.
    static APPLIED: Mutex<Option<bool>> = Mutex::const_new(None);

    WANTED.store(on, Ordering::Relaxed);
    let mut applied = APPLIED.lock().await;
    let wanted = WANTED.load(Ordering::Relaxed);
    if *applied == Some(wanted) {
        return Ok(());
    }
    let result = if wanted {
        request_either(ON, "keyword bindn ,mouse:272,event,eave-click").await
    } else {
        request_either(OFF, "keyword unbind ,mouse:272").await
    };
    *applied = result.is_ok().then_some(wanted);
    result
}

#[derive(Debug, Clone)]
enum Signal {
    Event(Event),
    /// The event socket came back after a gap, so every watcher needs a new snapshot.
    Reconnected,
}

/// The one event reader, started by the first watcher.
fn bus() -> &'static broadcast::Sender<Signal> {
    static BUS: OnceLock<broadcast::Sender<Signal>> = OnceLock::new();
    BUS.get_or_init(|| {
        // Room for a burst (a workspace switch is a handful of events); a watcher that falls
        // further behind than this re-snapshots instead.
        let (sender, _) = broadcast::channel(64);
        tokio::spawn(read_events(sender.clone()));
        sender
    })
}

async fn read_events(bus: broadcast::Sender<Signal>) {
    const MAX_DELAY: Duration = Duration::from_secs(30);
    let mut delay = Duration::from_secs(1);
    let mut connected_before = false;

    loop {
        let path = match socket_path(".socket2.sock") {
            Ok(path) => path,
            Err(error) => {
                eprintln!("eave-hyprland: {error}; Hyprland widgets stay empty");
                return;
            }
        };
        match UnixStream::connect(&path).await {
            Ok(socket) => {
                delay = Duration::from_secs(1);
                if connected_before {
                    let _ = bus.send(Signal::Reconnected);
                }
                connected_before = true;

                let mut socket = BufReader::new(socket);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match socket.read_until(b'\n', &mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    // Window titles can be anything; a broken byte shouldn't drop the event.
                    let text = String::from_utf8_lossy(&line);
                    if let Some(event) = Event::parse(text.trim_end_matches('\n')) {
                        // No receivers is fine: every widget may have gone away.
                        let _ = bus.send(Signal::Event(event));
                    }
                }
                eprintln!("eave-hyprland: event socket closed, reconnecting");
            }
            Err(error) => eprintln!("eave-hyprland: {}: {error}", path.display()),
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_DELAY);
    }
}

fn socket_path(name: &str) -> io::Result<PathBuf> {
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HYPRLAND_INSTANCE_SIGNATURE is not set (not running under Hyprland?)",
        )
    })?;
    // Hyprland moved its sockets from /tmp/hypr to $XDG_RUNTIME_DIR/hypr in 0.40.
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("hypr"))
        .filter(|dir| dir.join(&signature).exists())
        .unwrap_or_else(|| PathBuf::from("/tmp/hypr"));
    Ok(runtime.join(signature).join(name))
}
