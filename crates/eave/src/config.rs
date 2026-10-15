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
//! keys = { refresh = "ctrl+r", details = false }   # and this: its shortcuts' keys, by name
//!
//! [[right]]
//! widget = "media"
//! parts = ["someone/spotify"]   # and this: widgets shown in its bays, not in the bar (D-94)
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
//!
//! A `[monitor."…"]` table says all three for one monitor instead (D-94). It's named after the
//! connector, or after what Hyprland's description starts with: make, model, and serial if only
//! that one unit. Keys it leaves out come from the top level:
//!
//! ```toml
//! [monitor."ViewSonic Corporation XG2431"]
//! screen = { corners = 12 }
//! ```
//!
//! `theme` is `"system"` (the default), `"light"` or `"dark"` (D-92). `"system"` follows the
//! desktop's dark mode, as the settings portal tells it, and is dark when it has no preference.
//!
//! Two more say when the bar and the notifications show (D-88):
//!
//! ```toml
//! bar = { show = "always", fullscreen = "hide", peek = true, style = "islands" }
//! notifications = { fullscreen = "critical" }
//! ```
//!
//! `bar.style = "solid"` draws all of the bar black, down to the islands' bottom, with its lower
//! ends rounded like the screen's top corners, or square with `bar.corners = "square"`. Islands
//! stay islands in it, and popovers flow out of it (D-89). `bar.show` is `"always"` or `"hover"`: hidden above the screen until the pointer touches the
//! top edge, and keeping no room for itself. `bar.fullscreen` is the same on a workspace with a
//! fullscreen window, `"hide"`, `"hover"` or `"show"`. `bar.peek` brings a hidden bar out for a
//! moment when the workspace changes. `notifications.fullscreen` is `"show"`, `"critical"` (only
//! critical ones) or `"hold"`: the rest wait, without running out, until fullscreen ends.

use std::path::{Path, PathBuf};

use crate::shortcut;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// The keys the user gives its shortcuts, by name (D-91).
    pub keys: shortcut::Keys,
    /// For a part (`parts = [...]` in another entry): that entry's index in `entries`. A part is
    /// shown only in its owner's bays (D-94), never in a zone, so its other placement fields are
    /// its owner's and mean nothing.
    pub owner: Option<usize>,
    pub options: toml::Table,
}

impl Entry {
    /// Whether `other` is this entry, but for its keys: new keys need no new widget.
    pub fn same_but_keys(&self, other: &Entry) -> bool {
        fn fields(
            entry: &Entry,
        ) -> (
            &Zone,
            &String,
            &Option<String>,
            &usize,
            &bool,
            &bool,
            &bool,
            &Edges,
            &toml::Table,
        ) {
            let Entry {
                zone,
                widget,
                width_group,
                island,
                collapsible,
                divider,
                live,
                attach,
                keys: _,
                // Indices move whenever an entry is added above; the host compares owners itself.
                owner: _,
                options,
            } = entry;
            (
                zone,
                widget,
                width_group,
                island,
                collapsible,
                divider,
                live,
                attach,
                options,
            )
        }
        fields(self) == fields(other)
    }
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

/// The screen's notch, edges and islands, as they are on one monitor.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenSetting {
    pub notch: Notch,
    /// `None` when config.toml doesn't say: not measured.
    pub screen: Option<Screen>,
    pub islands: Islands,
}

/// A `[monitor."…"]` table: what it says for its monitors instead of the top-level keys. What it
/// leaves out, the top level says (D-94).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MonitorScreen {
    pub notch: Option<Notch>,
    pub screen: Option<Screen>,
    pub islands: Option<Islands>,
}

/// Every screen setting config.toml has: the top level's, and each monitor's.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Screens {
    pub top: ScreenSetting,
    pub monitors: Vec<(String, MonitorScreen)>,
}

impl Config {
    pub fn screens(&self) -> Screens {
        Screens {
            top: ScreenSetting {
                notch: self.notch,
                screen: self.screen,
                islands: self.islands,
            },
            monitors: self.monitors.clone(),
        }
    }
}

impl Screens {
    /// The screen setting for the monitor with this connector name (`eDP-1`) and Hyprland
    /// description (`make model serial`). A table named after the connector wins; then the one
    /// whose name the description starts with, the longest; then the top-level keys.
    pub fn screen_for(&self, name: &str, description: &str) -> ScreenSetting {
        let monitor = self
            .matching(name, description)
            .map(|(_, monitor)| *monitor)
            .unwrap_or_default();
        ScreenSetting {
            notch: monitor.notch.unwrap_or(self.top.notch),
            screen: monitor.screen.or(self.top.screen),
            islands: monitor.islands.unwrap_or(self.top.islands),
        }
    }

    /// The `[monitor."…"]` table for this monitor, and its name, as [`Self::screen_for`] picks it.
    pub fn matching(&self, name: &str, description: &str) -> Option<&(String, MonitorScreen)> {
        self.monitors
            .iter()
            .find(|(key, _)| key == name)
            .or_else(|| {
                self.monitors
                    .iter()
                    .filter(|(key, _)| !key.is_empty() && description.starts_with(key.as_str()))
                    .max_by_key(|(key, _)| key.len())
            })
    }
}

/// When the bar shows (D-88).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    /// Outside fullscreen.
    pub show: Showing,
    /// On a workspace with a fullscreen window.
    pub fullscreen: Showing,
    /// A hidden bar comes out for a moment when the workspace changes.
    pub peek: bool,
    /// Islands on their own, or all of the bar black (D-89).
    pub style: Style,
    /// A solid bar's lower ends: rounded like the screen's top corners, or square.
    pub corners: BarCorners,
}

/// How the bar is drawn (D-89).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Islands floating below the top edge.
    #[default]
    Islands,
    /// All of it black, down to the islands' bottom. The islands are in it, and flow out of it.
    Solid,
}

/// Where a solid bar's bottom meets the screen's sides.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BarCorners {
    /// Curving down into the sides the way the screen's top corners do.
    #[default]
    Round,
    Square,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            show: Showing::Always,
            fullscreen: Showing::Never,
            peek: true,
            style: Style::Islands,
            corners: BarCorners::Round,
        }
    }
}

/// Whether the bar shows on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Showing {
    Always,
    /// Hidden until the pointer touches the top edge.
    Hover,
    /// Hidden. Notifications, the dormer and peeks still bring it out.
    Never,
}

/// When notifications show (D-88).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Notifications {
    pub fullscreen: Held,
}

/// Which notifications wait for fullscreen to end.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// None: they all show.
    None,
    /// All but the critical ones.
    #[default]
    Uncritical,
    /// All of them.
    All,
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

/// Which way round the bar's palette is (D-92).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// The desktop's: light when it prefers light, dark otherwise.
    #[default]
    System,
    Light,
    Dark,
}

/// Entries in bar order, plus a warning for everything that was skipped.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Config {
    pub entries: Vec<Entry>,
    pub renderer: Renderer,
    pub theme: Theme,
    pub notch: Notch,
    /// `None` without a `screen` key: not measured.
    pub screen: Option<Screen>,
    pub islands: Islands,
    /// `[monitor."…"]` tables, by name: the screen settings of the monitors they match (D-94).
    pub monitors: Vec<(String, MonitorScreen)>,
    pub bar: Bar,
    pub notifications: Notifications,
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
    match table.remove("theme") {
        Some(toml::Value::String(name)) if name == "system" => config.theme = Theme::System,
        Some(toml::Value::String(name)) if name == "light" => config.theme = Theme::Light,
        Some(toml::Value::String(name)) if name == "dark" => config.theme = Theme::Dark,
        Some(_) => config
            .warnings
            .push("`theme` must be \"system\", \"light\" or \"dark\", using \"system\"".into()),
        None => {}
    }
    if let Some(value) = table.remove("notch") {
        config.notch = notch(value, &mut config.warnings);
    }
    if let Some(value) = table.remove("screen") {
        config.screen = Some(screen(value, &mut config.warnings));
    }
    if let Some(value) = table.remove("islands") {
        config.islands = islands(value, &mut config.warnings);
    }
    if let Some(value) = table.remove("monitor") {
        config.monitors = monitors(value, &mut config.warnings);
    }
    if let Some(value) = table.remove("bar") {
        config.bar = bar(value, &mut config.warnings);
    }
    if let Some(value) = table.remove("notifications") {
        config.notifications = notifications(value, &mut config.warnings);
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
            let keys = match options.remove("keys") {
                Some(value) => keys(value, &at, &mut config.warnings),
                None => shortcut::Keys::new(),
            };
            let parts = options.remove("parts");
            let owner = config.entries.len();
            config.entries.push(Entry {
                zone,
                widget,
                width_group,
                island,
                collapsible,
                divider: std::mem::take(&mut divider),
                live,
                attach,
                keys,
                owner: None,
                options,
            });
            if let Some(value) = parts {
                let parts = self::parts(value, &at, &mut config.warnings);
                let owner_entry = &config.entries[owner];
                let parts: Vec<_> = parts
                    .into_iter()
                    .map(|(widget, keys, options)| Entry {
                        widget,
                        width_group: None,
                        collapsible: false,
                        divider: false,
                        keys,
                        owner: Some(owner),
                        options,
                        ..owner_entry.clone()
                    })
                    .collect();
                config.entries.extend(parts);
            }
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

/// `parts = ["someone/airpods", { widget = "someone/spotify", client_id = "…" }]`: widgets shown in
/// this one's bays (D-94), each a reference or a table like a zone's entry. A part's table takes
/// its options and `keys`, and nothing else of the host's: it has no place in the bar.
fn parts(
    value: toml::Value,
    at: &str,
    warnings: &mut Vec<String>,
) -> Vec<(String, shortcut::Keys, toml::Table)> {
    let toml::Value::Array(items) = value else {
        warnings.push(format!(
            "{at}: `parts` must be a list of widgets, like `parts = [\"someone/spotify\"]`, \
             ignoring it"
        ));
        return Vec::new();
    };
    let mut parts = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let at = format!("{at}.parts[{index}]");
        let mut options = match item {
            toml::Value::String(widget) => {
                parts.push((widget, shortcut::Keys::new(), toml::Table::new()));
                continue;
            }
            toml::Value::Table(options) => options,
            _ => {
                warnings.push(format!("{at} must be a widget or a table, skipping it"));
                continue;
            }
        };
        let Some(toml::Value::String(widget)) = options.remove("widget") else {
            warnings.push(format!("{at} has no `widget = \"name\"`, skipping it"));
            continue;
        };
        let keys = match options.remove("keys") {
            Some(value) => keys(value, &at, warnings),
            None => shortcut::Keys::new(),
        };
        for key in ["width_group", "collapsible", "parts"] {
            if options.remove(key).is_some() {
                warnings.push(format!("{at}: a part has no `{key}`, ignoring it"));
            }
        }
        parts.push((widget, keys, options));
    }
    parts
}

/// `keys = { chat-1 = "alt+1", chat-10 = false }`: a shortcut's name, and its key or none.
fn keys(value: toml::Value, at: &str, warnings: &mut Vec<String>) -> shortcut::Keys {
    let toml::Value::Table(table) = value else {
        warnings.push(format!(
            "{at}: `keys` must be a table, like `keys = {{ mute = \"ctrl+m\" }}`, ignoring it"
        ));
        return shortcut::Keys::new();
    };
    let mut keys = shortcut::Keys::new();
    for (id, value) in table {
        match value {
            toml::Value::String(text) => match shortcut::parse(&text) {
                Ok(key) => {
                    keys.insert(id, Some(key));
                }
                Err(error) => warnings.push(format!("{at}: keys.{id} = \"{text}\": {error}")),
            },
            toml::Value::Boolean(false) => {
                keys.insert(id, None);
            }
            _ => warnings.push(format!(
                "{at}: keys.{id} must be a key like \"ctrl+k\", or false for none"
            )),
        }
    }
    keys
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

/// `[monitor."eDP-1"]` or `[monitor."ViewSonic Corporation XG2431"]`, each with `notch`, `screen`
/// and `islands` as the top level has them.
fn monitors(value: toml::Value, warnings: &mut Vec<String>) -> Vec<(String, MonitorScreen)> {
    let toml::Value::Table(table) = value else {
        warnings.push(
            "`monitor` must be tables, like [monitor.\"eDP-1\"] with notch, screen and islands, \
             ignoring it"
                .into(),
        );
        return Vec::new();
    };
    let mut monitors = Vec::new();
    for (key, value) in table {
        let toml::Value::Table(settings) = value else {
            warnings.push(format!(
                "`monitor.\"{key}\"` must be a table with notch, screen and islands, ignoring it"
            ));
            continue;
        };
        let mut monitor = MonitorScreen::default();
        for (name, value) in settings {
            // The same words as the top level's, about the monitor's own key.
            let mut said = Vec::new();
            match name.as_str() {
                "notch" => monitor.notch = Some(notch(value, &mut said)),
                "screen" => monitor.screen = Some(screen(value, &mut said)),
                "islands" => monitor.islands = Some(islands(value, &mut said)),
                _ => said.push(format!(
                    "takes notch, screen and islands, ignoring `{name}`"
                )),
            }
            warnings.extend(
                said.into_iter()
                    .map(|warning| format!("monitor.\"{key}\": {warning}")),
            );
        }
        monitors.push((key, monitor));
    }
    monitors
}

/// `bar = { show = "always", fullscreen = "hide", peek = true, style = "islands", corners = "round" }`.
fn bar(value: toml::Value, warnings: &mut Vec<String>) -> Bar {
    let mut bar = Bar::default();
    let toml::Value::Table(table) = value else {
        warnings.push("`bar` must be a table, like { show = \"hover\" }, ignoring it".into());
        return bar;
    };
    for (key, value) in &table {
        let name = value.as_str();
        match key.as_str() {
            "show" => match name {
                Some("always") => bar.show = Showing::Always,
                Some("hover") => bar.show = Showing::Hover,
                _ => warnings
                    .push("`bar.show` must be \"always\" or \"hover\", using \"always\"".into()),
            },
            "fullscreen" => match name {
                Some("show") => bar.fullscreen = Showing::Always,
                Some("hover") => bar.fullscreen = Showing::Hover,
                Some("hide") => bar.fullscreen = Showing::Never,
                _ => warnings.push(
                    "`bar.fullscreen` must be \"hide\", \"hover\" or \"show\", using \"hide\""
                        .into(),
                ),
            },
            "peek" => match value {
                toml::Value::Boolean(peek) => bar.peek = *peek,
                _ => warnings.push("`bar.peek` must be true or false, using true".into()),
            },
            "style" => match name {
                Some("islands") => bar.style = Style::Islands,
                Some("solid") => bar.style = Style::Solid,
                _ => warnings
                    .push("`bar.style` must be \"islands\" or \"solid\", using \"islands\"".into()),
            },
            "corners" => match name {
                Some("round") => bar.corners = BarCorners::Round,
                Some("square") => bar.corners = BarCorners::Square,
                _ => warnings
                    .push("`bar.corners` must be \"round\" or \"square\", using \"round\"".into()),
            },
            _ => warnings.push(format!(
                "`bar` takes show, fullscreen, peek, style and corners, ignoring `{key}`"
            )),
        }
    }
    bar
}

/// `notifications = { fullscreen = "critical" }`.
fn notifications(value: toml::Value, warnings: &mut Vec<String>) -> Notifications {
    let mut notifications = Notifications::default();
    let toml::Value::Table(table) = value else {
        warnings.push(
            "`notifications` must be a table, like { fullscreen = \"show\" }, ignoring it".into(),
        );
        return notifications;
    };
    for (key, value) in &table {
        if key != "fullscreen" {
            warnings.push(format!(
                "`notifications` takes fullscreen, ignoring `{key}`"
            ));
            continue;
        }
        notifications.fullscreen = match value.as_str() {
            Some("show") => Held::None,
            Some("critical") => Held::Uncritical,
            Some("hold") => Held::All,
            _ => {
                warnings.push(
                    "`notifications.fullscreen` must be \"show\", \"critical\" or \"hold\", \
                     using \"critical\""
                        .into(),
                );
                Held::Uncritical
            }
        };
    }
    notifications
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_always_shows_but_hides_in_fullscreen_unless_it_says_otherwise() {
        let plain = parse("").unwrap();
        assert_eq!(plain.bar, Bar::default());
        assert_eq!(
            (plain.bar.show, plain.bar.fullscreen, plain.bar.peek),
            (Showing::Always, Showing::Never, true)
        );
        assert_eq!(plain.notifications.fullscreen, Held::Uncritical);

        let set = parse(
            "bar = { show = \"hover\", fullscreen = \"show\", peek = false }\n\
             notifications = { fullscreen = \"hold\" }",
        )
        .unwrap();
        assert!(set.warnings.is_empty());
        assert_eq!(
            set.bar,
            Bar {
                show: Showing::Hover,
                fullscreen: Showing::Always,
                peek: false,
                ..Bar::default()
            }
        );
        assert_eq!(set.notifications.fullscreen, Held::All);
        let hover = parse("bar = { fullscreen = \"hover\" }").unwrap();
        assert_eq!(hover.bar.fullscreen, Showing::Hover);
        let show = parse("notifications = { fullscreen = \"show\" }").unwrap();
        assert_eq!(show.notifications.fullscreen, Held::None);
    }

    #[test]
    fn the_bar_is_islands_unless_its_solid() {
        let plain = parse("").unwrap();
        assert_eq!(
            (plain.bar.style, plain.bar.corners),
            (Style::Islands, BarCorners::Round)
        );
        let solid = parse("bar = { style = \"solid\", corners = \"square\" }").unwrap();
        assert!(solid.warnings.is_empty());
        assert_eq!(
            (solid.bar.style, solid.bar.corners),
            (Style::Solid, BarCorners::Square)
        );
        let odd = parse("bar = { style = \"glass\", corners = 4 }").unwrap();
        assert_eq!(odd.bar, Bar::default());
        assert_eq!(odd.warnings.len(), 2);
    }

    #[test]
    fn odd_bar_and_notification_settings_are_warned_about() {
        let odd = parse(
            "bar = { show = \"never\", fullscreen = true, peek = \"yes\", side = \"top\" }\n\
             notifications = { fullscreen = \"mute\", sound = false }",
        )
        .unwrap();
        assert_eq!(odd.bar, Bar::default());
        assert_eq!(odd.notifications, Notifications::default());
        assert_eq!(odd.warnings.len(), 6);
        assert_eq!(parse("bar = \"hover\"").unwrap().warnings.len(), 1);
        assert_eq!(parse("notifications = false").unwrap().warnings.len(), 1);
    }

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
    fn the_theme_follows_the_system_unless_it_says_light_or_dark() {
        assert_eq!(parse("").unwrap().theme, Theme::System);
        assert_eq!(parse("theme = \"light\"").unwrap().theme, Theme::Light);
        assert_eq!(parse("theme = \"dark\"").unwrap().theme, Theme::Dark);
        assert_eq!(parse("theme = \"system\"").unwrap().theme, Theme::System);
        let odd = parse("theme = \"sepia\"").unwrap();
        assert_eq!(odd.theme, Theme::System);
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
        assert_eq!(parse("").unwrap().screen, None);
        let config =
            parse("screen = { corners = [20, 20.5, 0, 0], smoothing = 0.6, inset = 2, grow = 4 }")
                .unwrap();
        assert_eq!(
            config.screen,
            Some(Screen {
                corners: [20.0, 20.5, 0.0, 0.0],
                smoothing: 0.6,
                inset: [2.0; 4],
                grow: 4.0,
            })
        );
        assert!(config.warnings.is_empty());
        let odd =
            parse("screen = { corners = [1, 2], smoothing = 3, edge = 1, grow = [1, 2] }").unwrap();
        assert_eq!(odd.screen, Some(Screen::default()));
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
    fn each_monitor_gets_its_table_or_the_top_level() {
        let config = parse(
            "\
notch = false
screen = { corners = 10 }

[monitor.\"eDP-1\"]
notch = { width = 370 }

[monitor.\"ViewSonic Corporation\"]
screen = { corners = 4 }

[monitor.\"ViewSonic Corporation XG2431\"]
screen = { corners = 12 }
islands = { radius = 6 }
",
        )
        .unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        let screens = config.screens();
        let corners = |name, description| {
            screens
                .screen_for(name, description)
                .screen
                .unwrap()
                .corners
        };

        // The connector's table wins, and what it leaves out comes from the top level.
        let laptop = screens.screen_for("eDP-1", "ViewSonic Corporation XG2431 X");
        assert!(matches!(laptop.notch, Notch::Fixed(shape) if shape.width == 370.0));
        assert_eq!(laptop.screen.unwrap().corners, [10.0; 4]);
        // Then the longest start of the description.
        let desk = screens.screen_for("DP-2", "ViewSonic Corporation XG2431 WEE241700714");
        assert_eq!(desk.screen.unwrap().corners, [12.0; 4]);
        assert_eq!(desk.islands.radius, Some(6.0));
        assert_eq!(desk.notch, Notch::Off);
        assert_eq!(corners("DP-3", "ViewSonic Corporation VX2758"), [4.0; 4]);
        // And none: the top level.
        assert_eq!(corners("HDMI-A-1", "Dell Inc. U2720Q"), [10.0; 4]);
        assert_eq!(corners("DP-1", "ViewSonic"), [10.0; 4]);
    }

    #[test]
    fn odd_monitor_tables_are_warned_about() {
        let odd = parse(
            "\
monitor = { \"eDP-1\" = 3 }
",
        )
        .unwrap();
        assert!(odd.monitors.is_empty());
        assert_eq!(odd.warnings.len(), 1, "{:?}", odd.warnings);

        let odd = parse(
            "\
[monitor.\"eDP-1\"]
screen = { corners = -1 }
bar = { show = \"hover\" }
",
        )
        .unwrap();
        assert_eq!(odd.warnings.len(), 2, "{:?}", odd.warnings);
        assert!(
            odd.warnings
                .iter()
                .all(|warning| warning.starts_with("monitor.\"eDP-1\": ")),
            "{:?}",
            odd.warnings
        );
        assert_eq!(odd.monitors[0].1.screen, Some(Screen::default()));

        assert_eq!(parse("monitor = 1").unwrap().warnings.len(), 1);
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

    #[test]
    fn keys_are_the_hosts() {
        let config = parse(
            r#"
            [[right]]
            widget = "claude"
            keys = { chat-1 = "Alt+1", chat-2 = false, chat-3 = "hyper+3", chat-4 = 4 }
            [[dormer]]
            widget = "volume"
            keys = "ctrl+m"
            "#,
        )
        .unwrap();
        let keys = &config.entries[0].keys;
        assert_eq!(keys["chat-1"], Some(eave_api::ui::shortcut('1').alt()));
        assert_eq!(keys["chat-2"], None);
        assert_eq!(keys.len(), 2);
        assert!(!config.entries[0].options.contains_key("keys"));
        assert!(config.entries[1].keys.is_empty());
        assert!(!config.entries[1].options.contains_key("keys"));
        assert_eq!(config.warnings.len(), 3);
    }

    #[test]
    fn parts_follow_their_owner() {
        let config = parse(
            r#"
            [[right]]
            widget = "volume"
            width_group = "w"
            parts = [
                "someone/airpods",
                { widget = "someone/spotify", client_id = "x", keys = { liked = "ctrl+l" } },
            ]
            [[right]]
            widget = "battery"
            "#,
        )
        .unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        let owners: Vec<_> = config
            .entries
            .iter()
            .map(|entry| (entry.widget.as_str(), entry.owner))
            .collect();
        assert_eq!(
            owners,
            [
                ("volume", None),
                ("someone/airpods", Some(0)),
                ("someone/spotify", Some(0)),
                ("battery", None),
            ]
        );
        assert!(!config.entries[0].options.contains_key("parts"));
        let spotify = &config.entries[2];
        assert_eq!(spotify.options["client_id"].as_str(), Some("x"));
        assert!(!spotify.options.contains_key("widget"));
        assert!(spotify.keys.contains_key("liked"));
        assert_eq!(spotify.width_group, None);
        assert_eq!(config.entries[1].options, toml::Table::new());
    }

    #[test]
    fn odd_parts_are_warned_about() {
        let config = parse(
            r#"
            [[right]]
            widget = "volume"
            parts = [3, { client_id = "x" }, { widget = "a", parts = ["b"], collapsible = true }]
            [[left]]
            widget = "clock"
            parts = "someone/spotify"
            "#,
        )
        .unwrap();
        let widgets: Vec<_> = config.entries.iter().map(|e| e.widget.as_str()).collect();
        assert_eq!(widgets, ["clock", "volume", "a"]);
        assert_eq!(config.warnings.len(), 5, "{:?}", config.warnings);
    }

    #[test]
    fn a_part_is_the_same_part_whoever_its_owner_is() {
        let config = parse(
            r#"
            [[left]]
            widget = "clock"
            [[right]]
            widget = "media"
            parts = ["someone/spotify"]
            "#,
        )
        .unwrap();
        let moved = parse(
            r#"
            [[right]]
            widget = "media"
            parts = ["someone/spotify"]
            "#,
        )
        .unwrap();
        assert_eq!(config.entries[2].owner, Some(1));
        assert_eq!(moved.entries[1].owner, Some(0));
        assert!(config.entries[2].same_but_keys(&moved.entries[1]));
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
