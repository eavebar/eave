//! What eave-build leaves in `~/.cache/eave/<check|update>.toml` with `--summary` (D-96), and what
//! the notifications say about it.

use std::io;
use std::path::PathBuf;

use serde::Deserialize;

/// Which of eave-build's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Check,
    Update,
}

impl Job {
    pub fn unit(self) -> &'static str {
        match self {
            Job::Check => "eave-update-check.service",
            Job::Update => "eave-update.service",
        }
    }

    fn file(self) -> &'static str {
        match self {
            Job::Check => "check.toml",
            Job::Update => "update.toml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Summary {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub moved: Vec<Moved>,
}

/// A source that moved, or would.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Moved {
    /// `eave`, or the widget it's for.
    pub label: String,
    #[serde(default)]
    pub commits: Option<u64>,
}

impl Moved {
    fn line(&self) -> String {
        match self.commits {
            Some(1) => format!("{}: 1 new commit", self.label),
            Some(commits) => format!("{}: {commits} new commits", self.label),
            None => format!("{}: newer", self.label),
        }
    }
}

/// What a notification says.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub summary: String,
    pub body: String,
    pub urgency: Urgency,
    /// Offers Update eave.
    pub offer_update: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

/// The summary `job` left, taken: read and removed, so it's told once, by whichever bar reads it
/// first. `None` when there's none. Reads a file, so it blocks.
pub fn take(job: Job) -> io::Result<Option<Summary>> {
    let path = path(job)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    std::fs::remove_file(&path)?;
    toml::from_str(&text).map(Some).map_err(io::Error::other)
}

fn path(job: Job) -> io::Result<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| io::Error::other("neither XDG_CACHE_HOME nor HOME is set"))?;
    Ok(cache.join("eave").join(job.file()))
}

/// What to tell about `summary`, which `job` left.
pub fn note(job: Job, summary: &Summary) -> Note {
    let lines = || {
        summary
            .moved
            .iter()
            .map(Moved::line)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let error = summary.error.clone().unwrap_or_default();
    let note = |summary: &str, body: String, urgency| Note {
        summary: summary.to_owned(),
        body,
        urgency,
        offer_update: false,
    };
    match (job, summary.ok, summary.moved.is_empty()) {
        (Job::Check, false, _) => note("Couldn't check for updates", error, Urgency::Normal),
        (Job::Check, true, true) => note("eave is up to date", String::new(), Urgency::Low),
        (Job::Check, true, false) => Note {
            offer_update: true,
            ..note("Updates for eave", lines(), Urgency::Normal)
        },
        (Job::Update, false, _) => note(
            "eave's update failed",
            format!("{error}\nSee journalctl --user -u eave-update"),
            Urgency::Critical,
        ),
        (Job::Update, true, true) => note("eave is up to date", String::new(), Urgency::Low),
        (Job::Update, true, false) => note("eave is updated", lines(), Urgency::Normal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As eave-build writes it.
    const CHECKED: &str = r#"
ok = true

[[moved]]
label = "eave"
from = "abc1234"
to = "def5678"
commits = 12

[[moved]]
label = "weather"
from = "1111111"
to = "2222222"
commits = 1
"#;

    #[test]
    fn a_check_with_updates_offers_one() {
        let summary: Summary = toml::from_str(CHECKED).unwrap();
        let note = note(Job::Check, &summary);
        assert_eq!(note.summary, "Updates for eave");
        assert_eq!(note.body, "eave: 12 new commits\nweather: 1 new commit");
        assert!(note.offer_update);
    }

    #[test]
    fn nothing_to_move_is_quiet() {
        let summary: Summary = toml::from_str("ok = true\nmoved = []\n").unwrap();
        for job in [Job::Check, Job::Update] {
            let note = note(job, &summary);
            assert_eq!(note.summary, "eave is up to date");
            assert_eq!(note.urgency, Urgency::Low);
            assert!(!note.offer_update);
        }
    }

    #[test]
    fn a_failed_update_is_critical_and_says_where_to_look() {
        let summary: Summary =
            toml::from_str("ok = false\nerror = \"cargo build failed\"\nmoved = []\n").unwrap();
        let note = note(Job::Update, &summary);
        assert_eq!(note.urgency, Urgency::Critical);
        assert!(note.body.starts_with("cargo build failed\n"));
        assert!(note.body.contains("journalctl --user -u eave-update"));
        let checked = super::note(Job::Check, &summary);
        assert_eq!(checked.summary, "Couldn't check for updates");
    }

    #[test]
    fn a_summary_is_told_once() {
        let cache = std::env::temp_dir().join(format!("eave-update-{}", std::process::id()));
        std::fs::create_dir_all(cache.join("eave")).unwrap();
        std::fs::write(cache.join("eave/check.toml"), CHECKED).unwrap();
        // SAFETY: the only test that reads the environment.
        unsafe { std::env::set_var("XDG_CACHE_HOME", &cache) };
        assert_eq!(take(Job::Check).unwrap().unwrap().moved.len(), 2);
        assert_eq!(take(Job::Check).unwrap(), None);
        assert_eq!(take(Job::Update).unwrap(), None);
        std::fs::remove_dir_all(cache).unwrap();
    }
}
