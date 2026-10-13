//! `config.toml`: which widgets go where. With no file, the bar is empty.
//!
//! ```toml
//! [[center]]
//! widget = "clock"
//! format = "%H:%M"   # every other key goes to the widget
//! width_group = "status"   # except this: items in an island with the same group are as wide
//!                          # as the widest of them
//!
//! [[right]]
//! widget = "someone/weather@v0.2"   # a plugin, compiled in by eave-build (D-30)
//! collapsible = true   # also the host's: hidden while its island is collapsed
//!
//! [[right]]
//! divider = true   # a line before the next item
//!
//! [[right]]
//! island = true   # the next items are another island, beside this one
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Center,
    Right,
}

impl Zone {
    pub const ALL: [Zone; 3] = [Zone::Left, Zone::Center, Zone::Right];

    fn key(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Center => "center",
            Zone::Right => "right",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub zone: Zone,
    pub widget: String,
    /// Items in the same island and group get the same width, the widest one's.
    pub width_group: Option<String>,
    /// Which of its zone's islands it's in, counted from 0 in bar order.
    pub island: usize,
    /// Hidden while its island is collapsed.
    pub collapsible: bool,
    /// A divider comes before it.
    pub divider: bool,
    pub options: toml::Table,
}

/// Entries in bar order, plus a warning for everything that was skipped.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Config {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

/// Reads `config.toml`. A missing file is an empty bar. A file that can't be read or parsed is an
/// error, so a reload can keep the current bar instead of blanking it over a typo.
pub fn load() -> Result<Config, String> {
    match dir() {
        Some(dir) => load_from(&dir.join(FILE)),
        None => Ok(Config::default()),
    }
}

pub fn load_from(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(source) => parse(&source).map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// The config file's name inside [`dir`].
pub const FILE: &str = "config.toml";

/// `$XDG_CONFIG_HOME/eave`, where `config.toml` lives.
pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("eave"))
}

/// The widget a `widget = "…"` reference names. `clock` is built in, the rest name a plugin's
/// source ([D-30](../../../docs/decisions.md)): `someone/eave-weather@v0.2` is `weather` (the
/// repository's name without `eave-`), and `someone/tools#cpu` is `cpu`. eave-build parses the
/// same grammar, and keeps the same test cases.
pub fn widget_name(reference: &str) -> &str {
    if let Some((_, name)) = reference.rsplit_once('#') {
        return name;
    }
    // A revision follows the last `/` and never holds a `:`, so `git@host:x/y` keeps its `@`.
    let tail = reference.rfind('/').map_or(0, |slash| slash + 1);
    let source = match reference[tail..].rfind('@') {
        Some(at) if !reference[tail + at + 1..].contains(':') => &reference[..tail + at],
        _ => reference,
    };
    let last = source
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(source);
    let last = last.strip_suffix(".git").unwrap_or(last);
    last.strip_prefix("eave-").unwrap_or(last)
}

pub fn parse(source: &str) -> Result<Config, toml::de::Error> {
    let mut table: toml::Table = source.parse()?;
    // Where eave itself comes from. Only eave-build reads it.
    table.remove("eave");
    let mut config = Config::default();

    for zone in Zone::ALL {
        let Some(value) = table.remove(zone.key()) else {
            continue;
        };
        let toml::Value::Array(items) = value else {
            config.warnings.push(format!(
                "`{}` must be a list of widgets ([[{0}]])",
                zone.key()
            ));
            continue;
        };
        let mut island = 0;
        // Whether the current island has an item yet, so markers before any don't make an empty
        // one.
        let mut filled = false;
        let mut divider = false;
        for (index, item) in items.into_iter().enumerate() {
            let at = format!("{}[{index}]", zone.key());
            let toml::Value::Table(mut options) = item else {
                config.warnings.push(format!("{at} is not a table"));
                continue;
            };
            let Some(widget) = options.remove("widget") else {
                // A marker: a divider, or the start of another island.
                let marker = |options: &toml::Table, key| options.get(key) == Some(&true.into());
                match options.len() {
                    1 if marker(&options, "island") => {
                        if divider {
                            config.warnings.push(format!(
                                "{at}: a divider must come before an item, ignoring it"
                            ));
                            divider = false;
                        }
                        if filled {
                            island += 1;
                            filled = false;
                        }
                    }
                    1 if marker(&options, "divider") => divider = true,
                    _ => config.warnings.push(format!(
                        "{at} has no `widget = \"name\"` (or is `divider = true` or \
                         `island = true`)"
                    )),
                }
                continue;
            };
            let toml::Value::String(widget) = widget else {
                config
                    .warnings
                    .push(format!("{at}: `widget` must be a name, skipping it"));
                continue;
            };
            let width_group = match options.remove("width_group") {
                Some(toml::Value::String(group)) => Some(group),
                Some(_) => {
                    config
                        .warnings
                        .push(format!("{at}: `width_group` must be a name, ignoring it"));
                    None
                }
                None => None,
            };
            let collapsible = match options.remove("collapsible") {
                Some(toml::Value::Boolean(collapsible)) => collapsible,
                Some(_) => {
                    config.warnings.push(format!(
                        "{at}: `collapsible` must be true or false, ignoring it"
                    ));
                    false
                }
                None => false,
            };
            config.entries.push(Entry {
                zone,
                widget,
                width_group,
                island,
                collapsible,
                divider: std::mem::take(&mut divider),
                options,
            });
            filled = true;
        }
        if divider {
            config.warnings.push(format!(
                "{}: a divider must come before an item, ignoring the last one",
                zone.key()
            ));
        }
    }

    config
        .warnings
        .extend(table.keys().map(|key| format!("unknown key `{key}`")));
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_means_empty_bar() {
        let config = parse("").unwrap();
        assert!(config.entries.is_empty());
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn entries_keep_zone_order_and_options() {
        let config = parse(
            r#"
            [[right]]
            widget = "battery"

            [[left]]
            widget = "clock"
            format = "%H"

            [[left]]
            widget = "title"
            "#,
        )
        .unwrap();

        let names: Vec<_> = config
            .entries
            .iter()
            .map(|entry| (entry.zone, entry.widget.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                (Zone::Left, "clock"),
                (Zone::Left, "title"),
                (Zone::Right, "battery")
            ]
        );
        assert_eq!(config.entries[0].options["format"].as_str(), Some("%H"));
        assert!(!config.entries[0].options.contains_key("widget"));
    }

    #[test]
    fn bad_entries_are_skipped_with_warnings() {
        let config = parse(
            r#"
            center = "clock"
            colour = "red"
            [[left]]
            format = "%H"
            "#,
        )
        .unwrap();
        assert!(config.entries.is_empty());
        assert_eq!(config.warnings.len(), 3);
    }

    #[test]
    fn eave_source_is_not_a_warning() {
        let config = parse("eave = \"~/code/eave\"").unwrap();
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn references_name_their_widget() {
        // Keep in sync with eave-build's `references_name_their_widget`.
        for (reference, name) in [
            ("clock", "clock"),
            ("someone/weather", "weather"),
            ("someone/eave-weather", "weather"),
            ("someone/eave-weather@v0.2", "weather"),
            ("someone/tools#cpu", "cpu"),
            ("someone/tools@main#cpu", "cpu"),
            ("https://git.sr.ht/~x/eave-mail", "mail"),
            ("https://codeberg.org/x/eave-mail.git@1a2b3c", "mail"),
            ("git@github.com:x/eave-mail.git", "mail"),
            ("git@host:mail.git", "mail"),
            ("codeberg.org/x/mail/", "mail"),
            ("~/code/eave-weather", "weather"),
            ("./weather", "weather"),
        ] {
            assert_eq!(widget_name(reference), name, "{reference}");
        }
    }

    #[test]
    fn width_groups_are_the_hosts() {
        let config = parse(
            r#"
            [[right]]
            widget = "volume"
            width_group = "status"
            step = 2
            [[right]]
            widget = "network"
            width_group = 3
            "#,
        )
        .unwrap();
        assert_eq!(config.entries[0].width_group.as_deref(), Some("status"));
        assert!(!config.entries[0].options.contains_key("width_group"));
        assert_eq!(config.entries[1].width_group, None);
        assert_eq!(config.warnings.len(), 1);
    }

    fn layout(config: &Config) -> Vec<(&str, usize, bool, bool)> {
        config
            .entries
            .iter()
            .map(|entry| {
                let name = entry.widget.as_str();
                (name, entry.island, entry.divider, entry.collapsible)
            })
            .collect()
    }

    #[test]
    fn markers_start_islands_and_put_dividers_before_items() {
        let config = parse(
            r#"
            [[right]]
            island = true
            [[right]]
            widget = "volume"
            collapsible = true
            [[right]]
            divider = true
            [[right]]
            widget = "network"
            [[right]]
            island = true
            [[right]]
            island = true
            [[right]]
            widget = "battery"
            [[left]]
            widget = "clock"
            "#,
        )
        .unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(
            layout(&config),
            [
                ("clock", 0, false, false),
                ("volume", 0, false, true),
                ("network", 0, true, false),
                ("battery", 1, false, false),
            ]
        );
        assert!(!config.entries[1].options.contains_key("collapsible"));
    }

    #[test]
    fn dividers_without_an_item_after_them_are_warned_about() {
        let config = parse(
            r#"
            [[right]]
            widget = "volume"
            [[right]]
            divider = true
            [[right]]
            island = true
            [[right]]
            widget = "battery"
            [[right]]
            divider = true
            "#,
        )
        .unwrap();
        assert_eq!(config.warnings.len(), 2, "{:?}", config.warnings);
        assert_eq!(
            layout(&config),
            [("volume", 0, false, false), ("battery", 1, false, false)]
        );
    }

    #[test]
    fn odd_markers_and_collapsible_values_are_warned_about() {
        let config = parse(
            r#"
            [[left]]
            divider = 1
            [[left]]
            divider = true
            island = true
            [[left]]
            widget = "clock"
            collapsible = "yes"
            "#,
        )
        .unwrap();
        assert_eq!(config.warnings.len(), 3, "{:?}", config.warnings);
        assert_eq!(layout(&config), [("clock", 0, false, false)]);
    }

    #[test]
    fn syntax_errors_are_errors() {
        assert!(parse("[[left]").is_err());
    }

    #[test]
    fn missing_file_is_empty_but_broken_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("eave-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);

        assert_eq!(load_from(&path), Ok(Config::default()));
        std::fs::write(&path, "[[left]").unwrap();
        assert!(load_from(&path).is_err());
        std::fs::write(&path, "[[left]]\nwidget = \"clock\"").unwrap();
        assert_eq!(load_from(&path).unwrap().entries.len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
