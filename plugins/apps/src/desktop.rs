//! Desktop entries: the `.desktop` files apps install under `applications` in each XDG data
//! directory, read the way the Desktop Entry spec says, as far as a launcher needs.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// An app that can be launched, from its desktop entry.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    /// The desktop file id: its path under `applications`, `/` turned into `-`, without
    /// `.desktop`. Also how the host finds its icon.
    pub id: String,
    pub name: String,
    /// What kind of app it is: "Web Browser".
    pub generic: Option<String>,
    pub keywords: Vec<String>,
    /// The command line, field codes and all.
    pub exec: String,
    /// It runs in a terminal.
    pub terminal: bool,
}

/// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`: where `applications` directories are, first wins.
pub fn data_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = var("HOME").map(PathBuf::from).unwrap_or_default();
    let data_home = var("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
    let data_dirs = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut dirs = vec![data_home];
    dirs.extend(std::env::split_paths(&data_dirs));
    dirs
}

/// The desktops this session is, from `$XDG_CURRENT_DESKTOP`, for `OnlyShowIn` and `NotShowIn`.
pub fn desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every app to show, by name. An entry in an earlier directory hides one with the same id in a
/// later one, even if it isn't shown itself: that's how a user hides or overrides an app. Reads
/// files, so it blocks.
pub fn scan(dirs: &[PathBuf], desktops: &[String]) -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let root = dir.join("applications");
        let mut files = Vec::new();
        collect(&root, &mut files);
        for file in files {
            let Some(id) = id(&root, &file) else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            apps.extend(parse(id, &text, desktops));
        }
    }
    apps.sort_by_cached_key(|app| app.name.to_lowercase());
    apps
}

/// The `.desktop` files under `dir`, in its subdirectories too.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "desktop") {
            out.push(path);
        }
    }
}

/// The desktop file id of `file` under `root`.
fn id(root: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(root).ok()?.to_str()?;
    Some(relative.strip_suffix(".desktop")?.replace('/', "-"))
}

/// The app in a desktop entry's text, if it's one to show here.
pub fn parse(id: String, text: &str, desktops: &[String]) -> Option<App> {
    let mut in_entry = false;
    let mut name = None;
    let mut generic = None;
    let mut keywords = Vec::new();
    let mut exec = None;
    let mut terminal = false;
    let mut application = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // Localized keys (`Name[de]`) don't match: the plain ones are used.
        let value = unescape(value.trim());
        let shown_in = |list: &str| {
            list.split(';')
                .any(|desktop| desktops.iter().any(|ours| ours == desktop))
        };
        match key.trim() {
            "Type" => application = value == "Application",
            "Name" => name = Some(value),
            "GenericName" if !value.is_empty() => generic = Some(value),
            "Keywords" => keywords = list(&value),
            "Exec" => exec = Some(value),
            "Terminal" => terminal = value == "true",
            "NoDisplay" | "Hidden" if value == "true" => return None,
            "OnlyShowIn" if !shown_in(&value) => return None,
            "NotShowIn" if shown_in(&value) => return None,
            _ => {}
        }
    }
    if !application {
        return None;
    }
    Some(App {
        id,
        name: name.filter(|name| !name.is_empty())?,
        generic,
        keywords,
        exec: exec.filter(|exec| !exec.is_empty())?,
        terminal,
    })
}

/// A string value with its escapes (`\s`, `\n`, `\t`, `\r`, `\\`) turned back into characters.
/// `\;` stays as it is, for [`list`].
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A `;`-separated list, where `\;` is a `;` inside an item.
fn list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&';') => {
                item.push(';');
                chars.next();
            }
            ';' => items.push(std::mem::take(&mut item)),
            _ => item.push(c),
        }
    }
    items.push(item);
    items.retain(|item| !item.is_empty());
    items
}

/// The arguments of an `Exec` line, with its quoting undone and its field codes (`%f`, `%U`, …)
/// left out: there are no files or URLs to hand it. `None` if the quoting is broken.
pub fn arguments(exec: &str) -> Option<Vec<String>> {
    let mut arguments = Vec::new();
    let mut chars = exec.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else {
            break;
        };
        let mut argument = String::new();
        if first == '"' {
            chars.next();
            loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => argument.push(chars.next()?),
                    c => argument.push(c),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                argument.push(c);
            }
        }
        // A field code alone is left out with its argument; elsewhere, codes are dropped.
        if argument.len() == 2 && argument.starts_with('%') && argument != "%%" {
            continue;
        }
        arguments.push(fields(&argument));
    }
    Some(arguments)
}

/// `argument` with `%%` turned into `%` and other field codes left out.
fn fields(argument: &str) -> String {
    let mut out = String::with_capacity(argument.len());
    let mut chars = argument.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if let Some('%') = chars.next() {
            out.push('%');
        }
    }
    out
}

/// A command line for a shell, which Hyprland's exec runs: each argument quoted where it needs to
/// be. In `terminal` first, if it's given: `foot`, or `alacritty -e`.
pub fn command(arguments: &[String], terminal: Option<&str>) -> String {
    let quoted = arguments.iter().map(|argument| quote(argument));
    terminal
        .map(str::to_owned)
        .into_iter()
        .chain(quoted)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./=:,@+".contains(c));
    if plain {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> Option<App> {
        parse("app".into(), text, &["Hyprland".into()])
    }

    #[test]
    fn reads_what_a_launcher_needs() {
        let parsed = app("[Desktop Entry]\n\
             Type=Application\n\
             Name=Firefox\n\
             Name[it]=Firefox italiano\n\
             GenericName=Web Browser\n\
             Keywords=Internet;WWW;Web\\;Net;\n\
             Exec=firefox %u\n\
             \n\
             [Desktop Action new-window]\n\
             Name=New Window\n\
             Exec=firefox --new-window %u\n")
        .unwrap();
        assert_eq!(parsed.name, "Firefox");
        assert_eq!(parsed.generic.as_deref(), Some("Web Browser"));
        assert_eq!(parsed.keywords, ["Internet", "WWW", "Web;Net"]);
        assert_eq!(parsed.exec, "firefox %u");
        assert!(!parsed.terminal);
    }

    #[test]
    fn leaves_out_what_isnt_shown_here() {
        let entry = |extra: &str| {
            app(&format!(
                "[Desktop Entry]\nType=Application\nName=A\nExec=a\n{extra}"
            ))
        };
        assert!(entry("").is_some());
        assert!(entry("NoDisplay=true").is_none());
        assert!(entry("Hidden=true").is_none());
        assert!(entry("OnlyShowIn=GNOME;KDE;").is_none());
        assert!(entry("OnlyShowIn=GNOME;Hyprland;").is_some());
        assert!(entry("NotShowIn=Hyprland;").is_none());
        assert!(app("[Desktop Entry]\nType=Link\nName=A\nURL=x").is_none());
        assert!(app("[Desktop Entry]\nType=Application\nName=A").is_none());
    }

    #[test]
    fn exec_lines_lose_their_field_codes_and_quoting() {
        assert_eq!(arguments("firefox %u").unwrap(), ["firefox"]);
        assert_eq!(
            arguments(r#"env "A=b c" app --x=%f --pct=50%% \"q"#).unwrap(),
            ["env", "A=b c", "app", "--x=", "--pct=50%", r#"\"q"#]
        );
        assert_eq!(
            arguments(r#"sh -c "echo \"hi\" \$HOME""#).unwrap(),
            ["sh", "-c", r#"echo "hi" $HOME"#]
        );
        assert_eq!(arguments(r#"app "unclosed"#), None);
    }

    #[test]
    fn commands_are_quoted_for_the_shell() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            command(&args(&["env", "A=b c", "it's"]), None),
            r"env 'A=b c' 'it'\''s'"
        );
        assert_eq!(command(&args(&["htop"]), Some("foot")), "foot htop");
        assert_eq!(command(&args(&["$HOME"]), None), "'$HOME'");
    }

    #[test]
    fn earlier_directories_win_and_hidden_entries_hide() {
        let root = std::env::temp_dir().join(format!("eave-apps-{}", std::process::id()));
        let write = |dir: &str, name: &str, text: &str| {
            let path = root.join(dir).join("applications").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        let entry = |name: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x");
        write("home", "editor.desktop", &entry("My Editor"));
        write("home", "gone.desktop", "[Desktop Entry]\nHidden=true");
        write("system", "editor.desktop", &entry("Editor"));
        write("system", "gone.desktop", &entry("Gone"));
        write("system", "kde/tool.desktop", &entry("Tool"));
        write("system", "readme.txt", "not an entry");

        let apps = scan(&[root.join("home"), root.join("system")], &[]);
        let found: Vec<_> = apps.iter().map(|app| (app.id.as_str(), app.name.as_str())).collect();
        assert_eq!(found, [("editor", "My Editor"), ("kde-tool", "Tool")]);
        fs::remove_dir_all(root).unwrap();
    }
}
