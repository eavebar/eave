//! Watches `config.toml` with inotify, so edits apply without a restart ([D-17](../../../docs/decisions.md)).
//!
//! The watch is on the directory, not the file: editors save by writing a temporary file and
//! renaming it over the old one, and the file may not exist yet. If the directory itself doesn't
//! exist, its parent is watched until it appears.

use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::path::Path;

use iced::Subscription;
use iced::futures::channel::mpsc::{self, UnboundedSender};
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use rustix::io::{Errno, Result};

use crate::config::{self, Config};

const DIR_NAME: &CStr = c"eave";
/// [`config::FILE`], as inotify reports it.
const FILE_NAME: &CStr = c"config.toml";

/// Writes that are complete (`CLOSE_WRITE`, not `MODIFY`, so a half-written file is never read),
/// renames in and out, deletion, and the directory itself going away.
const DIR_EVENTS: WatchFlags = WatchFlags::CLOSE_WRITE
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::DELETE)
    .union(WatchFlags::DELETE_SELF)
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR);
const PARENT_EVENTS: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::ONLYDIR);

/// Emits the freshly loaded config whenever `config.toml` may have changed. Reading and parsing
/// happen on the watcher's thread, so `update` does no I/O. An editor firing several events per
/// save sends several equal configs; the host ignores the ones that change nothing. The channel is
/// unbounded so a newer config is never dropped in favour of an older one.
pub fn config_changes() -> Subscription<std::result::Result<Config, String>> {
    Subscription::run(|| {
        let (sender, receiver) = mpsc::unbounded();
        if let Some(dir) = config::dir() {
            // inotify reads block, so they get their own thread instead of the UI's executor.
            let spawned = std::thread::Builder::new()
                .name("eave-watch".into())
                .spawn(move || {
                    if let Err(error) = watch(&dir, sender) {
                        eprintln!("eave: not watching {} for changes: {error}", dir.display());
                    }
                });
            if let Err(error) = spawned {
                eprintln!("eave: not watching the config for changes: {error}");
            }
        }
        receiver
    })
}

#[derive(Clone, Copy)]
enum Watching {
    /// The `eave` directory, by watch descriptor.
    Dir(i32),
    /// Its parent, until `eave` appears.
    Parent(i32),
}

/// Blocks forever, sending the config after every relevant event. Returns `Ok` once nobody listens.
fn watch(dir: &Path, changed: UnboundedSender<std::result::Result<Config, String>>) -> Result<()> {
    let parent = dir.parent().ok_or(Errno::INVAL)?;
    let file = dir.join(config::FILE);
    let fd = inotify::init(CreateFlags::CLOEXEC)?;
    let mut watching = arm(&fd, dir, parent)?;
    let mut buffer = [MaybeUninit::uninit(); 4096];
    let mut reader = inotify::Reader::new(&fd, &mut buffer);

    loop {
        let (wd, flags, is_config, is_dir_name) = {
            let event = reader.next()?;
            let name = event.file_name();
            (
                event.wd(),
                event.events(),
                name == Some(FILE_NAME),
                name == Some(DIR_NAME),
            )
        };

        let notify = match watching {
            // The queue overflowed: events were lost, so assume the worst.
            _ if flags.contains(ReadFlags::QUEUE_OVERFLOW) => true,
            Watching::Dir(current) if wd == current => {
                if flags.intersects(ReadFlags::MOVE_SELF | ReadFlags::IGNORED) {
                    // The directory was moved or deleted: watch for it to come back.
                    if flags.contains(ReadFlags::MOVE_SELF) {
                        let _ = inotify::remove_watch(&fd, current);
                    }
                    watching = arm(&fd, dir, parent)?;
                    true
                } else {
                    is_config
                }
            }
            Watching::Parent(current) if wd == current && is_dir_name => {
                watching = arm(&fd, dir, parent)?;
                if matches!(watching, Watching::Dir(_)) {
                    let _ = inotify::remove_watch(&fd, current);
                }
                // The directory may have arrived with a config already in it.
                true
            }
            // Leftovers from a watch we already replaced.
            _ => false,
        };

        if notify && changed.unbounded_send(config::load_from(&file)).is_err() {
            return Ok(());
        }
    }
}

/// Watches `dir` if it exists, otherwise `parent` until it does.
fn arm(fd: &impl rustix::fd::AsFd, dir: &Path, parent: &Path) -> Result<Watching> {
    match inotify::add_watch(fd, dir, DIR_EVENTS) {
        Ok(wd) => Ok(Watching::Dir(wd)),
        Err(Errno::NOENT | Errno::NOTDIR) => {
            inotify::add_watch(fd, parent, PARENT_EVENTS).map(Watching::Parent)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use iced::futures::channel::mpsc::UnboundedReceiver;

    use super::*;

    type Loaded = std::result::Result<Config, String>;

    /// The last config sent once the watcher has been quiet for a moment.
    fn settle(receiver: &mut UnboundedReceiver<Loaded>) -> Option<Loaded> {
        let mut last = None;
        let mut quiet_since = Instant::now();
        while quiet_since.elapsed() < Duration::from_millis(300) {
            match receiver.try_recv() {
                Ok(loaded) => {
                    last = Some(loaded);
                    quiet_since = Instant::now();
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        last
    }

    fn widgets(loaded: Option<Loaded>) -> Vec<String> {
        let config = loaded.expect("no event").expect("config didn't parse");
        config
            .entries
            .into_iter()
            .map(|entry| entry.widget)
            .collect()
    }

    #[test]
    fn follows_writes_renames_errors_and_the_directory_coming_and_going() {
        let root = std::env::temp_dir().join(format!("eave-watch-test-{}", std::process::id()));
        let dir = root.join("eave");
        let file = dir.join(config::FILE);
        std::fs::create_dir_all(&dir).unwrap();

        let (sender, mut receiver) = mpsc::unbounded();
        let watched = dir.clone();
        std::thread::spawn(move || watch(&watched, sender));
        std::thread::sleep(Duration::from_millis(100));

        // A plain write.
        std::fs::write(&file, "[[left]]\nwidget = \"a\"").unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["a"]);

        // An editor-style save: write elsewhere, rename over.
        let temp = dir.join(".config.toml.swp");
        std::fs::write(&temp, "[[left]]\nwidget = \"b\"").unwrap();
        std::fs::rename(&temp, &file).unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["b"]);

        // Other files in the directory are ignored.
        std::fs::write(dir.join("plugins.toml"), "").unwrap();
        assert!(settle(&mut receiver).is_none());

        // A syntax error arrives as an error, for the host to keep the current bar.
        std::fs::write(&file, "[[left").unwrap();
        assert!(settle(&mut receiver).unwrap().is_err());

        // The directory goes away (an empty bar), then comes back with a config in it.
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(widgets(settle(&mut receiver)).is_empty());
        std::fs::create_dir(&dir).unwrap();
        settle(&mut receiver);
        std::fs::write(&file, "[[left]]\nwidget = \"c\"").unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["c"]);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
