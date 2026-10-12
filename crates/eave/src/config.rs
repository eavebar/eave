//! `config.toml`: which widgets go where. With no file, the bar is empty.
//!
//! ```toml
//! [[center]]
//! widget = "clock"
//! format = "%H:%M"   # every other key goes to the widget
//! ```

use std::path::PathBuf;

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
    pub options: toml::Table,
}

/// Entries in bar order, plus a warning for everything that was skipped.
#[derive(Debug, Default)]
pub struct Config {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

pub fn load() -> Config {
    let Some(path) = path() else {
        return Config::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(source) => parse(&source).unwrap_or_else(|error| Config {
            entries: Vec::new(),
            warnings: vec![format!("{}: {error}", path.display())],
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(error) => Config {
            entries: Vec::new(),
            warnings: vec![format!("{}: {error}", path.display())],
        },
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("eave/config.toml"))
}

pub fn parse(source: &str) -> Result<Config, toml::de::Error> {
    let mut table: toml::Table = source.parse()?;
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
        for (index, item) in items.into_iter().enumerate() {
            let toml::Value::Table(mut options) = item else {
                config
                    .warnings
                    .push(format!("{}[{index}] is not a table", zone.key()));
                continue;
            };
            match options.remove("widget") {
                Some(toml::Value::String(widget)) => config.entries.push(Entry {
                    zone,
                    widget,
                    options,
                }),
                _ => config.warnings.push(format!(
                    "{}[{index}] has no `widget = \"name\"`",
                    zone.key()
                )),
            }
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
    fn syntax_errors_are_errors() {
        assert!(parse("[[left]").is_err());
    }
}
