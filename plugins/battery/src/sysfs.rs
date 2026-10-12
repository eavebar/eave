//! Batteries straight from the kernel: `/sys/class/power_supply`, re-read on the kernel's
//! `power_supply` uevents. No UPower ([D-23](https://github.com/francescomengarelli/eave/blob/main/docs/decisions.md)).
//!
//! One thread serves every widget. It blocks on the uevent socket, so plugging in the charger
//! shows at once. Drivers don't send an event for every percent, so while there's a battery it
//! also re-reads every 30 s. `/sys` is the one thing we're allowed to poll.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::{Stream, StreamExt, stream};
use rustix::fd::OwnedFd;
use rustix::net::{self, AddressFamily, SocketType, netlink};
use tokio::sync::watch as channel;

use crate::{Reading, State};

const ROOT: &str = "/sys/class/power_supply";
const REFRESH: Duration = Duration::from_secs(30);

/// The reading now, then again whenever it changes.
pub fn watch() -> impl Stream<Item = Reading> {
    let receiver = readings().subscribe();
    stream::unfold((receiver, true), |(mut receiver, first)| async move {
        if !first {
            receiver.changed().await.ok()?;
        }
        let reading = receiver.borrow_and_update().clone();
        Some((reading, (receiver, false)))
    })
    .boxed()
}

fn readings() -> &'static channel::Sender<Reading> {
    static READINGS: OnceLock<channel::Sender<Reading>> = OnceLock::new();
    READINGS.get_or_init(|| {
        let (sender, _) = channel::channel(Reading::default());
        let readings = sender.clone();
        std::thread::Builder::new()
            .name("eave-battery".into())
            .spawn(move || listen(&readings))
            .expect("spawning a thread");
        sender
    })
}

fn listen(readings: &channel::Sender<Reading>) {
    let socket = uevents()
        .inspect_err(|error| eprintln!("eave-battery: uevent socket: {error}, polling only"))
        .ok();
    let mut buffer = vec![0u8; 8192];
    loop {
        let reading = read(Path::new(ROOT));
        // Without a battery there's nothing to refresh; a new one arrives as a uevent.
        let refresh = (reading.present || socket.is_none()).then_some(REFRESH);
        readings.send_if_modified(|current| {
            let changed = *current != reading;
            *current = reading;
            changed
        });

        let Some(socket) = &socket else {
            std::thread::sleep(REFRESH);
            continue;
        };
        let deadline = refresh.map(|refresh| Instant::now() + refresh);
        loop {
            let timeout = deadline.map(|deadline| {
                // A zero timeout means "block forever" to the kernel.
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1))
            });
            if net::sockopt::set_socket_timeout(socket, net::sockopt::Timeout::Recv, timeout)
                .is_err()
            {
                std::thread::sleep(REFRESH);
                break;
            }
            match net::recv(socket, &mut buffer, net::RecvFlags::empty()) {
                Ok((length, _)) if is_power_supply(&buffer[..length]) => break,
                Ok(_) => {}
                // Timed out (or interrupted): time for the periodic re-read.
                Err(_) => break,
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

/// A uevent is `action@devpath`, then `KEY=value` fields, all NUL-separated.
fn is_power_supply(event: &[u8]) -> bool {
    event
        .split(|&byte| byte == 0)
        .any(|field| field == b"SUBSYSTEM=power_supply")
}

/// One battery's state, in watt-hours and watts.
#[derive(Debug, Default)]
struct Supply {
    state: State,
    capacity: Option<f64>,
    energy: Option<f64>,
    energy_full: Option<f64>,
    power: Option<f64>,
    time_to_empty: Option<i64>,
    time_to_full: Option<i64>,
}

/// Every system battery under `root`, combined into one reading the way UPower's display device
/// does. Peripherals (a mouse's battery) have `scope = Device` and are skipped.
pub(crate) fn read(root: &Path) -> Reading {
    let Ok(entries) = fs::read_dir(root) else {
        return Reading::default();
    };
    let mut supplies: Vec<Supply> = entries
        .filter_map(|entry| supply(&entry.ok()?.path()))
        .collect();
    // read_dir order isn't stable; the combination shouldn't depend on it.
    supplies.sort_by(|a, b| {
        a.energy_full
            .partial_cmp(&b.energy_full)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    combine(&supplies)
}

fn supply(dir: &Path) -> Option<Supply> {
    let text = |name: &str| {
        fs::read_to_string(dir.join(name))
            .ok()
            .map(|s| s.trim().to_owned())
    };
    let number = |name: &str| text(name)?.parse::<f64>().ok();

    if text("type")? != "Battery" || text("scope").as_deref() == Some("Device") {
        return None;
    }
    if text("present").as_deref() == Some("0") {
        return None;
    }

    // Charge (µAh) and current (µA) need a voltage to become energy and power.
    let voltage = number("voltage_now").filter(|v| *v > 0.0);
    let design_voltage = number("voltage_min_design")
        .filter(|v| *v > 0.0)
        .or(voltage);
    let energy = |name: &str, charge: &str| {
        number(name)
            .map(|uwh| uwh / 1e6)
            .or_else(|| Some(number(charge)? * design_voltage? / 1e12))
    };
    let power = number("power_now")
        .map(|uw| uw.abs() / 1e6)
        .or_else(|| Some((number("current_now")? * voltage? / 1e12).abs()));
    let seconds = |name: &str| number(name).map(|s| s as i64).filter(|s| *s > 0);

    Some(Supply {
        state: State::from_sysfs(&text("status").unwrap_or_default()),
        capacity: number("capacity"),
        energy: energy("energy_now", "charge_now"),
        energy_full: energy("energy_full", "charge_full"),
        power,
        time_to_empty: seconds("time_to_empty_now"),
        time_to_full: seconds("time_to_full_now"),
    })
}

fn combine(supplies: &[Supply]) -> Reading {
    let [first, rest @ ..] = supplies else {
        return Reading::default();
    };
    let any = |state| supplies.iter().any(|s| s.state == state);
    let state = if any(State::Charging) {
        State::Charging
    } else if any(State::Discharging) {
        State::Discharging
    } else if supplies.iter().all(|s| s.state == State::Full) {
        State::Full
    } else if any(State::NotCharging) || any(State::Full) {
        State::NotCharging
    } else {
        first.state
    };

    let sum =
        |field: fn(&Supply) -> Option<f64>| -> Option<f64> { supplies.iter().map(field).sum() };
    let energy = sum(|s| s.energy);
    let energy_full = sum(|s| s.energy_full).filter(|full| *full > 0.0);
    let power = sum(|s| s.power).unwrap_or(0.0);

    // Stored energy over full energy, as UPower computes it, so the percentage and the time left
    // agree. `capacity` can differ: on Apple Silicon it's the firmware's rounded-up display
    // value (98% where the energy says 93%). It's only the fallback.
    let percentage = match (energy, energy_full) {
        (Some(energy), Some(full)) => energy / full * 100.0,
        _ => {
            let capacities: Vec<f64> = supplies.iter().filter_map(|s| s.capacity).collect();
            capacities.iter().sum::<f64>() / capacities.len().max(1) as f64
        }
    };

    let hours_to = |target: Option<f64>| -> i64 {
        match (target, power > 0.1) {
            (Some(wh), true) => (wh / power * 3600.0) as i64,
            _ => 0,
        }
    };
    let estimate =
        |field: fn(&Supply) -> Option<i64>, fallback: i64| match (rest.is_empty(), field(first)) {
            (true, Some(seconds)) => seconds,
            _ => fallback,
        };
    // Past a few days the estimate means nothing (a nearly idle battery).
    let sane = |seconds: i64| if seconds < 5 * 24 * 3600 { seconds } else { 0 };
    let (time_to_empty, time_to_full) = match state {
        State::Discharging => (sane(estimate(|s| s.time_to_empty, hours_to(energy))), 0),
        State::Charging => {
            let missing = energy
                .zip(energy_full)
                .map(|(now, full)| (full - now).max(0.0));
            (0, sane(estimate(|s| s.time_to_full, hours_to(missing))))
        }
        _ => (0, 0),
    };

    Reading {
        present: true,
        percentage: percentage.clamp(0.0, 100.0),
        state,
        time_to_empty,
        time_to_full,
        energy_rate: power,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("eave-battery-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn supply(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        for (file, value) in files {
            fs::write(dir.join(file), format!("{value}\n")).unwrap();
        }
    }

    #[test]
    fn no_battery_is_absent() {
        let root = root("none");
        supply(&root, "AC", &[("type", "Mains"), ("online", "1")]);
        assert!(!read(&root).present);
        assert!(!read(&root.join("missing")).present);
    }

    #[test]
    fn reads_a_charge_battery() {
        // macsmc-battery on Asahi: charge_* in µAh, power_now in µW.
        let root = root("charge");
        supply(
            &root,
            "macsmc-battery",
            &[
                ("type", "Battery"),
                ("scope", "System"),
                ("status", "Discharging"),
                ("capacity", "98"),
                ("charge_now", "4922000"),
                ("charge_full", "5097000"),
                ("voltage_min_design", "9000000"),
                ("voltage_now", "12793000"),
                ("power_now", "8405000"),
            ],
        );
        let reading = read(&root);
        assert!(reading.present);
        assert_eq!(reading.state, State::Discharging);
        assert!((reading.percentage - 4922.0 / 5097.0 * 100.0).abs() < 1e-9);
        assert!((reading.energy_rate - 8.405).abs() < 1e-6);
        // 4.922 Ah × 9 V = 44.3 Wh at 8.4 W.
        assert_eq!(reading.time_to_empty, (44.298 / 8.405 * 3600.0) as i64);
    }

    #[test]
    fn prefers_the_kernels_estimate() {
        let root = root("estimate");
        supply(
            &root,
            "BAT0",
            &[
                ("type", "Battery"),
                ("status", "Charging"),
                ("capacity", "50"),
                ("energy_now", "25000000"),
                ("energy_full", "50000000"),
                ("power_now", "10000000"),
                ("time_to_full_now", "4000"),
            ],
        );
        let reading = read(&root);
        assert_eq!(reading.state, State::Charging);
        assert_eq!(reading.time_to_full, 4000);
        assert_eq!(reading.time_to_empty, 0);
    }

    #[test]
    fn combines_two_batteries_by_energy() {
        let root = root("two");
        let battery = |status, now: &str, full: &str| {
            vec![
                ("type", "Battery"),
                ("status", status),
                ("energy_now", now),
                ("energy_full", full),
                ("power_now", "5000000"),
            ]
            .into_iter()
            .map(|(k, v)| (k, v.to_owned()))
            .collect::<Vec<_>>()
        };
        for (name, files) in [
            ("BAT0", battery("Discharging", "10000000", "20000000")),
            ("BAT1", battery("Not charging", "70000000", "80000000")),
        ] {
            let files: Vec<(&str, &str)> = files.iter().map(|(k, v)| (*k, v.as_str())).collect();
            supply(&root, name, &files);
        }
        let reading = read(&root);
        assert_eq!(reading.state, State::Discharging);
        assert_eq!(reading.percentage, 80.0);
        assert!((reading.energy_rate - 10.0).abs() < 1e-6);
        assert_eq!(reading.time_to_empty, 8 * 3600);
    }

    #[test]
    fn falls_back_to_capacity() {
        let root = root("capacity");
        supply(
            &root,
            "BAT0",
            &[("type", "Battery"), ("status", "Full"), ("capacity", "100")],
        );
        let reading = read(&root);
        assert_eq!(reading.percentage, 100.0);
        assert_eq!(reading.state, State::Full);
    }

    #[test]
    fn skips_peripherals() {
        let root = root("peripheral");
        supply(
            &root,
            "hidpp_battery_0",
            &[("type", "Battery"), ("scope", "Device"), ("capacity", "40")],
        );
        assert!(!read(&root).present);
    }

    #[test]
    fn recognises_power_supply_events() {
        assert!(is_power_supply(
            b"change@/devices/platform/macsmc-battery\0ACTION=change\0SUBSYSTEM=power_supply\0"
        ));
        assert!(!is_power_supply(
            b"add@/devices/usb1\0ACTION=add\0SUBSYSTEM=usb\0"
        ));
    }
}
