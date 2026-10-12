//! Audio for eave plugins, over the PulseAudio native protocol. On PipeWire this is served by
//! `pipewire-pulse`, which every desktop setup runs ([D-19](https://github.com/francescomengarelli/eave/blob/main/docs/decisions.md)).
//!
//! One thread holds a subscription to sink and server events for every widget. Reads and commands
//! each use a short-lived connection on tokio's blocking pool, so the UI thread never waits on
//! the socket.

use std::ffi::CString;
use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::OnceLock;
use std::time::Duration;

use futures_util::{FutureExt, StreamExt, stream};
use pulseaudio::protocol::{self, ChannelVolume, Command, Volume};
use tokio::sync::broadcast;

pub use futures_util::Stream;

/// The output side of the audio server.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outputs {
    pub sinks: Vec<Sink>,
    /// Index into `sinks` of the default sink.
    pub default: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub index: u32,
    pub description: String,
    /// Percent of normal volume, per channel: 100 is 0 dB. Can go above 100.
    pub volume: u32,
    pub muted: bool,
    channels: u8,
}

/// A stereo sink at `volume` percent, for plugin tests.
#[doc(hidden)]
pub fn test_sink(volume: u32) -> Sink {
    Sink {
        index: 0,
        description: "Speakers".into(),
        volume,
        muted: false,
        channels: 2,
    }
}

impl Outputs {
    pub fn default_sink(&self) -> Option<&Sink> {
        self.default.and_then(|index| self.sinks.get(index))
    }
}

/// The outputs, then again after every change on the server. Unchanged readings are skipped.
pub fn watch() -> impl Stream<Item = Outputs> {
    struct State {
        events: Option<broadcast::Receiver<()>>,
        last: Option<Outputs>,
        stale: bool,
    }

    let state = State {
        events: None,
        last: None,
        stale: true,
    };
    stream::unfold(state, |mut state| async move {
        // Subscribed before reading, so a change during the read still marks it stale.
        let events = state.events.get_or_insert_with(|| bus().subscribe());
        loop {
            if state.stale {
                state.stale = false;
                match outputs().await {
                    Ok(outputs) if state.last.as_ref() != Some(&outputs) => {
                        state.last = Some(outputs.clone());
                        return Some((outputs, state));
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("eave-pulse: reading outputs: {error}"),
                }
            }
            match events.recv().await {
                Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    // One volume drag is dozens of events; read once for the lot.
                    while let Some(Ok(())) = events.recv().now_or_never() {}
                    state.stale = true;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .boxed()
}

/// Sets every channel of `sink` to `percent` of normal volume.
pub async fn set_volume(sink: &Sink, percent: u32) -> io::Result<()> {
    let raw = (u64::from(Volume::NORM.as_u32()) * u64::from(percent) / 100) as u32;
    let mut volume = ChannelVolume::empty();
    for _ in 0..sink.channels.max(1) {
        volume.push(Volume::from_u32_clamped(raw));
    }
    command(Command::SetSinkVolume(protocol::SetDeviceVolumeParams {
        device_index: Some(sink.index),
        device_name: None,
        volume,
    }))
    .await
}

pub async fn set_mute(sink: &Sink, mute: bool) -> io::Result<()> {
    command(Command::SetSinkMute(protocol::SetDeviceMuteParams {
        device_index: Some(sink.index),
        device_name: None,
        mute,
    }))
    .await
}

pub async fn set_default(sink: &Sink) -> io::Result<()> {
    // The server takes a name, which we don't keep; look it up by index.
    let index = sink.index;
    blocking(move || {
        let (mut socket, version) = connect()?;
        let info: protocol::SinkInfo = roundtrip(
            &mut socket,
            version,
            2,
            Command::GetSinkInfo(protocol::GetSinkInfo {
                index: Some(index),
                name: None,
            }),
        )?;
        ack(&mut socket, version, 3, Command::SetDefaultSink(info.name))
    })
    .await
}

async fn outputs() -> io::Result<Outputs> {
    blocking(|| {
        let (mut socket, version) = connect()?;
        let server: protocol::ServerInfo =
            roundtrip(&mut socket, version, 2, Command::GetServerInfo)?;
        let sinks: protocol::SinkInfoList =
            roundtrip(&mut socket, version, 3, Command::GetSinkInfoList)?;
        Ok(read_outputs(server.default_sink_name.as_ref(), sinks))
    })
    .await
}

fn read_outputs(default: Option<&CString>, sinks: protocol::SinkInfoList) -> Outputs {
    let default = default.and_then(|name| sinks.iter().position(|sink| sink.name == *name));
    let sinks = sinks
        .into_iter()
        .map(|sink| {
            let channels = sink.cvolume.channels();
            Sink {
                index: sink.index,
                description: sink
                    .description
                    .as_ref()
                    .unwrap_or(&sink.name)
                    .to_string_lossy()
                    .into_owned(),
                volume: percent(channels),
                muted: sink.muted,
                channels: channels.len() as u8,
            }
        })
        .collect();
    Outputs { sinks, default }
}

/// The loudest channel, as a rounded percent of normal volume, the way `pactl` reports it.
fn percent(channels: &[Volume]) -> u32 {
    let raw = channels.iter().map(Volume::as_u32).max().unwrap_or(0);
    let norm = Volume::NORM.as_u32();
    ((u64::from(raw) * 100 + u64::from(norm) / 2) / u64::from(norm)) as u32
}

async fn command(command: Command) -> io::Result<()> {
    blocking(move || {
        let (mut socket, version) = connect()?;
        ack(&mut socket, version, 2, command)
    })
    .await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(io::Error::other)?
}

type Socket = BufReader<UnixStream>;

/// A new, authenticated connection, and the protocol version it speaks.
fn connect() -> io::Result<(Socket, u16)> {
    let path = pulseaudio::socket_path_from_env()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no PulseAudio socket"))?;
    let mut socket = BufReader::new(UnixStream::connect(path)?);
    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    let auth = protocol::AuthParams {
        version: protocol::MAX_VERSION,
        supports_shm: false,
        supports_memfd: false,
        cookie,
    };
    let reply: protocol::AuthReply =
        roundtrip(&mut socket, protocol::MAX_VERSION, 0, Command::Auth(auth))?;
    let version = protocol::MAX_VERSION.min(reply.version);

    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, c"eave");
    let _: protocol::SetClientNameReply =
        roundtrip(&mut socket, version, 1, Command::SetClientName(props))?;
    Ok((socket, version))
}

fn roundtrip<R: protocol::CommandReply>(
    socket: &mut Socket,
    version: u16,
    seq: u32,
    command: Command,
) -> io::Result<R> {
    protocol::write_command_message(socket.get_mut(), seq, &command, version)
        .map_err(protocol_error)?;
    let (_, reply) = protocol::read_reply_message::<R>(socket, version).map_err(protocol_error)?;
    Ok(reply)
}

fn ack(socket: &mut Socket, version: u16, seq: u32, command: Command) -> io::Result<()> {
    protocol::write_command_message(socket.get_mut(), seq, &command, version)
        .map_err(protocol_error)?;
    protocol::read_ack_message(socket).map_err(protocol_error)?;
    Ok(())
}

fn protocol_error(error: protocol::ProtocolError) -> io::Error {
    io::Error::other(error)
}

/// The one subscription, started by the first watcher. Each message means "something changed".
fn bus() -> &'static broadcast::Sender<()> {
    static BUS: OnceLock<broadcast::Sender<()>> = OnceLock::new();
    BUS.get_or_init(|| {
        let (sender, _) = broadcast::channel(16);
        let events = sender.clone();
        std::thread::Builder::new()
            .name("eave-pulse".into())
            .spawn(move || listen(&events))
            .expect("spawning a thread");
        sender
    })
}

/// Blocks on the event socket, forever. Reconnects with a growing delay if the server goes away.
fn listen(events: &broadcast::Sender<()>) {
    const MAX_DELAY: Duration = Duration::from_secs(30);
    let mut delay = Duration::from_secs(1);
    loop {
        match subscribe() {
            Ok((mut socket, version)) => {
                delay = Duration::from_secs(1);
                // Whatever happened while we were away.
                let _ = events.send(());
                loop {
                    match protocol::read_command_message(&mut socket, version) {
                        Ok((_, Command::SubscribeEvent(_))) => {
                            let _ = events.send(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("eave-pulse: event socket: {error}, reconnecting");
                            break;
                        }
                    }
                }
            }
            Err(error) => eprintln!("eave-pulse: {error}"),
        }
        std::thread::sleep(delay);
        delay = (delay * 2).min(MAX_DELAY);
    }
}

fn subscribe() -> io::Result<(Socket, u16)> {
    let (mut socket, version) = connect()?;
    let mask = protocol::SubscriptionMask::SINK | protocol::SubscriptionMask::SERVER;
    ack(&mut socket, version, 2, Command::Subscribe(mask))?;
    Ok((socket, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_like_pactl() {
        let norm = Volume::NORM.as_u32();
        assert_eq!(percent(&[Volume::from_u32_clamped(norm)]), 100);
        assert_eq!(percent(&[Volume::from_u32_clamped(norm * 45 / 100)]), 45);
        assert_eq!(
            percent(&[
                Volume::from_u32_clamped(norm / 2),
                Volume::from_u32_clamped(norm)
            ]),
            100
        );
        assert_eq!(percent(&[]), 0);
    }
}
