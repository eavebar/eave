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
//! live = true     # optional: a live island, snug around its items
//! attach = ["top", "right"]   # optional: flush with these screen edges (D-70)
//!
//! [[dormer]]
//! widget = "apps"   # only in the dormer, with its actions: not in the bar (D-84)
//! ```
//!
//! Four keys sit above the zones. `renderer = "gpu"` (the default) or `"cpu"` (D-67), read when
//! the bar starts. `notch`: `"auto"` (the default) finds a MacBook's, `false` ignores it, and a
//! table says what it is (D-69). `screen`, how the screen's edges are rounded or hidden. Both are
//! in physical pixels, and `eave calibrate` measures and writes them (D-71). And `islands`, how
//! round the islands are (D-73):
//!
//! ```toml
//! notch = { width = 370, height = 66, radius = 20, smoothing = 0.6, shoulder = 8, top = 0, grow = [4, 4] }
//! screen = { corners = [20, 20, 0, 0], smoothing = 0.6, inset = [0, 0, 0, 0], grow = 4 }
//! islands = { radius = "auto", smoothing = "auto" }
//! ```
//!
//! A notch's `radius` rounds its lower corners, `shoulder` rounds it outwards where it meets the
//! top edge, and `top` above 0 makes it a cut-out that far down instead. The bar draws it black
//! (D-75), `grow` pixels bigger on each side and below than it is. `smoothing` 0 is a
//! circle's corner, up to 1 a squircle's. A screen's `corners` go top left, top right, bottom
//! right, bottom left, and its `inset` top, right, bottom, left: the rows and columns hidden under
//! the bezel. Either takes one number for all four. The bar draws the top edge black too, `grow`
//! pixels thicker than it is (D-81), and hangs everything from there.
//!
//! `islands` rounds the islands' corners that face neither a screen corner nor the notch (D-73),
//! also in physical pixels. `"auto"`, the default, takes the notch's, then the screen's.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Center,
    Right,
    /// Not in the bar: widgets that are there only for their actions, in the dormer (D-84).
    Dormer,
}

impl Zone {
    pub const ALL: [Zone; 4] = [Zone::Left, Zone::Center, Zone::Right, Zone::Dormer];

    fn key(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Center => "center",
            Zone::Right => "right",
            Zone::Dormer => "dormer",
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
    /// Whether its island is a live one, set on the marker that starts it.
    pub live: bool,
    /// The screen edges its island is attached to, set on the marker that starts it.
    pub attach: Edges,
    pub options: toml::Table,
}

/// The screen edges an island is attached to (D-70): it reaches them, with no gap and no corners
/// there. Only the first island on the left can be attached left, and only the last on the right
/// can be attached right.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Edges {
    pub top: bool,
    pub left: bool,
    pub right: bool,
}

/// What the bar knows of the notch at the top of the screen (D-69).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Notch {
    /// Found on a MacBook that shows it, none anywhere else.
    #[default]
    Auto,
    /// No notch, whatever the screen has.
    Off,
    /// This one, as measured.
    Fixed(NotchShape),
}

/// A notch as config.toml has it, in physical pixels, centered at the top of the screen.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct NotchShape {
    pub width: f32,
    /// Without one, as tall as the bar's islands reach.
    pub height: Option<f32>,
    /// Its lower corners'.
    pub radius: f32,
    /// 0 rounds its corners and shoulders like a circle, up to 1 like a squircle.
    pub smoothing: f32,
    /// How much it's rounded outwards where it meets the top edge.
    pub shoulder: f32,
    /// 0 hangs from the top edge; more is a cut-out this far below it, rounded all round.
    pub top: f32,
    /// How much bigger the bar draws it than it is: on each side, and below (D-75).
    pub grow: [f32; 2],
}

/// How the screen's own edges are shaped, in physical pixels (D-71). All zero is a plain
/// rectangle, every pixel of it shown.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Screen {
    /// Each corner's radius: top left, top right, bottom right, bottom left.
    pub corners: [f32; 4],
    /// 0 rounds the corners like a circle, up to 1 like a squircle.
    pub smoothing: f32,
    /// The rows and columns hidden under the bezel: top, right, bottom, left.
    pub inset: [f32; 4],
    /// How much thicker the bar draws the top edge than it is (D-81).
    pub grow: f32,
}

/// How the islands' corners are rounded where nothing else says (D-73): where they don't face a
/// screen corner or the notch. `None` is `"auto"`: the notch's, then the screen's.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Islands {
    /// In physical pixels.
    pub radius: Option<f32>,
    /// 0 rounds the corners like a circle, up to 1 like a squircle.
    pub smoothing: Option<f32>,
}

/// What draws the bar (D-67).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    /// wgpu, falling back to tiny-skia where there's no GPU to use.
    #[default]
    Gpu,
    /// tiny-skia only.
    Cpu,
}

impl Renderer {
    /// What iced's `ICED_BACKEND` is set to for it: the backends to try, in order.
    pub fn backends(self) -> &'static str {
        match self {
            Renderer::Gpu => "wgpu,tiny-skia",
            Renderer::Cpu => "tiny-skia",
        }
    }
}

/// Entries in bar order, plus a warning for everything that was skipped.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Config {
    pub entries: Vec<Entry>,
    pub renderer: Renderer,
    pub notch: Notch,
    pub screen: Screen,
    pub islands: Islands,
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
    match table.remove("renderer") {
        Some(toml::Value::String(name)) if name == "gpu" => config.renderer = Renderer::Gpu,
        Some(toml::Value::String(name)) if name == "cpu" => config.renderer = Renderer::Cpu,
        Some(_) => config
            .warnings
            .push("`renderer` must be \"gpu\" or \"cpu\", using \"gpu\"".into()),
        None => {}
    }
    if let Some(value) = table.remove("notch") {
        config.notch = notch(value, &mut config.warnings);
    }
    if let Some(value) = table.remove("screen") {
        config.screen = screen(value, &mut config.warnings);
    }
    if let Some(value) = table.remove("islands") {
        config.islands = islands(value, &mut config.warnings);
    }

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
        let mut live = false;
        let mut attach = Edges::default();
        // Each island marker: the island it starts, where it is, and what it attaches.
        let mut markers = Vec::new();
        let first = config.entries.len();
        for (index, item) in items.into_iter().enumerate() {
            let at = format!("{}[{index}]", zone.key());
            let toml::Value::Table(mut options) = item else {
                config.warnings.push(format!("{at} is not a table"));
                continue;
            };
            let Some(widget) = options.remove("widget") else {
                // A marker: a divider, or the start of another island.
                let marker = |options: &toml::Table, key| options.get(key) == Some(&true.into());
                let island_marker = marker(&options, "island")
                    && options
                        .keys()
                        .all(|key| matches!(key.as_str(), "island" | "live" | "attach"));
                if island_marker {
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
                    live = match options.get("live") {
                        Some(toml::Value::Boolean(value)) => *value,
                        Some(_) => {
                            config
                                .warnings
                                .push(format!("{at}: `live` must be true or false, ignoring it"));
                            false
                        }
                        None => false,
                    };
                    attach = match options.get("attach") {
                        Some(value) => edges(value, &at, &mut config.warnings),
                        None => Edges::default(),
                    };
                    markers.push((island, at, attach));
                } else if options.len() == 1 && marker(&options, "divider") {
                    divider = true;
                } else {
                    config.warnings.push(format!(
                        "{at} has no `widget = \"name\"` (or is `divider = true` or \
                         `island = true`, with `live = true` or `attach = [\"top\"]` if you \
                         like)"
                    ));
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
                live,
                attach,
                options,
            });
            filled = true;
        }
        // Only an island at the screen's side can reach it.
        let last = config.entries[first..].last().map(|entry| entry.island);
        for (island, at, edges) in markers {
            let mut kept = edges;
            if edges.left && !(zone == Zone::Left && island == 0) {
                config.warnings.push(format!(
                    "{at}: only the first island on the left can be attached left, ignoring it"
                ));
                kept.left = false;
            }
            if edges.right && !(zone == Zone::Right && Some(island) == last) {
                config.warnings.push(format!(
                    "{at}: only the last island on the right can be attached right, ignoring it"
                ));
                kept.right = false;
            }
            if kept != edges {
                for entry in &mut config.entries[first..] {
                    if entry.island == island {
                        entry.attach = kept;
                    }
                }
            }
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

/// `attach = ["top", "left"]`, or just `attach = "top"`.
fn edges(value: &toml::Value, at: &str, warnings: &mut Vec<String>) -> Edges {
    let names = match value {
        toml::Value::Array(names) => names.as_slice(),
        name => std::slice::from_ref(name),
    };
    let mut edges = Edges::default();
    for name in names {
        match name.as_str() {
            Some("top") => edges.top = true,
            Some("left") => edges.left = true,
            Some("right") => edges.right = true,
            Some(other) => warnings.push(format!(
                "{at}: `attach` takes \"top\", \"left\" and \"right\", ignoring \"{other}\""
            )),
            None => warnings.push(format!(
                "{at}: `attach` takes \"top\", \"left\" and \"right\", ignoring a {}",
                name.type_str()
            )),
        }
    }
    edges
}

/// A size in pixels: an integer or a float, at least 0.
fn pixels(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Integer(size) if *size >= 0 => Some(*size as f32),
        toml::Value::Float(size) if *size >= 0.0 && size.is_finite() => Some(*size as f32),
        _ => None,
    }
}

/// A smoothing: 0 to 1.
fn smoothing(value: &toml::Value) -> Option<f32> {
    pixels(value).filter(|smoothing| *smoothing <= 1.0)
}

/// Four sizes, or one for all four.
fn four(value: &toml::Value) -> Option<[f32; 4]> {
    match value {
        toml::Value::Array(sizes) if sizes.len() == 4 => {
            let mut four = [0.0; 4];
            for (size, value) in four.iter_mut().zip(sizes) {
                *size = pixels(value)?;
            }
            Some(four)
        }
        size => pixels(size).map(|size| [size; 4]),
    }
}

/// `[4, 4]`: two sizes in pixels, or one for both.
fn two(value: &toml::Value) -> Option<[f32; 2]> {
    match value {
        toml::Value::Array(sizes) if sizes.len() == 2 => {
            Some([pixels(&sizes[0])?, pixels(&sizes[1])?])
        }
        size => pixels(size).map(|size| [size; 2]),
    }
}

/// `notch = "auto"`, `false`, or `{ width = 370, height = 66, … }`.
fn notch(value: toml::Value, warnings: &mut Vec<String>) -> Notch {
    match value {
        toml::Value::String(name) if name == "auto" => return Notch::Auto,
        toml::Value::Boolean(false) => return Notch::Off,
        toml::Value::Table(table) => {
            let mut shape = NotchShape::default();
            let read = table.iter().all(|(key, value)| {
                let field = match key.as_str() {
                    "width" => &mut shape.width,
                    "height" => shape.height.insert(0.0),
                    "radius" => &mut shape.radius,
                    "shoulder" => &mut shape.shoulder,
                    "top" => &mut shape.top,
                    "grow" => {
                        return two(value).map(|grow| shape.grow = grow).is_some();
                    }
                    "smoothing" => {
                        return smoothing(value)
                            .map(|value| shape.smoothing = value)
                            .is_some();
                    }
                    _ => return false,
                };
                pixels(value).map(|value| *field = value).is_some()
            });
            // Everything else may be left out, but not how wide it is.
            if read && shape.width > 0.0 {
                return Notch::Fixed(shape);
            }
        }
        _ => {}
    }
    warnings.push(
        "`notch` must be \"auto\", false or { width = 370, height = 66 } in physical pixels, \
         with radius, smoothing, shoulder, top and grow if you like, using \"auto\""
            .into(),
    );
    Notch::Auto
}

/// `screen = { corners = [20, 20, 0, 0], smoothing = 0.6, inset = 0, grow = 0 }`.
fn screen(value: toml::Value, warnings: &mut Vec<String>) -> Screen {
    let mut screen = Screen::default();
    let toml::Value::Table(table) = value else {
        warnings.push("`screen` must be a table, like { corners = 20 }, ignoring it".into());
        return screen;
    };
    for (key, value) in &table {
        let read = match key.as_str() {
            "corners" => four(value).map(|corners| screen.corners = corners),
            "inset" => four(value).map(|inset| screen.inset = inset),
            "smoothing" => smoothing(value).map(|value| screen.smoothing = value),
            "grow" => pixels(value).map(|grow| screen.grow = grow),
            _ => {
                warnings.push(format!(
                    "`screen` takes corners, smoothing, inset and grow, ignoring `{key}`"
                ));
                continue;
            }
        };
        if read.is_none() {
            warnings.push(format!(
                "`screen.{key}` must be {}, ignoring it",
                if key == "smoothing" {
                    "a number from 0 to 1"
                } else if key == "grow" {
                    "a size in pixels"
                } else {
                    "a size in pixels, or four of them"
                }
            ));
        }
    }
    screen
}

/// `islands = { radius = "auto", smoothing = "auto" }`, or numbers.
fn islands(value: toml::Value, warnings: &mut Vec<String>) -> Islands {
    let mut islands = Islands::default();
    let toml::Value::Table(table) = value else {
        warnings.push("`islands` must be a table, like { radius = \"auto\" }, ignoring it".into());
        return islands;
    };
    for (key, value) in &table {
        let (field, read, what) = match key.as_str() {
            "radius" => (&mut islands.radius, pixels(value), "a size in pixels"),
            "smoothing" => (
                &mut islands.smoothing,
                smoothing(value),
                "a number from 0 to 1",
            ),
            _ => {
                warnings.push(format!(
                    "`islands` takes radius and smoothing, ignoring `{key}`"
                ));
                continue;
            }
        };
        match (value, read) {
            (toml::Value::String(auto), _) if auto == "auto" => *field = None,
            (_, Some(read)) => *field = Some(read),
            _ => warnings.push(format!(
                "`islands.{key}` must be \"auto\" or {what}, using \"auto\""
            )),
        }
    }
    islands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widgets_can_live_in_the_dormer_alone() {
        let config = parse("[[dormer]]\nwidget = \"apps\"\nterminal = \"foot\"").unwrap();
        assert!(config.warnings.is_empty());
        assert_eq!(config.entries.len(), 1);
        assert_eq!(config.entries[0].zone, Zone::Dormer);
        assert_eq!(
            config.entries[0].options.get("terminal"),
            Some(&"foot".into())
        );
    }

    #[test]
    fn the_renderer_is_the_gpu_unless_it_says_cpu() {
        assert_eq!(parse("").unwrap().renderer, Renderer::Gpu);
        assert_eq!(parse("renderer = \"cpu\"").unwrap().renderer, Renderer::Cpu);
        assert_eq!(parse("renderer = \"gpu\"").unwrap().renderer, Renderer::Gpu);
        let odd = parse("renderer = \"vulkan\"").unwrap();
        assert_eq!(odd.renderer, Renderer::Gpu);
        assert_eq!(odd.warnings.len(), 1);
    }

    #[test]
    fn the_notch_is_found_unless_its_off_or_given() {
        assert_eq!(parse("").unwrap().notch, Notch::Auto);
        assert_eq!(parse("notch = \"auto\"").unwrap().notch, Notch::Auto);
        assert_eq!(parse("notch = false").unwrap().notch, Notch::Off);
        assert_eq!(
            parse("notch = { width = 190 }").unwrap().notch,
            Notch::Fixed(NotchShape {
                width: 190.0,
                ..NotchShape::default()
            })
        );
        assert_eq!(
            parse(
                "notch = { width = 370.5, height = 66, radius = 20, smoothing = 0.6, \
                 shoulder = 8, top = 0, grow = [4, 2] }"
            )
            .unwrap()
            .notch,
            Notch::Fixed(NotchShape {
                width: 370.5,
                height: Some(66.0),
                radius: 20.0,
                smoothing: 0.6,
                shoulder: 8.0,
                top: 0.0,
                grow: [4.0, 2.0],
            })
        );
        assert_eq!(
            parse("notch = { width = 190, grow = 3 }").unwrap().notch,
            Notch::Fixed(NotchShape {
                width: 190.0,
                grow: [3.0, 3.0],
                ..NotchShape::default()
            })
        );
        for odd in [
            "notch = true",
            "notch = \"yes\"",
            "notch = { height = 37 }",
            "notch = { width = -1 }",
            "notch = { width = 190, depth = 2 }",
            "notch = { width = 190, smoothing = 2 }",
            "notch = { width = 190, grow = [1, 2, 3] }",
        ] {
            let config = parse(odd).unwrap();
            assert_eq!(config.notch, Notch::Auto, "{odd}");
            assert_eq!(config.warnings.len(), 1, "{odd}");
        }
    }

    #[test]
    fn the_screen_is_a_plain_rectangle_unless_it_says_otherwise() {
        assert_eq!(parse("").unwrap().screen, Screen::default());
        let config =
            parse("screen = { corners = [20, 20.5, 0, 0], smoothing = 0.6, inset = 2, grow = 4 }")
                .unwrap();
        assert_eq!(
            config.screen,
            Screen {
                corners: [20.0, 20.5, 0.0, 0.0],
                smoothing: 0.6,
                inset: [2.0; 4],
                grow: 4.0,
            }
        );
        assert!(config.warnings.is_empty());
        let odd =
            parse("screen = { corners = [1, 2], smoothing = 3, edge = 1, grow = [1, 2] }").unwrap();
        assert_eq!(odd.screen, Screen::default());
        assert_eq!(odd.warnings.len(), 4, "{:?}", odd.warnings);
    }

    #[test]
    fn island_corners_are_auto_unless_given() {
        assert_eq!(parse("").unwrap().islands, Islands::default());
        let config = parse("islands = { radius = \"auto\", smoothing = \"auto\" }").unwrap();
        assert_eq!(config.islands, Islands::default());
        assert!(config.warnings.is_empty());
        let config = parse("islands = { radius = 24, smoothing = 0.5 }").unwrap();
        assert_eq!(
            config.islands,
            Islands {
                radius: Some(24.0),
                smoothing: Some(0.5),
            }
        );
        let odd = parse("islands = { radius = -1, smoothing = 2, shape = 1 }").unwrap();
        assert_eq!(odd.islands, Islands::default());
        assert_eq!(odd.warnings.len(), 3, "{:?}", odd.warnings);
    }

    #[test]
    fn islands_attach_to_the_edges_they_can_reach() {
        let config = parse(
            r#"
            [[left]]
            island = true
            attach = ["top", "left"]
            [[left]]
            widget = "workspaces"
            [[left]]
            island = true
            attach = ["left"]
            [[left]]
            widget = "window"
            [[center]]
            island = true
            attach = "top"
            [[center]]
            widget = "clock"
            [[right]]
            island = true
            attach = ["right", "bottom"]
            [[right]]
            widget = "volume"
            [[right]]
            island = true
            attach = ["top", "right"]
            [[right]]
            widget = "battery"
            "#,
        )
        .unwrap();
        let attach: Vec<_> = config
            .entries
            .iter()
            .map(|entry| (entry.widget.as_str(), entry.attach))
            .collect();
        let edges = |top, left, right| Edges { top, left, right };
        assert_eq!(
            attach,
            [
                ("workspaces", edges(true, true, false)),
                ("window", edges(false, false, false)),
                ("clock", edges(true, false, false)),
                ("volume", edges(false, false, false)),
                ("battery", edges(true, false, true)),
            ]
        );
        assert_eq!(config.warnings.len(), 3, "{:?}", config.warnings);
        assert!(config.warnings[0].contains("only the first island on the left"));
        assert!(config.warnings[1].contains("ignoring \"bottom\""));
        assert!(config.warnings[2].contains("only the last island on the right"));
    }

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
    fn live_markers_make_their_island_live() {
        let config = parse(
            r#"
            [[center]]
            island = true
            live = true
            [[center]]
            widget = "media"
            [[center]]
            island = true
            [[center]]
            widget = "clock"
            [[center]]
            island = true
            live = true
            [[center]]
            widget = "window"
            [[right]]
            island = true
            live = "yes"
            [[right]]
            widget = "battery"
            [[right]]
            island = true
            live = true
            extra = 1
            "#,
        )
        .unwrap();
        let live: Vec<_> = config
            .entries
            .iter()
            .map(|entry| (entry.widget.as_str(), entry.island, entry.live))
            .collect();
        assert_eq!(
            live,
            [
                ("media", 0, true),
                ("clock", 1, false),
                ("window", 2, true),
                ("battery", 0, false),
            ]
        );
        assert_eq!(config.warnings.len(), 2, "{:?}", config.warnings);
        assert!(config.warnings[0].contains("`live` must be true or false"));
        assert!(config.warnings[1].contains("has no `widget"));
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
