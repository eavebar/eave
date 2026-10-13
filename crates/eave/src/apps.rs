//! The host side of [`eave_api::ui::app_icon`]: finds an app's icon the way freedesktop desktops
//! do, and loads it. The app's desktop entry names the icon (matched by file name or
//! `StartupWMClass`), and the icon theme GTK apps use (`gtk-icon-theme-name` in `settings.ini`,
//! read as a file, no dconf) finds the file, falling back through its `Inherits=` chain to
//! `hicolor`, then `pixmaps`. All of it is file I/O, so it runs on tokio's blocking pool, once per
//! app id; the host caches the result. Every icon is drawn there, once, into a small bitmap
//! ([`TARGET`] pixels square), so drawing the bar only copies pixels and big icons don't stay in
//! memory.
//!
//! Icon files a plugin ships ([`eave_api::ui::image`], [`eave_api::ui::symbol`]) go the same way,
//! minus the lookup: the bytes are compiled into the plugin, so they're drawn once and kept by
//! their address.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use iced::widget::image;
use resvg::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use resvg::usvg;

use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Widget, layout, mouse, renderer};
use iced::{Color, Element, Length, Rectangle, Size};

use crate::cover::Cover;
use crate::{squish, theme};

/// A loaded icon, ready to draw.
#[derive(Debug, Clone)]
pub struct AppIcon(pub image::Handle);

impl AppIcon {
    /// The icon at the bar's icon size, faded to `opacity`.
    pub fn view<'a, M: 'a>(&self, opacity: f32) -> Element<'a, M> {
        Element::new(Picture {
            handle: self.0.clone(),
            opacity,
            size: theme::ICON_SIZE,
        })
    }
}

/// An icon or cover on screen, `size` square. Unlike iced's image, it squishes with its island
/// ([`squish`]): an image can be drawn into any rectangle, so it's drawn into a stretched one.
pub struct Picture {
    pub handle: image::Handle,
    pub opacity: f32,
    pub size: f32,
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Picture {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.size, self.size))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::image::Renderer as _;

        let squish = squish::current();
        let bounds = squish.rect(layout.bounds());
        renderer.draw_image(
            iced::advanced::image::Image {
                opacity: self.opacity,
                // Snapping to whole pixels would make a squish jitter.
                snap: squish.is_none(),
                ..iced::advanced::image::Image::new(self.handle.clone())
            },
            bounds,
            bounds,
        );
    }
}

/// The icons the host knows so far, for every widget to share.
#[derive(Debug, Default)]
pub struct Icons {
    /// By app id: `None` for an app with no icon to be found.
    pub apps: HashMap<String, Option<AppIcon>>,
    /// The files plugins ship: `None` for one that isn't an SVG or PNG it can draw.
    pub files: HashMap<FileKey, Option<FileIcon>>,
    /// Sprites' frames, at their own size: `None` for one that can't be drawn.
    pub frames: HashMap<FileKey, Option<Frame>>,
    /// Covers by URL: `None` for one that couldn't be had.
    pub covers: HashMap<String, Option<Cover>>,
}

/// The icons a view shows, so the host can load the ones it doesn't have.
#[derive(Debug, Default)]
pub struct Wanted {
    pub apps: Vec<String>,
    pub files: Vec<&'static [u8]>,
    pub frames: Vec<&'static [u8]>,
    pub covers: Vec<String>,
}

/// Which shipped file: its address and length. The bytes are `'static`, compiled into a plugin,
/// so the address stays theirs and comparing it saves hashing the file on every view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileKey(usize, usize);

impl FileKey {
    pub fn of(file: &'static [u8]) -> Self {
        Self(file.as_ptr() as usize, file.len())
    }
}

/// How many colors a symbol keeps drawn: the tones it shows in, plus a hover or two.
const TINTS: usize = 6;

/// A shipped icon file, drawn: in its own colors, and its shape for drawing it in any color.
#[derive(Debug, Clone)]
pub struct FileIcon {
    image: image::Handle,
    /// Each pixel's coverage, row by row.
    mask: Vec<u8>,
    /// The shape drawn in the colors it was asked for so far, the latest last. The same color
    /// gets the same handle, so the renderer uploads it once.
    tinted: RefCell<Vec<(Color, image::Handle)>>,
}

impl FileIcon {
    /// The file in its own colors, faded to `opacity`.
    pub fn view<'a, M: 'a>(&self, opacity: f32) -> Element<'a, M> {
        AppIcon(self.image.clone()).view(opacity)
    }

    /// The file's shape, filled with `color`.
    pub fn symbol<'a, M: 'a>(&self, color: Color) -> Element<'a, M> {
        let mut tinted = self.tinted.borrow_mut();
        let handle = match tinted.iter().position(|(drawn, _)| *drawn == color) {
            Some(at) => tinted[at].1.clone(),
            None => {
                let [red, green, blue, alpha] = color.into_rgba8();
                let rgba = self
                    .mask
                    .iter()
                    .flat_map(|coverage| {
                        let alpha = (u16::from(*coverage) * u16::from(alpha) / 255) as u8;
                        [red, green, blue, alpha]
                    })
                    .collect::<Vec<_>>();
                let handle = image::Handle::from_rgba(TARGET, TARGET, rgba);
                if tinted.len() == TINTS {
                    tinted.remove(0);
                }
                tinted.push((color, handle.clone()));
                handle
            }
        };
        AppIcon(handle).view(1.0)
    }
}

/// A sprite's frame: the file at its own size, to be scaled up pixel by pixel.
#[derive(Debug, Clone)]
pub struct Frame {
    pub handle: image::Handle,
    pub size: Size,
}

/// The largest frame side taken, so a mistaken file can't take all the memory.
const MAX_FRAME: u32 = 1024;

/// Decodes a sprite's frame, off the UI thread.
pub async fn load_frame(file: &'static [u8]) -> Option<Frame> {
    tokio::task::spawn_blocking(move || draw_frame(file))
        .await
        .ok()
        .flatten()
}

/// A PNG as it is, an SVG at its own size.
fn draw_frame(file: &[u8]) -> Option<Frame> {
    let pixmap = if file.starts_with(b"\x89PNG") {
        Pixmap::decode_png(file).ok()?
    } else {
        let tree = usvg::Tree::from_data(file, &usvg::Options::default()).ok()?;
        let size = tree.size().to_int_size();
        let mut pixmap = Pixmap::new(size.width(), size.height())?;
        resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());
        pixmap
    };
    if pixmap.width().max(pixmap.height()) > MAX_FRAME {
        return None;
    }
    Some(Frame {
        handle: handle(&pixmap),
        size: Size::new(pixmap.width() as f32, pixmap.height() as f32),
    })
}

/// Draws a shipped icon file, off the UI thread.
pub async fn load_file(file: &'static [u8]) -> Option<FileIcon> {
    tokio::task::spawn_blocking(move || draw_file(file))
        .await
        .ok()
        .flatten()
}

/// A PNG by its signature, anything else as an SVG.
fn draw_file(file: &[u8]) -> Option<FileIcon> {
    let pixmap = decode(file, !file.starts_with(b"\x89PNG"))?;
    Some(FileIcon {
        image: handle(&pixmap),
        mask: pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect(),
        tinted: RefCell::default(),
    })
}

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
    let svg = match path.extension()?.to_str()? {
        "svg" | "svgz" => true,
        "png" => false,
        _ => return None,
    };
    let pixmap = decode(&fs::read(path).ok()?, svg)?;
    Some(AppIcon(handle(&pixmap)))
}

/// An SVG or PNG file drawn into a [`TARGET`] square.
fn decode(bytes: &[u8], svg: bool) -> Option<Pixmap> {
    if svg {
        render_svg(bytes)
    } else {
        shrink(Pixmap::decode_png(bytes).ok()?)
    }
}

fn handle(pixmap: &Pixmap) -> image::Handle {
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect::<Vec<_>>();
    image::Handle::from_rgba(pixmap.width(), pixmap.height(), rgba)
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

    #[test]
    fn a_shipped_file_is_drawn_once_and_its_shape_per_color() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="0" y="0" width="5" height="10" fill="red"/></svg>"#;
        let icon = draw_file(svg).unwrap();
        assert_eq!(icon.mask.len(), (TARGET * TARGET) as usize);
        assert_eq!(icon.mask[0], 255, "the left half is covered");
        assert_eq!(icon.mask[TARGET as usize - 1], 0, "the right half isn't");

        let tinted = |icon: &FileIcon, color| {
            let _: Element<'_, ()> = icon.symbol(color);
            icon.tinted.borrow().last().unwrap().1.id()
        };
        let white = tinted(&icon, Color::WHITE);
        assert_eq!(tinted(&icon, Color::WHITE), white, "drawn once per color");
        assert_ne!(tinted(&icon, Color::BLACK), white);
        for shade in 0..TINTS {
            tinted(&icon, Color::from_rgb(shade as f32 / 10.0, 0.0, 0.0));
        }
        assert_eq!(
            icon.tinted.borrow().len(),
            TINTS,
            "only the latest are kept"
        );

        assert!(draw_file(b"not an icon").is_none());
    }

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
