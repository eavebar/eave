//! Watches `config.toml` with inotify, so edits apply without a restart ([D-21](../../../docs/decisions.md)).
//!
//! The watch is on the directory, not the file: editors save by writing a temporary file and
//! renaming it over the old one, and the file may not exist yet. If the directory itself doesn't
//! exist, its parent is watched until it appears.
//!
//! A symlinked `config.toml` (a dotfiles manager's) is followed: the directory of the file it
//! points to is watched too, since writes there never reach the link's directory. Only the last
//! hop is: re-pointing a link in the middle of a chain shows on the next event, not on its own.

use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

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

/// Where a symlinked `config.toml` points: its directory's watch and its file name there.
struct Target {
    wd: i32,
    name: Vec<u8>,
}

/// Blocks forever, sending the config after every relevant event. Returns `Ok` once nobody listens.
fn watch(dir: &Path, changed: UnboundedSender<std::result::Result<Config, String>>) -> Result<()> {
    let parent = dir.parent().ok_or(Errno::INVAL)?;
    let file = dir.join(config::FILE);
    let fd = inotify::init(CreateFlags::CLOEXEC)?;
    let mut watching = arm(&fd, dir, parent)?;
    let mut target = follow(&fd, &file, dir);
    let mut buffer = [MaybeUninit::uninit(); 4096];
    let mut reader = inotify::Reader::new(&fd, &mut buffer);

    loop {
        let (wd, flags, is_config, is_dir_name, is_target) = {
            let event = reader.next()?;
            let name = event.file_name();
            (
                event.wd(),
                event.events(),
                name == Some(FILE_NAME),
                name == Some(DIR_NAME),
                target
                    .as_ref()
                    .is_some_and(|target| name.map(CStr::to_bytes) == Some(&target.name[..])),
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
            // In the directory a symlinked config.toml points into. Gone or moved: follow again.
            _ if target.as_ref().is_some_and(|target| target.wd == wd) => {
                is_target || flags.intersects(ReadFlags::MOVE_SELF | ReadFlags::IGNORED)
            }
            // Leftovers from a watch we already replaced.
            _ => false,
        };

        if notify {
            // The link may have been made, re-pointed or removed, or its target may have moved.
            let followed = follow(&fd, &file, dir);
            if let Some(old) = target.take()
                && followed.as_ref().is_none_or(|new| new.wd != old.wd)
                && !matches!(watching, Watching::Dir(current) if current == old.wd)
            {
                let _ = inotify::remove_watch(&fd, old.wd);
            }
            target = followed;
            if changed.unbounded_send(config::load_from(&file)).is_err() {
                return Ok(());
            }
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

/// Watches the directory `file` resolves into, if it's a symlink out of `dir`. inotify hands back
/// the same watch for a directory it already watches, so a link within `dir` is no extra watch.
fn follow(fd: &impl rustix::fd::AsFd, file: &Path, dir: &Path) -> Option<Target> {
    let resolved = resolve(file);
    let into = std::fs::canonicalize(resolved.parent()?).ok()?;
    if std::fs::canonicalize(dir).is_ok_and(|dir| dir == into) {
        return None;
    }
    let wd = inotify::add_watch(fd, &into, DIR_EVENTS).ok()?;
    let name = resolved.file_name()?.as_bytes().to_vec();
    Some(Target { wd, name })
}

/// Where `file` ends up through its symlinks, even when that's nowhere yet: an editor that moves
/// the old file away before writing the new one leaves the link dangling for a moment, and the
/// new file has to be seen arriving.
fn resolve(file: &Path) -> PathBuf {
    let mut path = file.to_path_buf();
    // The kernel's own limit on links followed in one lookup.
    for _ in 0..40 {
        let Ok(next) = std::fs::read_link(&path) else {
            break;
        };
        path = match path.parent() {
            Some(parent) => parent.join(next),
            None => next,
        };
    }
    path
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

    #[test]
    fn follows_a_symlinked_config() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("eave-watch-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir, real, other) = (root.join("eave"), root.join("real"), root.join("other"));
        for each in [&dir, &real, &other] {
            std::fs::create_dir_all(each).unwrap();
        }
        let target = real.join(config::FILE);
        let link = dir.join(config::FILE);
        std::fs::write(&target, "[[left]]\nwidget = \"a\"").unwrap();
        symlink(&target, &link).unwrap();

        let (sender, mut receiver) = mpsc::unbounded();
        let watched = dir.clone();
        std::thread::spawn(move || watch(&watched, sender));
        std::thread::sleep(Duration::from_millis(100));

        // A plain write to where the link points.
        std::fs::write(&target, "[[left]]\nwidget = \"b\"").unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["b"]);

        // An editor-style save there.
        let temp = real.join(".config.toml.swp");
        std::fs::write(&temp, "[[left]]\nwidget = \"c\"").unwrap();
        std::fs::rename(&temp, &target).unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["c"]);

        // A save that moves the old file away first, then writes a new one (vim's backup), leaves
        // the link pointing at nothing for a moment: it's still followed.
        std::fs::rename(&target, real.join("config.toml~")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&target, "[[left]]\nwidget = \"c2\"").unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["c2"]);

        // Other files beside it are ignored.
        std::fs::write(real.join("hyprland.conf"), "").unwrap();
        assert!(settle(&mut receiver).is_none());

        // eave calibrate saves through the link, and leaves it a link.
        let shape = crate::calibrate::shape::Shape {
            screen: config::Screen {
                corners: [20.0, 20.0, 12.0, 12.0],
                ..config::Screen::default()
            },
            notch: None,
        };
        crate::calibrate::save::save(&link, "eDP-1", &shape).unwrap();
        let saved = settle(&mut receiver).expect("no event").unwrap();
        assert_eq!(
            saved.screens().screen_for("eDP-1", "").screen,
            Some(shape.screen)
        );
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());

        // The link is re-pointed, atomically, to a file of another name.
        let moved = other.join("eave.toml");
        std::fs::write(&moved, "[[left]]\nwidget = \"d\"").unwrap();
        let new_link = dir.join(".config.toml.new");
        symlink(&moved, &new_link).unwrap();
        std::fs::rename(&new_link, &link).unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["d"]);
        std::fs::write(&moved, "[[left]]\nwidget = \"e\"").unwrap();
        assert_eq!(widgets(settle(&mut receiver)), ["e"]);
        // Where it used to point no longer counts.
        std::fs::write(&target, "[[left]]\nwidget = \"f\"").unwrap();
        assert!(settle(&mut receiver).is_none());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
