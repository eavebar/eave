//! Check health: runs `eave doctor --report`, waits for the report it writes, and reads it
//! (D-96, D-98).

use std::io;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
use serde::Deserialize;

/// How long eave doctor gets. It asks Hyprland and systemd a few things; it takes well under a
/// second unless one of them hangs.
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Good,
    Warning,
    Problem,
}

/// One thing eave doctor checked.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Check {
    pub level: Level,
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    check: Vec<Check>,
}

/// Runs eave doctor and gives back what it found.
pub async fn check() -> Result<Vec<Check>, String> {
    let (watch, path) = tokio::task::spawn_blocking(prepare)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("can't wait for the report: {error}"))?;
    let report = path.to_string_lossy().into_owned();
    crate::run(&format!("doctor --report {}", crate::quoted(&report))).await?;
    tokio::task::spawn_blocking(move || {
        match wait(&watch, &path) {
            Ok(true) => {}
            Ok(false) => return Err("eave doctor didn't answer in time".to_owned()),
            Err(error) => return Err(format!("waiting for the report: {error}")),
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        parse(&text)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Where the report goes, with the last one gone, and a watch on its directory, set before eave
/// doctor runs so its report can't slip by.
fn prepare() -> io::Result<(OwnedFd, PathBuf)> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("eave");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("doctor.toml");
    match std::fs::remove_file(&path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let watch = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
    // eave doctor writes it beside, then renames it into place.
    inotify::add_watch(&watch, &dir, WatchFlags::MOVED_TO | WatchFlags::CLOSE_WRITE)?;
    Ok((watch, path))
}

/// Blocks until `path` is there, or [`TIMEOUT`] passes: false then.
fn wait(watch: &OwnedFd, path: &Path) -> io::Result<bool> {
    let deadline = Instant::now() + TIMEOUT;
    let mut buffer = [0u8; 4096];
    loop {
        if path.exists() {
            return Ok(true);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        let timeout = Timespec {
            tv_sec: left.as_secs() as _,
            tv_nsec: left.subsec_nanos() as _,
        };
        let mut fds = [PollFd::new(watch, PollFlags::IN)];
        rustix::event::poll(&mut fds, Some(&timeout))?;
        // Whatever the events were, the file's being there is what counts.
        match rustix::io::read(watch, &mut buffer) {
            Ok(_) | Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn parse(text: &str) -> Result<Vec<Check>, String> {
    toml::from_str::<Report>(text)
        .map(|report| report.check)
        .map_err(|error| format!("can't read eave doctor's report: {error}"))
}

/// Problems first, then warnings, then what's fine, each in the order doctor checked them.
pub fn ordered(mut checks: Vec<Check>) -> Vec<Check> {
    checks.sort_by_key(|check| std::cmp::Reverse(check.level));
    checks
}

/// What the dialog's header says under "Health".
pub fn summary(checks: &[Check]) -> String {
    let count = |level| checks.iter().filter(|check| check.level == level).count();
    let plural = |count: usize, one: &str, many: &str| match count {
        1 => format!("1 {one}"),
        _ => format!("{count} {many}"),
    };
    match (count(Level::Problem), count(Level::Warning)) {
        (0, 0) => "All good".to_owned(),
        (0, warnings) => plural(warnings, "warning", "warnings"),
        (problems, 0) => plural(problems, "problem", "problems"),
        (problems, warnings) => format!(
            "{}, {}",
            plural(problems, "problem", "problems"),
            plural(warnings, "warning", "warnings")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As eave doctor writes it (`crates/eave/src/doctor/report.rs`).
    const REPORT: &str = r#"
[[check]]
level = "good"
title = "config.toml reads"
detail = "/home/me/.config/eave/config.toml"

[[check]]
level = "warning"
title = "eDP-1's notch isn't measured"
detail = "The bar guesses how wide it is."

[[check]]
level = "problem"
title = "`weather` isn't in this build"
detail = ""
"#;

    #[test]
    fn reads_doctors_report_problems_first() {
        let checks = ordered(parse(REPORT).unwrap());
        let levels: Vec<_> = checks.iter().map(|check| check.level).collect();
        assert_eq!(levels, [Level::Problem, Level::Warning, Level::Good]);
        assert_eq!(checks[2].detail, "/home/me/.config/eave/config.toml");
        assert!(parse("[[check]]\nlevel = \"odd\"\ntitle = \"x\"").is_err());
        assert_eq!(parse("").unwrap(), []);
    }

    #[test]
    fn the_summary_counts_what_needs_doing() {
        let checks = parse(REPORT).unwrap();
        assert_eq!(summary(&checks), "1 problem, 1 warning");
        assert_eq!(summary(&checks[..1]), "All good");
        assert_eq!(summary(&checks[..2]), "1 warning");
        let two = [checks[2].clone(), checks[2].clone()];
        assert_eq!(summary(&two), "2 problems");
    }

    #[test]
    fn waits_for_the_report_to_be_renamed_into_place() {
        let dir = std::env::temp_dir().join(format!("eave-essential-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doctor.toml");
        let watch = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK).unwrap();
        inotify::add_watch(&watch, &dir, WatchFlags::MOVED_TO).unwrap();
        let writer = {
            let (dir, path) = (dir.clone(), path.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                let staged = dir.join("doctor.toml.new");
                std::fs::write(&staged, REPORT).unwrap();
                std::fs::rename(staged, path).unwrap();
            })
        };
        assert!(wait(&watch, &path).unwrap());
        writer.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
