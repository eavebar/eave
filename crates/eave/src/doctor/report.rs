//! What `eave doctor` found: a list of checks, as text for a terminal and as TOML for the
//! `essential` plugin's dialog, which reads it back (D-96).

use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Good,
    Warning,
    Problem,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Good => "good",
            Level::Warning => "warning",
            Level::Problem => "problem",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub level: Level,
    /// What was checked, or what's wrong: one line.
    pub title: String,
    /// More about it, or what to do. May be empty.
    pub detail: String,
}

impl Check {
    pub fn new(level: Level, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            level,
            title: title.into(),
            detail: detail.into(),
        }
    }

    pub fn good(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(Level::Good, title, detail)
    }

    pub fn warning(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(Level::Warning, title, detail)
    }

    pub fn problem(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(Level::Problem, title, detail)
    }
}

/// One check a line, its detail indented under it.
pub fn text(checks: &[Check]) -> String {
    let mut out = String::new();
    for check in checks {
        out.push_str(&format!("{:<8} {}\n", check.level.name(), check.title));
        for line in check.detail.lines() {
            out.push_str(&format!("{:<8} {line}\n", ""));
        }
    }
    out
}

/// `[[check]]` tables, in order.
pub fn toml(checks: &[Check]) -> String {
    let mut document = toml_edit::DocumentMut::new();
    let mut tables = toml_edit::ArrayOfTables::new();
    for check in checks {
        let mut table = toml_edit::Table::new();
        table["level"] = toml_edit::value(check.level.name());
        table["title"] = toml_edit::value(&check.title);
        table["detail"] = toml_edit::value(&check.detail);
        tables.push(table);
    }
    document.insert("check", toml_edit::Item::ArrayOfTables(tables));
    document.to_string()
}

/// Writes the TOML report to `path` whole: to a file beside it first, then renamed over it, so
/// whoever waits for it never reads half of one.
pub fn save(checks: &[Check], path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staged = path.with_extension("toml.new");
    std::fs::write(&staged, toml(checks))?;
    std::fs::rename(&staged, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_hang_under_their_check() {
        let checks = [
            Check::good("config.toml reads", ""),
            Check::problem("`weather` isn't compiled in", "Run eave-build.\nOr wait."),
        ];
        assert_eq!(
            text(&checks),
            "good     config.toml reads\n\
             problem  `weather` isn't compiled in\n         Run eave-build.\n         Or wait.\n"
        );
    }

    #[test]
    fn the_report_reads_back_as_toml() {
        let checks = [
            Check::warning("a \"quoted\" title", "line one\nline two"),
            Check::good("fine", ""),
        ];
        let table: toml::Table = toml(&checks).parse().unwrap();
        let read = table["check"].as_array().unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0]["level"].as_str(), Some("warning"));
        assert_eq!(read[0]["title"].as_str(), Some("a \"quoted\" title"));
        assert_eq!(read[0]["detail"].as_str(), Some("line one\nline two"));
        assert_eq!(read[1]["level"].as_str(), Some("good"));
    }

    #[test]
    fn a_saved_report_replaces_the_old_one_whole() {
        let dir = std::env::temp_dir().join(format!("eave-doctor-{}", std::process::id()));
        let path = dir.join("doctor.toml");
        save(&[Check::good("one", "")], &path).unwrap();
        save(&[Check::problem("two", "")], &path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("two") && !saved.contains("one"));
        assert!(!path.with_extension("toml.new").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
