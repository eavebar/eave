//! The host side of [`eave_api::ui::app_icon`]: finds an app's icon the way freedesktop desktops
//! do, and loads it. The app's desktop entry names the icon (matched by file name or
//! `StartupWMClass`), and the icon theme GTK apps use (`gtk-icon-theme-name` in `settings.ini`,
//! read as a file, no dconf) finds the file, falling back through its `Inherits=` chain to
//! `hicolor`, then `pixmaps`. All of it is file I/O, so it runs on tokio's blocking pool, once per
//! app id; the host caches the result. Every icon is drawn there, once, into a small bitmap
//! ([`TARGET`] pixels square), so drawing the bar only copies pixels and big icons don't stay in
//! memory.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use iced::widget::image;
use resvg::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use resvg::usvg;

use crate::theme;

/// A loaded icon, ready to draw.
#[derive(Debug, Clone)]
pub struct AppIcon(pub image::Handle);

/// What the host knows so far: `None` for an app with no icon to be found.
pub type Icons = HashMap<String, Option<AppIcon>>;

/// Finds and loads the icon for `app_id`, off the UI thread.
pub async fn load(app_id: String) -> Option<AppIcon> {
    tokio::task::spawn_blocking(move || read(&find(&Env::from_process(), &app_id)?))
        .await
        .ok()
        .flatten()
}

/// The pixel size icons are picked for: twice the drawn size, so they stay sharp on a 2× screen.
const TARGET: u32 = 2 * theme::ICON_SIZE as u32;

/// Where to look, from the XDG variables.
struct Env {
    /// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`.
    data_dirs: Vec<PathBuf>,
    /// Where icon themes live, in order: `~/.icons`, then `icons` in each data dir.
    icon_dirs: Vec<PathBuf>,
    /// The user's icon theme, if they set one.
    theme: Option<String>,
}

impl Env {
    fn from_process() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let data_home =
            var("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
        let config_home =
            var("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
        let data_dirs =
            var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let mut dirs = vec![data_home];
        dirs.extend(std::env::split_paths(&data_dirs));
        Self::new(dirs, Some(home.join(".icons")), &config_home)
    }

    fn new(data_dirs: Vec<PathBuf>, home_icons: Option<PathBuf>, config_home: &Path) -> Self {
        let icon_dirs = home_icons
            .into_iter()
            .chain(data_dirs.iter().map(|dir| dir.join("icons")))
            .collect();
        Self {
            data_dirs,
            icon_dirs,
            theme: gtk_icon_theme(config_home),
        }
    }
}

/// `gtk-icon-theme-name` from GTK 3's `settings.ini`, or GTK 4's.
fn gtk_icon_theme(config_home: &Path) -> Option<String> {
    ["gtk-3.0", "gtk-4.0"].iter().find_map(|gtk| {
        let settings = fs::read_to_string(config_home.join(gtk).join("settings.ini")).ok()?;
        settings.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches('"');
            (key.trim() == "gtk-icon-theme-name" && !value.is_empty()).then(|| value.to_string())
        })
    })
}

/// The icon file for `app_id`.
fn find(env: &Env, app_id: &str) -> Option<PathBuf> {
    let name = desktop_icon(env, app_id);
    if let Some(name) = &name
        && Path::new(name).is_absolute()
    {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    let lowercase = app_id.to_lowercase();
    [name.as_deref(), Some(app_id), Some(&lowercase)]
        .into_iter()
        .flatten()
        .find_map(|name| lookup(env, name))
}

/// The `Icon=` of the app's desktop entry: the one named after it, else the one whose
/// `StartupWMClass` is it, else one named after it ignoring case or a reverse-DNS prefix
/// (`org.gnome.Nautilus` for `nautilus`).
fn desktop_icon(env: &Env, app_id: &str) -> Option<String> {
    let entries: Vec<PathBuf> = env
        .data_dirs
        .iter()
        .filter_map(|dir| fs::read_dir(dir.join("applications")).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
        .collect();
    let stem = |path: &PathBuf| path.file_stem()?.to_str().map(str::to_string);

    if let Some(icon) = entries
        .iter()
        .filter(|path| stem(path).as_deref() == Some(app_id))
        .find_map(|path| desktop_entry(path).icon)
    {
        return Some(icon);
    }
    if let Some(icon) = entries.iter().find_map(|path| {
        let entry = desktop_entry(path);
        let class = entry.wm_class?;
        class
            .eq_ignore_ascii_case(app_id)
            .then_some(entry.icon)
            .flatten()
    }) {
        return Some(icon);
    }
    entries
        .iter()
        .filter(|path| {
            stem(path).is_some_and(|stem| {
                stem.eq_ignore_ascii_case(app_id)
                    || stem
                        .rsplit('.')
                        .next()
                        .is_some_and(|last| last.eq_ignore_ascii_case(app_id))
            })
        })
        .find_map(|path| desktop_entry(path).icon)
}

#[derive(Default)]
struct DesktopEntry {
    icon: Option<String>,
    wm_class: Option<String>,
}

fn desktop_entry(path: &Path) -> DesktopEntry {
    let mut entry = DesktopEntry::default();
    let Ok(text) = fs::read_to_string(path) else {
        return entry;
    };
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_main) else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Icon" if !value.is_empty() => entry.icon = Some(value.to_string()),
            "StartupWMClass" if !value.is_empty() => entry.wm_class = Some(value.to_string()),
            _ => {}
        }
    }
    entry
}

/// The best file for the icon `name`: from the user's theme or a theme it inherits, then
/// `hicolor`, then `pixmaps`.
fn lookup(env: &Env, name: &str) -> Option<PathBuf> {
    let mut themes: Vec<String> = env.theme.iter().cloned().collect();
    let mut next = 0;
    while next < themes.len() {
        let theme = themes[next].clone();
        next += 1;
        let indexes = indexes(env, &theme);
        if let Some(path) = best(&indexes, name) {
            return Some(path);
        }
        for index in &indexes {
            for parent in &index.inherits {
                if !themes.contains(parent) {
                    themes.push(parent.clone());
                }
            }
        }
    }
    if !themes.iter().any(|theme| theme == "hicolor")
        && let Some(path) = best(&indexes(env, "hicolor"), name)
    {
        return Some(path);
    }
    env.data_dirs
        .iter()
        .map(|dir| dir.join("pixmaps"))
        .flat_map(|dir| ["svg", "png"].map(|ext| dir.join(format!("{name}.{ext}"))))
        .find(|path| path.is_file())
}

/// One copy of a theme, in one of the icon dirs.
struct Index {
    base: PathBuf,
    inherits: Vec<String>,
    /// Each subdirectory, with the pixel size its icons are drawn at, or `None` for scalable.
    dirs: Vec<(String, Option<u32>)>,
}

/// Every copy of `theme` that has an `index.theme`.
fn indexes(env: &Env, theme: &str) -> Vec<Index> {
    env.icon_dirs
        .iter()
        .map(|dir| dir.join(theme))
        .filter_map(|base| {
            let text = fs::read_to_string(base.join("index.theme")).ok()?;
            Some(parse_index(base, &text))
        })
        .collect()
}

fn parse_index(base: PathBuf, text: &str) -> Index {
    let mut sections: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    let mut current = "";
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current = name;
        } else if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(current)
                .or_default()
                .insert(key.trim(), value.trim());
        }
    }
    let main = sections.get("Icon Theme");
    let list = |key| -> Vec<String> {
        main.and_then(|main| main.get(key))
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let dirs = list("Directories")
        .into_iter()
        .chain(list("ScaledDirectories"))
        .filter_map(|dir| {
            let section = sections.get(dir.as_str())?;
            let number = |key| {
                section
                    .get(key)
                    .and_then(|value: &&str| value.parse::<u32>().ok())
            };
            let size = number("Size")? * number("Scale").unwrap_or(1);
            let scalable = section.get("Type") == Some(&"Scalable");
            Some((dir, (!scalable).then_some(size)))
        })
        .collect();
    Index {
        base,
        inherits: list("Inherits"),
        dirs,
    }
}

/// The file in `indexes` closest to [`TARGET`]: the smallest one at least that big, else the
/// biggest, with scalable ones counting as big enough and SVGs winning ties.
fn best(indexes: &[Index], name: &str) -> Option<PathBuf> {
    let candidates = indexes.iter().flat_map(|index| {
        index.dirs.iter().flat_map(move |(dir, size)| {
            [("svg", true), ("png", false)]
                .into_iter()
                .filter_map(move |(ext, vector)| {
                    let path = index.base.join(dir).join(format!("{name}.{ext}"));
                    path.is_file().then_some((path, *size, vector))
                })
        })
    });
    // Lower is better: big enough first, then the closest size, then vector over raster.
    candidates
        .min_by_key(|(_, size, vector)| match size {
            None => (0, 0, !*vector),
            Some(size) if *size >= TARGET => (0, size - TARGET, !*vector),
            Some(size) => (1, TARGET - size, !*vector),
        })
        .map(|(path, ..)| path)
}

fn read(path: &Path) -> Option<AppIcon> {
    let bytes = fs::read(path).ok()?;
    let pixmap = match path.extension()?.to_str()? {
        "svg" | "svgz" => render_svg(&bytes)?,
        "png" => shrink(Pixmap::decode_png(&bytes).ok()?)?,
        _ => return None,
    };
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect::<Vec<_>>();
    Some(AppIcon(image::Handle::from_rgba(
        pixmap.width(),
        pixmap.height(),
        rgba,
    )))
}

/// The transform that fits `width` × `height` into a [`TARGET`] square, centered.
fn fit(width: f32, height: f32) -> Transform {
    let scale = TARGET as f32 / width.max(height);
    let x = (TARGET as f32 - width * scale) / 2.0;
    let y = (TARGET as f32 - height * scale) / 2.0;
    Transform::from_scale(scale, scale).post_translate(x, y)
}

fn render_svg(bytes: &[u8]) -> Option<Pixmap> {
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let mut pixmap = Pixmap::new(TARGET, TARGET)?;
    resvg::render(
        &tree,
        fit(size.width(), size.height()),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

/// Scales a raster icon to fit [`TARGET`]. Big ones are halved first, as often as it takes to
/// get close, so no pixels are skipped on the way down.
fn shrink(mut pixmap: Pixmap) -> Option<Pixmap> {
    while pixmap.width().min(pixmap.height()) >= 2 * TARGET {
        let mut half = Pixmap::new(pixmap.width() / 2, pixmap.height() / 2)?;
        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        half.draw_pixmap(
            0,
            0,
            pixmap.as_ref(),
            &paint,
            Transform::from_scale(0.5, 0.5),
            None,
        );
        pixmap = half;
    }
    if pixmap.width() == TARGET && pixmap.height() == TARGET {
        return Some(pixmap);
    }
    let mut fitted = Pixmap::new(TARGET, TARGET)?;
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let transform = fit(pixmap.width() as f32, pixmap.height() as f32);
    fitted.draw_pixmap(0, 0, pixmap.as_ref(), &paint, transform, None);
    Some(fitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory tree, removed when dropped.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("eave-apps-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn file(&self, path: &str, contents: &str) -> PathBuf {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        fn env(&self) -> Env {
            Env::new(vec![self.0.join("share")], None, &self.0.join("config"))
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const HICOLOR: &str = "[Icon Theme]\nDirectories=16x16/apps,48x48/apps,scalable/apps\n\
        [16x16/apps]\nSize=16\n[48x48/apps]\nSize=48\n[scalable/apps]\nSize=128\nType=Scalable\n";

    #[test]
    fn matches_desktop_entries() {
        let tree = Tree::new("entries");
        tree.file(
            "share/applications/kitty.desktop",
            "[Desktop Entry]\nIcon=kitty-cat\n",
        );
        tree.file(
            "share/applications/chromium.desktop",
            "[Desktop Entry]\nIcon=chromium\nStartupWMClass=Chromium-browser\n",
        );
        tree.file(
            "share/applications/org.gnome.Nautilus.desktop",
            "[Desktop Entry]\nIcon=nautilus-icon\n[Desktop Action new]\nIcon=other\n",
        );
        let env = tree.env();
        assert_eq!(desktop_icon(&env, "kitty").as_deref(), Some("kitty-cat"));
        assert_eq!(
            desktop_icon(&env, "chromium-browser").as_deref(),
            Some("chromium")
        );
        assert_eq!(
            desktop_icon(&env, "nautilus").as_deref(),
            Some("nautilus-icon")
        );
        assert_eq!(desktop_icon(&env, "foot"), None);
    }

    #[test]
    fn follows_the_theme_then_hicolor() {
        let tree = Tree::new("themes");
        tree.file(
            "config/gtk-3.0/settings.ini",
            "[Settings]\ngtk-icon-theme-name=Mine\n",
        );
        tree.file(
            "share/icons/Mine/index.theme",
            "[Icon Theme]\nInherits=Base\nDirectories=apps\n[apps]\nSize=32\n",
        );
        tree.file("share/icons/Base/index.theme", HICOLOR);
        tree.file("share/icons/hicolor/index.theme", HICOLOR);
        let mine = tree.file("share/icons/Mine/apps/kitty.png", "");
        let base = tree.file("share/icons/Base/scalable/apps/firefox.svg", "");
        let hicolor = tree.file("share/icons/hicolor/48x48/apps/foot.png", "");
        let pixmap = tree.file("share/pixmaps/xterm.png", "");
        let env = tree.env();
        assert_eq!(env.theme.as_deref(), Some("Mine"));
        assert_eq!(lookup(&env, "kitty"), Some(mine));
        assert_eq!(lookup(&env, "firefox"), Some(base));
        assert_eq!(lookup(&env, "foot"), Some(hicolor));
        assert_eq!(lookup(&env, "xterm"), Some(pixmap));
        assert_eq!(lookup(&env, "nothing"), None);
    }

    #[test]
    fn picks_the_closest_size_up() {
        let tree = Tree::new("sizes");
        tree.file("share/icons/hicolor/index.theme", HICOLOR);
        let small = tree.file("share/icons/hicolor/16x16/apps/a.png", "");
        let medium = tree.file("share/icons/hicolor/48x48/apps/a.png", "");
        let env = tree.env();
        assert_eq!(lookup(&env, "a"), Some(medium.clone()));
        let scalable = tree.file("share/icons/hicolor/scalable/apps/a.svg", "");
        assert_eq!(
            lookup(&env, "a"),
            Some(scalable),
            "scalable beats any fixed size"
        );
        fs::remove_file(&medium).unwrap();
        fs::remove_file(tree.0.join("share/icons/hicolor/scalable/apps/a.svg")).unwrap();
        assert_eq!(lookup(&env, "a"), Some(small), "too small beats nothing");
    }

    #[test]
    fn finds_by_class_when_the_entry_has_no_icon() {
        let tree = Tree::new("class");
        tree.file("share/icons/hicolor/index.theme", HICOLOR);
        let icon = tree.file("share/icons/hicolor/48x48/apps/blender.png", "");
        assert_eq!(find(&tree.env(), "Blender"), Some(icon));
    }
}
