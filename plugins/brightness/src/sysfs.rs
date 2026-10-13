//! Backlights straight from the kernel: `/sys/class/backlight` and the keyboard's LED in
//! `/sys/class/leds`, re-read on the kernel's uevents, and set through logind
//! ([D-77](https://github.com/eaveshell/eave/blob/main/docs/decisions.md)).
//!
//! One thread serves every widget. A backlight sends a uevent whenever its level is written,
//! whoever writes it (us, a brightness key's binding, brightnessctl), so it shows at once and
//! nothing is polled. LEDs send none, so the widget re-reads the keyboard's itself while its
//! popover is open.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use futures_util::{Stream, StreamExt, stream};
use rustix::fd::OwnedFd;
use rustix::net::{self, AddressFamily, SocketType, netlink};
use tokio::sync::watch as channel;

const BACKLIGHTS: &str = "/sys/class/backlight";
const LEDS: &str = "/sys/class/leds";
/// How often to re-read without a uevent socket.
const POLL: Duration = Duration::from_secs(2);

/// Where a light lives: logind and sysfs both call it this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Backlight,
    Leds,
}

impl Subsystem {
    fn name(self) -> &'static str {
        match self {
            Subsystem::Backlight => "backlight",
            Subsystem::Leds => "leds",
        }
    }
}

/// A light and its level, raw as the driver counts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Light {
    pub subsystem: Subsystem,
    pub name: String,
    pub raw: u32,
    pub max: u32,
}

/// Every light there is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lights {
    /// Screen backlights, the one to use first: firmware, then platform, then raw ones.
    pub backlights: Vec<Light>,
    /// The keyboard's backlight, if it has one.
    pub keyboard: Option<Light>,
}

/// The lights now, then again whenever they change.
pub fn watch() -> impl Stream<Item = Lights> {
    let receiver = lights().subscribe();
    stream::unfold((receiver, true), |(mut receiver, first)| async move {
        if !first {
            receiver.changed().await.ok()?;
        }
        let lights = receiver.borrow_and_update().clone();
        Some((lights, (receiver, false)))
    })
    .boxed()
}

/// The keyboard's backlight as it is now. A single small sysfs read.
pub fn keyboard() -> Option<Light> {
    read_keyboard(Path::new(LEDS))
}

/// Sets `light` to `raw` through logind, which may write the session's own backlights without
/// root.
pub async fn set(light: &Light, raw: u32) -> io::Result<()> {
    let connection = eave_dbus::system().await.map_err(io::Error::other)?;
    connection
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &(light.subsystem.name(), light.name.as_str(), raw),
        )
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

fn lights() -> &'static channel::Sender<Lights> {
    static LIGHTS: OnceLock<channel::Sender<Lights>> = OnceLock::new();
    LIGHTS.get_or_init(|| {
        let (sender, _) = channel::channel(Lights::default());
        let lights = sender.clone();
        std::thread::Builder::new()
            .name("eave-brightness".into())
            .spawn(move || listen(&lights))
            .expect("spawning a thread");
        sender
    })
}

fn listen(lights: &channel::Sender<Lights>) {
    let socket = uevents()
        .inspect_err(|error| eprintln!("eave-brightness: uevent socket: {error}, polling"))
        .ok();
    let mut buffer = vec![0u8; 8192];
    loop {
        let reading = read(Path::new(BACKLIGHTS), Path::new(LEDS));
        lights.send_if_modified(|current| {
            let changed = *current != reading;
            *current = reading;
            changed
        });

        let Some(socket) = &socket else {
            std::thread::sleep(POLL);
            continue;
        };
        loop {
            match net::recv(socket, &mut buffer, net::RecvFlags::empty()) {
                Ok((length, _)) if is_light(&buffer[..length]) => break,
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => {
                    eprintln!("eave-brightness: uevent socket: {error}");
                    std::thread::sleep(POLL);
                    break;
                }
            }
        }
    }
}

/// A socket that receives the kernel's uevents. Unprivileged processes may listen.
fn uevents() -> rustix::io::Result<OwnedFd> {
    let socket = net::socket(
        AddressFamily::NETLINK,
        SocketType::DGRAM,
        Some(netlink::KOBJECT_UEVENT),
    )?;
    // Group 1 is the kernel's own broadcast (udev rebroadcasts on group 2).
    net::bind(&socket, &netlink::SocketAddrNetlink::new(0, 1))?;
    Ok(socket)
}

/// A uevent is `action@devpath`, then `KEY=value` fields, all NUL-separated. LEDs only send
/// one when they come or go, which is worth a re-read too.
fn is_light(event: &[u8]) -> bool {
    event
        .split(|&byte| byte == 0)
        .any(|field| field == b"SUBSYSTEM=backlight" || field == b"SUBSYSTEM=leds")
}

fn read(backlights: &Path, leds: &Path) -> Lights {
    let mut ranked: Vec<(u8, Light)> = entries(backlights)
        .filter_map(|(name, dir)| {
            let rank = match attribute(&dir, "type").as_deref() {
                Some("firmware") => 0,
                Some("platform") => 1,
                _ => 2,
            };
            Some((rank, light(Subsystem::Backlight, name, &dir)?))
        })
        .collect();
    ranked.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.name.cmp(&y.name)));
    Lights {
        backlights: ranked.into_iter().map(|(_, light)| light).collect(),
        keyboard: read_keyboard(leds),
    }
}

fn read_keyboard(leds: &Path) -> Option<Light> {
    let mut keyboards: Vec<Light> = entries(leds)
        .filter(|(name, _)| name.ends_with("kbd_backlight"))
        .filter_map(|(name, dir)| light(Subsystem::Leds, name, &dir))
        .collect();
    keyboards.sort_by(|a, b| a.name.cmp(&b.name));
    keyboards.into_iter().next()
}

fn entries(root: &Path) -> impl Iterator<Item = (String, std::path::PathBuf)> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
}

fn light(subsystem: Subsystem, name: String, dir: &Path) -> Option<Light> {
    let max = attribute(dir, "max_brightness")?.parse().ok()?;
    let raw = attribute(dir, "brightness")?.parse().ok()?;
    (max > 0).then_some(Light {
        subsystem,
        name,
        raw,
        max,
    })
}

fn attribute(dir: &Path, name: &str) -> Option<String> {
    Some(fs::read_to_string(dir.join(name)).ok()?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("eave-brightness-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn device(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        for (file, value) in files {
            fs::write(dir.join(file), format!("{value}\n")).unwrap();
        }
    }

    #[test]
    fn firmware_backlights_come_first_and_the_keyboard_is_found() {
        let root = temp("rank");
        let (backlights, leds) = (root.join("backlight"), root.join("leds"));
        fs::create_dir_all(&leds).unwrap();
        let light = |kind| {
            [
                ("type", kind),
                ("brightness", "10"),
                ("max_brightness", "100"),
            ]
        };
        device(&backlights, "intel_backlight", &light("raw"));
        device(&backlights, "acpi_video0", &light("firmware"));
        device(&backlights, "apple-panel-bl", &light("platform"));
        device(&backlights, "broken", &[("type", "raw")]);
        device(
            &leds,
            "input3::capslock",
            &[("brightness", "0"), ("max_brightness", "1")],
        );
        device(
            &leds,
            "kbd_backlight",
            &[("brightness", "50"), ("max_brightness", "255")],
        );

        let lights = read(&backlights, &leds);
        let names: Vec<_> = lights.backlights.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["acpi_video0", "apple-panel-bl", "intel_backlight"]);
        assert_eq!(
            lights.keyboard,
            Some(Light {
                subsystem: Subsystem::Leds,
                name: "kbd_backlight".into(),
                raw: 50,
                max: 255,
            })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nothing_there_is_no_lights() {
        let root = temp("none");
        assert_eq!(
            read(&root.join("backlight"), &root.join("leds")),
            Lights::default()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_light_uevents_count() {
        let event = |subsystem: &str| {
            format!("change@/devices/x\0ACTION=change\0SUBSYSTEM={subsystem}\0SEQNUM=1\0")
        };
        assert!(is_light(event("backlight").as_bytes()));
        assert!(is_light(event("leds").as_bytes()));
        assert!(!is_light(event("power_supply").as_bytes()));
    }
}
