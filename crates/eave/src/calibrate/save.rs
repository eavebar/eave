//! Writes what `eave calibrate` measured into config.toml: only its `notch` and `screen` keys,
//! keeping everything else as it was, comments too (D-71).

use std::path::Path;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Value};

use super::shape::Shape;

/// `source` with `notch` and `screen` set to `shape`. Keys already there keep their place.
pub fn edit(source: &str, shape: &Shape) -> Result<String, toml_edit::TomlError> {
    let mut document: DocumentMut = source.parse()?;
    let root = document.as_table_mut();

    let notch = match shape.notch {
        Some(notch) => {
            let mut table = InlineTable::new();
            table.insert("width", number(notch.width));
            table.insert("height", number(notch.height.unwrap_or_default()));
            table.insert("radius", number(notch.radius));
            table.insert("smoothing", smoothing(notch.smoothing));
            table.insert("shoulder", number(notch.shoulder));
            table.insert("top", number(notch.top));
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

fn four(sizes: [f32; 4]) -> Value {
    Value::Array(sizes.into_iter().map(number).collect::<Array>())
}

/// Saves `shape` into the config.toml at `path`, or a new one with just it. Through a symlink to
/// where it points, and by renaming a whole new file over the old, so the bar never reads half
/// of one.
pub fn save(path: &Path, shape: &Shape) -> Result<(), String> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let edited = edit(&source, shape).map_err(|error| format!("{}: {error}", path.display()))?;
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
            },
            notch: Some(NotchShape {
                width: 370.0,
                height: Some(66.0),
                radius: 18.0,
                smoothing: 0.25,
                shoulder: 6.0,
                top: 0.0,
            }),
        }
    }

    #[test]
    fn it_keeps_the_rest_of_the_file_as_it_was() {
        let source = "\
# My bar.
renderer = \"cpu\"   # the GPU drains the battery
notch = \"auto\"   # found by eave
islands = { radius = 24 }   # rounder

[[center]]
widget = \"clock\"   # every minute
";
        let edited = edit(source, &measured()).unwrap();
        assert!(
            edited.starts_with("# My bar.\nrenderer = \"cpu\"   # the GPU drains the battery\n")
        );
        assert!(edited.contains("notch = { width = 370, height = 66, radius = 18, smoothing = 0.25, shoulder = 6, top = 0 }   # found by eave\n"), "{edited}");
        assert!(edited.contains("widget = \"clock\"   # every minute\n"));
        assert!(edited.contains("islands = { radius = 24 }   # rounder\n"));
        // New keys go with the others above the zones, so they aren't read as a zone's.
        let screen = edited.find("screen = ").unwrap();
        assert!(screen < edited.find("[[center]]").unwrap(), "{edited}");
    }

    #[test]
    fn what_it_writes_reads_back_the_same() {
        let shape = measured();
        let config = config::parse(&edit("", &shape).unwrap()).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.screen, shape.screen);
        assert_eq!(config.notch, config::Notch::Fixed(shape.notch.unwrap()));

        let none = Shape {
            notch: None,
            ..shape
        };
        let config = config::parse(&edit("", &none).unwrap()).unwrap();
        assert_eq!(config.notch, config::Notch::Off);
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

        save(&link, &measured()).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        let saved = std::fs::read_to_string(&real).unwrap();
        assert!(
            saved.contains("# mine\n") && saved.contains("screen = "),
            "{saved}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
