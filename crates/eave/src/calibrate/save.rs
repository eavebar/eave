//! Writes what `eave calibrate` measured into config.toml: only the `notch` and `screen` keys of
//! the monitor's `[monitor."…"]` table (D-94), keeping everything else as it was, comments too
//! (D-71).

use std::path::Path;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use super::shape::Shape;

/// `source` with `notch` and `screen` in `[monitor."<table>"]` set to `shape`. Keys already there
/// keep their place.
pub fn edit(source: &str, table: &str, shape: &Shape) -> Result<String, String> {
    let mut document: DocumentMut = source.parse().map_err(|error| format!("{error}"))?;
    // `[monitor."…"]` alone, without an empty `[monitor]` above it.
    let monitors = document
        .as_table_mut()
        .entry("monitor")
        .or_insert_with(|| {
            let mut monitors = Table::new();
            monitors.set_implicit(true);
            Item::Table(monitors)
        })
        .as_table_mut()
        .ok_or("`monitor` isn't a table of monitors")?;
    let root = monitors
        .entry(table)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("`monitor.\"{table}\"` isn't a table"))?;

    let notch = match shape.notch {
        Some(notch) => {
            let mut table = InlineTable::new();
            table.insert("width", number(notch.width));
            table.insert("height", number(notch.height.unwrap_or_default()));
            table.insert("radius", number(notch.radius));
            table.insert("smoothing", smoothing(notch.smoothing));
            table.insert("shoulder", number(notch.shoulder));
            table.insert("top", number(notch.top));
            // Not measured, but the user's: kept as they had it (D-75).
            if notch.grow != [0.0; 2] {
                table.insert("grow", two(notch.grow));
            }
            Value::InlineTable(table)
        }
        None => Value::from(false),
    };
    set(root, "notch", notch);

    let screen = shape.screen;
    let mut table = InlineTable::new();
    table.insert("corners", four(screen.corners));
    table.insert("smoothing", smoothing(screen.smoothing));
    table.insert("inset", four(screen.inset));
    // Not measured, but the user's: kept as they had it (D-81).
    if screen.grow != 0.0 {
        table.insert("grow", number(screen.grow));
    }
    set(root, "screen", Value::InlineTable(table));

    Ok(document.to_string())
}

/// Sets `key` to `value`, keeping the key's place, and the comments around it, if it's there.
fn set(root: &mut toml_edit::Table, key: &str, value: Value) {
    match root.get_mut(key) {
        Some(Item::Value(old)) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        Some(item) => *item = Item::Value(value),
        None => {
            root.insert(key, Item::Value(value));
        }
    }
}

/// A size: whole when it is, so config.toml reads `20`, not `20.0`.
fn number(size: f32) -> Value {
    if size.fract() == 0.0 {
        Value::from(size as i64)
    } else {
        Value::from(f64::from((size * 100.0).round() / 100.0))
    }
}

/// A smoothing, to two places.
fn smoothing(smoothing: f32) -> Value {
    Value::from((f64::from(smoothing) * 100.0).round() / 100.0)
}

fn two(sizes: [f32; 2]) -> Value {
    Value::Array(sizes.into_iter().map(number).collect::<Array>())
}

fn four(sizes: [f32; 4]) -> Value {
    Value::Array(sizes.into_iter().map(number).collect::<Array>())
}

/// Saves `shape` into the config.toml at `path`, or a new one with just it. Through a symlink to
/// where it points, and by renaming a whole new file over the old, so the bar never reads half
/// of one.
pub fn save(path: &Path, table: &str, shape: &Shape) -> Result<(), String> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let edited =
        edit(&source, table, shape).map_err(|error| format!("{}: {error}", path.display()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    let temporary = path.with_extension("toml.calibrating");
    std::fs::write(&temporary, edited)
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, NotchShape, Screen};

    fn measured() -> Shape {
        Shape {
            screen: Screen {
                corners: [20.0, 20.0, 12.5, 12.5],
                smoothing: 0.6,
                inset: [0.0; 4],
                grow: 0.0,
            },
            notch: Some(NotchShape {
                width: 370.0,
                height: Some(66.0),
                radius: 18.0,
                smoothing: 0.25,
                shoulder: 6.0,
                top: 0.0,
                grow: [0.0; 2],
            }),
        }
    }

    /// What `config` says for the monitor `edit` saved as `table`.
    fn saved(config: &config::Config, table: &str) -> config::MonitorScreen {
        let screens = config.screens();
        let (_, monitor) = screens.matching(table, "").expect("the monitor's table");
        *monitor
    }

    #[test]
    fn it_keeps_the_rest_of_the_file_as_it_was() {
        let source = "\
# My bar.
renderer = \"cpu\"   # the GPU drains the battery
notch = \"auto\"   # everywhere else
islands = { radius = 24 }   # rounder

[[center]]
widget = \"clock\"   # every minute

[monitor.\"eDP-1\"]   # the MacBook
notch = false   # found by eave
";
        let edited = edit(source, "eDP-1", &measured()).unwrap();
        assert!(
            edited.starts_with("# My bar.\nrenderer = \"cpu\"   # the GPU drains the battery\n")
        );
        assert!(
            edited.contains("notch = \"auto\"   # everywhere else\n"),
            "{edited}"
        );
        assert!(
            edited.contains("[monitor.\"eDP-1\"]   # the MacBook\n"),
            "{edited}"
        );
        assert!(edited.contains("notch = { width = 370, height = 66, radius = 18, smoothing = 0.25, shoulder = 6, top = 0 }   # found by eave\n"), "{edited}");
        assert!(edited.contains("widget = \"clock\"   # every minute\n"));
        assert!(edited.contains("islands = { radius = 24 }   # rounder\n"));
        // Only the monitor's table has a screen.
        let screen = edited.find("screen = ").unwrap();
        assert!(screen > edited.find("[monitor.").unwrap(), "{edited}");
    }

    #[test]
    fn a_new_monitor_gets_its_own_table() {
        let source = "notch = \"auto\"\n\n[[center]]\nwidget = \"clock\"\n";
        let edited = edit(source, "ViewSonic Corporation XG2431", &measured()).unwrap();
        assert!(
            edited.contains("\n[monitor.\"ViewSonic Corporation XG2431\"]\nnotch = {"),
            "{edited}"
        );
        assert!(!edited.contains("[monitor]"), "{edited}");
        let config = config::parse(&edited).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.notch, config::Notch::Auto);
        assert_eq!(config.entries.len(), 1);
        let setting = config
            .screens()
            .screen_for("DP-2", "ViewSonic Corporation XG2431 WEE241700714");
        assert_eq!(setting.screen, measured().screen);
    }

    #[test]
    fn what_it_writes_reads_back_the_same() {
        let shape = measured();
        let config = config::parse(&edit("", "eDP-1", &shape).unwrap()).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        let monitor = saved(&config, "eDP-1");
        assert_eq!(monitor.screen, Some(shape.screen));
        assert_eq!(
            monitor.notch,
            Some(config::Notch::Fixed(shape.notch.unwrap()))
        );

        let none = Shape {
            notch: None,
            ..shape
        };
        let config = config::parse(&edit("", "eDP-1", &none).unwrap()).unwrap();
        assert_eq!(saved(&config, "eDP-1").notch, Some(config::Notch::Off));
    }

    #[test]
    fn a_grown_notch_stays_grown() {
        let mut shape = measured();
        shape.notch.as_mut().unwrap().grow = [4.0, 2.0];
        let edited = edit("", "eDP-1", &shape).unwrap();
        assert!(edited.contains("top = 0, grow = [4, 2] }"), "{edited}");
        let config = config::parse(&edited).unwrap();
        assert_eq!(
            saved(&config, "eDP-1").notch,
            Some(config::Notch::Fixed(shape.notch.unwrap()))
        );
    }

    #[test]
    fn a_grown_edge_stays_grown() {
        let mut shape = measured();
        shape.screen.grow = 4.0;
        let edited = edit("", "eDP-1", &shape).unwrap();
        assert!(
            edited.contains("inset = [0, 0, 0, 0], grow = 4 }"),
            "{edited}"
        );
        let config = config::parse(&edited).unwrap();
        assert_eq!(saved(&config, "eDP-1").screen, Some(shape.screen));
    }

    #[test]
    fn it_wont_write_over_a_monitor_that_isnt_a_table() {
        assert!(edit("monitor = 1\n", "eDP-1", &measured()).is_err());
    }

    #[test]
    fn it_saves_through_a_symlink() {
        let dir = std::env::temp_dir().join(format!("eave-calibrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let real = dir.join("dotfiles/config.toml");
        std::fs::write(&real, "# mine\n").unwrap();
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        save(&link, "eDP-1", &measured()).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        let saved = std::fs::read_to_string(&real).unwrap();
        assert!(
            saved.contains("# mine\n") && saved.contains("screen = "),
            "{saved}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
