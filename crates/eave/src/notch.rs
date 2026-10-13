//! The notch at the top of a MacBook's screen (D-69). Asahi hides it unless the kernel is told to
//! show it (`appledrm.show_notch=1`): then the panel's full height is the output's, and its top
//! center is a hole the bar has to go around.
//!
//! The kernel knows how tall it is (the display controller's `apple,notch-height` in the device
//! tree), but not how wide: that comes from a table of the models someone has measured. Found once,
//! when the bar starts: none of it changes while it runs.

use std::path::Path;

use crate::config;
use crate::squircle::CornerShape;
use crate::theme;

/// The notch, in logical pixels, centered at the top of the output.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Notch {
    pub width: f32,
    pub height: f32,
    /// Its lower corners'.
    pub radius: f32,
    /// 0 rounds its corners and shoulders like a circle, up to 1 like a squircle.
    pub smoothing: f32,
    /// How much it's rounded outwards where it meets the top edge.
    pub shoulder: f32,
    /// How far below the top edge it starts: 0 hangs from it.
    pub top: f32,
    /// How much bigger than the hardware's `width` and `height` are: on each side, and below
    /// (D-75). The room a squish has to shrink it back by.
    pub grow: [f32; 2],
}

/// The notch as the hardware has it, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    /// `None` for a model nobody has measured yet.
    width: Option<u32>,
    height: u32,
}

/// How big each model's notch is, width and height in physical pixels, by the first entry of the
/// device tree's `compatible`. Only measured ones: a guess would put islands over the screen or
/// under the notch.
///
/// The 14" and 16" panels are both 254 ppi, and macOS measures their notch at 185 points wide in
/// the native-scale mode (`NSScreen`'s frame minus its auxiliary top areas): 370 pixels
/// (notchbay.com/blog/macbook-notch-size, crestnotch.app/macbook-notch-dimensions). The height is
/// fitted by eye on a 14": 66, where the kernel's `apple,notch-height` says 74, which hangs below
/// it, and macOS's top safe area inset says 64.
const MODELS: &[(&str, u32, u32)] = &[
    // MacBook Pro 14" and 16" (M1 Pro, M1 Max).
    ("apple,j314s", 370, 66),
    ("apple,j314c", 370, 66),
    ("apple,j316s", 370, 66),
    ("apple,j316c", 370, 66),
];

/// For a notch whose width nobody has measured: wide enough to clear any MacBook's, in logical
/// pixels. `notch = { width = … }` in config.toml says better.
pub const UNKNOWN_WIDTH: f32 = 260.0;

/// What the bar goes around on its output, in logical pixels: the notch, the rows and columns
/// hidden under the bezel (D-71), and how round the screen and the islands are (D-73).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub notch: Option<Notch>,
    /// Top, right, bottom, left.
    pub inset: [f32; 4],
    /// The screen's own corners: top left, top right, bottom right, bottom left.
    pub corners: [CornerShape; 4],
    /// An island's corners where they face neither a screen corner nor the notch.
    pub islands: CornerShape,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            notch: None,
            inset: [0.0; 4],
            corners: [CornerShape::SQUARE; 4],
            islands: CornerShape::new(theme::ISLAND_RADIUS, theme::ISLAND_SMOOTHING),
        }
    }
}

/// A side of an island, or of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// What an island is next to, for its corners (D-73).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// The screen's top corner on this side is the nearest thing to the island's.
    pub screen_corner: Option<Side>,
    /// The notch is on this side of it, one gap away.
    pub notch: Option<Side>,
}

impl Screen {
    /// An island's corners where it is: top left, top right, bottom right, bottom left. One that
    /// faces a screen corner follows it at an even distance; on the notch's side, the top one is
    /// rounded like the notch's shoulder and the bottom one like its lower corner; the rest are
    /// `islands`.
    pub fn island_corners(&self, place: Place) -> [CornerShape; 4] {
        let mut corners = [self.islands; 4];
        match place.screen_corner {
            Some(Side::Left) => corners[0] = self.concentric(0).unwrap_or(corners[0]),
            Some(Side::Right) => corners[1] = self.concentric(1).unwrap_or(corners[1]),
            None => {}
        }
        let (top, bottom) = match place.notch {
            Some(Side::Left) => (0, 3),
            Some(Side::Right) => (1, 2),
            None => return corners,
        };
        if let Some(shoulder) = self.notch_shoulder() {
            corners[top] = shoulder;
        }
        if let Some(lower) = self.notch_corner() {
            corners[bottom] = lower;
        }
        corners
    }

    /// How tall the islands are: down to where the notch ends, from one gap below the top that
    /// shows (D-76). Without a notch, the theme's.
    pub fn island_height(&self) -> f32 {
        match self.notch {
            Some(notch) => (notch.top + notch.height - self.inset[0] - f32::from(theme::EDGE_GAP))
                .max(theme::MIN_ISLAND_HEIGHT),
            None => theme::ISLAND_HEIGHT as f32,
        }
    }

    /// The notch's lower corners, if it has measured ones.
    fn notch_corner(&self) -> Option<CornerShape> {
        self.notch
            .filter(|notch| notch.radius > 0.0)
            .map(|notch| CornerShape::new(notch.radius, notch.smoothing))
    }

    /// How the notch is rounded where it meets the top edge, if it's measured.
    fn notch_shoulder(&self) -> Option<CornerShape> {
        self.notch
            .filter(|notch| notch.shoulder > 0.0)
            .map(|notch| CornerShape::new(notch.shoulder, notch.smoothing))
    }

    /// A corner one edge gap in from the screen's corner `index`, so the gap between them is the
    /// same all the way round: none if the screen's is no rounder than the gap.
    fn concentric(&self, index: usize) -> Option<CornerShape> {
        let corner = self.corners[index];
        let radius = corner.radius - f32::from(theme::EDGE_GAP);
        (radius > 0.0).then(|| CornerShape::new(radius, corner.smoothing))
    }

    /// `islands` from config.toml: what it says, or the notch's corners, else the screen's top
    /// ones followed at an even distance, else the theme's.
    fn resolve_islands(&mut self, islands: config::Islands, scale: f32) {
        let auto = self
            .notch_corner()
            .or_else(|| {
                let rounder = if self.corners[1].radius > self.corners[0].radius {
                    1
                } else {
                    0
                };
                self.concentric(rounder)
            })
            .unwrap_or(self.islands);
        self.islands = CornerShape::new(
            islands.radius.map_or(auto.radius, |radius| radius / scale),
            islands.smoothing.unwrap_or(auto.smoothing),
        );
    }
}

/// The screen as config.toml has it, in the output's logical pixels; its notch found, given, or
/// none; and the islands' corners.
pub async fn resolve(
    notch: config::Notch,
    screen: config::Screen,
    islands: config::Islands,
) -> Screen {
    // Only what's measured needs the scale: physical pixels to logical.
    let measured = matches!(notch, config::Notch::Fixed(_))
        || screen != config::Screen::default()
        || islands.radius.is_some();
    let scale = if measured {
        focused().await.map_or(1.0, |monitor| monitor.scale)
    } else {
        1.0
    };
    let notch = match notch {
        config::Notch::Off => None,
        config::Notch::Fixed(shape) => Some(fixed(shape, scale)),
        config::Notch::Auto => detect().await,
    };
    let mut resolved = Screen {
        notch,
        inset: screen.inset.map(|inset| inset / scale),
        corners: screen
            .corners
            .map(|radius| CornerShape::new(radius / scale, screen.smoothing)),
        ..Screen::default()
    };
    resolved.resolve_islands(islands, scale);
    resolved
}

/// The notch config.toml gives, in logical pixels at `scale`, as big as the bar draws it: grown.
/// Without a height, it's as tall as islands reach without one.
fn fixed(shape: config::NotchShape, scale: f32) -> Notch {
    let bar_height = f32::from(theme::EDGE_GAP) + theme::ISLAND_HEIGHT as f32;
    let grow = shape.grow.map(|grow| grow / scale);
    Notch {
        width: shape.width / scale + 2.0 * grow[0],
        height: shape.height.map_or(bar_height, |height| height / scale) + grow[1],
        radius: shape.radius / scale,
        smoothing: shape.smoothing,
        shoulder: shape.shoulder / scale,
        top: shape.top / scale,
        grow,
    }
}

/// This machine's notch, on the output the bar is on, if it shows there.
async fn detect() -> Option<Notch> {
    let found = tokio::task::spawn_blocking(|| find(Path::new("/")))
        .await
        .ok()??;
    // Only the built-in panel has the notch, and only upright is it at the top.
    let monitor = focused().await?;
    if !monitor.name.starts_with("eDP") || monitor.transform != 0 {
        return None;
    }
    let width = match found.width {
        Some(width) => width as f32 / monitor.scale,
        None => {
            eprintln!(
                "eave: this MacBook's notch hasn't been measured, guessing it's {UNKNOWN_WIDTH} \
                 wide. Set `notch = {{ width = … }}` in config.toml to fit it"
            );
            UNKNOWN_WIDTH
        }
    };
    Some(Notch {
        width,
        height: found.height as f32 / monitor.scale,
        ..Notch::default()
    })
}

/// The output the bar opens on: the focused one (one surface for now).
pub async fn focused() -> Option<eave_hyprland::types::Monitor> {
    let monitors: Vec<eave_hyprland::types::Monitor> =
        eave_hyprland::query("monitors").await.ok()?;
    monitors
        .into_iter()
        .find(|monitor| monitor.focused && monitor.scale > 0.0)
}

/// This machine's notch as the hardware has it, in physical pixels, if the kernel shows it: its
/// width, if someone has measured it, and its height. Reads files, so it blocks.
pub fn hardware() -> Option<(Option<f32>, f32)> {
    let found = find(Path::new("/"))?;
    Some((found.width.map(|width| width as f32), found.height as f32))
}

/// The notch under `root` (`/`, or a fake tree in tests), if the kernel shows it.
fn find(root: &Path) -> Option<Found> {
    if !shown(root) {
        return None;
    }
    // The kernel's height says the notch is there; a measured one says how tall it looks.
    let height = height(root)?;
    let model = model(root);
    let measured = MODELS
        .iter()
        .find(|(name, ..)| Some(*name) == model.as_deref());
    Some(Found {
        width: measured.map(|(_, width, _)| *width),
        height: measured.map_or(height, |(.., height)| *height),
    })
}

/// Whether the kernel shows the notch. The parameter was `apple_dcp`'s before the driver became
/// `appledrm`.
fn shown(root: &Path) -> bool {
    ["appledrm", "apple_dcp"].iter().any(|module| {
        let path = root.join(format!("sys/module/{module}/parameters/show_notch"));
        std::fs::read_to_string(path).is_ok_and(|value| matches!(value.trim(), "Y" | "1"))
    })
}

/// The notch's height as the kernel has it, from the display controller that drives the built-in panel: the only one
/// with `apple,notch-height`, a big-endian u32.
fn height(root: &Path) -> Option<u32> {
    let soc = std::fs::read_dir(root.join("proc/device-tree/soc")).ok()?;
    soc.flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("dcp@"))
        .find_map(|entry| {
            let bytes = std::fs::read(entry.path().join("apple,notch-height")).ok()?;
            let height = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?);
            (height > 0).then_some(height)
        })
}

/// The machine's model, like `apple,j314s`: the first of the device tree's `compatible`.
fn model(root: &Path) -> Option<String> {
    let bytes = std::fs::read(root.join("proc/device-tree/compatible")).ok()?;
    let first = bytes.split(|byte| *byte == 0).next()?;
    Some(String::from_utf8_lossy(first).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fake `/` with what the kernel shows on a MacBook.
    fn tree(name: &str, model: &str, shown: Option<&str>, height: Option<u32>) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eave-notch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let soc = root.join("proc/device-tree/soc");
        std::fs::create_dir_all(soc.join("dcp@289c00000")).unwrap();
        std::fs::create_dir_all(soc.join("dcp@38bc00000")).unwrap();
        std::fs::write(
            root.join("proc/device-tree/compatible"),
            format!("{model}\0apple,t6000\0apple,arm-platform\0"),
        )
        .unwrap();
        if let Some(height) = height {
            std::fs::write(
                soc.join("dcp@38bc00000/apple,notch-height"),
                height.to_be_bytes(),
            )
            .unwrap();
        }
        if let Some(shown) = shown {
            let parameters = root.join("sys/module/appledrm/parameters");
            std::fs::create_dir_all(&parameters).unwrap();
            std::fs::write(parameters.join("show_notch"), format!("{shown}\n")).unwrap();
        }
        root
    }

    /// A MacBook's, at scale 2: corners of 19.5 and a notch rounded 10.
    fn macbook(islands: config::Islands) -> Screen {
        let mut screen = Screen {
            notch: Some(Notch {
                width: 184.0,
                height: 32.0,
                radius: 10.0,
                smoothing: 0.4,
                shoulder: 5.5,
                top: 0.0,
                grow: [0.0; 2],
            }),
            corners: [
                CornerShape::new(19.5, 0.65),
                CornerShape::new(19.5, 0.65),
                CornerShape::new(0.5, 0.65),
                CornerShape::new(0.5, 0.65),
            ],
            ..Screen::default()
        };
        screen.resolve_islands(islands, 2.0);
        screen
    }

    #[test]
    fn a_grown_notch_is_bigger_by_its_growth() {
        let shape = config::NotchShape {
            width: 368.0,
            height: Some(64.0),
            radius: 20.0,
            shoulder: 11.0,
            grow: [4.0, 2.0],
            ..config::NotchShape::default()
        };
        let notch = fixed(shape, 2.0);
        assert_eq!((notch.width, notch.height), (188.0, 33.0));
        assert_eq!((notch.radius, notch.shoulder), (10.0, 5.5));
        assert_eq!(notch.grow, [2.0, 1.0]);
    }

    #[test]
    fn islands_end_where_the_notch_does() {
        assert_eq!(
            Screen::default().island_height(),
            theme::ISLAND_HEIGHT as f32
        );
        let mut screen = macbook(config::Islands::default());
        assert_eq!(screen.island_height(), 26.0);
        // A row hidden under the bezel moves the islands' top, not the notch's bottom.
        screen.inset[0] = 2.0;
        assert_eq!(screen.island_height(), 24.0);
        // A tiny notch doesn't crush them.
        screen.notch.as_mut().unwrap().height = 10.0;
        assert_eq!(screen.island_height(), theme::MIN_ISLAND_HEIGHT);
    }

    #[test]
    fn with_nothing_measured_islands_are_the_themes() {
        let mut screen = Screen::default();
        screen.resolve_islands(config::Islands::default(), 1.0);
        let theme = CornerShape::new(theme::ISLAND_RADIUS, theme::ISLAND_SMOOTHING);
        let place = Place {
            screen_corner: Some(Side::Left),
            notch: Some(Side::Right),
        };
        assert_eq!(screen.island_corners(place), [theme; 4]);
    }

    #[test]
    fn auto_islands_are_the_notchs_then_the_screens() {
        let screen = macbook(config::Islands::default());
        assert_eq!(screen.islands, CornerShape::new(10.0, 0.4));
        // An unmeasured notch has no corners to go by.
        let mut screen = Screen {
            notch: Some(Notch::default()),
            ..macbook(config::Islands::default())
        };
        screen.resolve_islands(config::Islands::default(), 2.0);
        assert_eq!(screen.islands, CornerShape::new(13.5, 0.65));
        // Given, in physical pixels; what isn't given is still auto.
        let screen = macbook(config::Islands {
            radius: Some(24.0),
            smoothing: None,
        });
        assert_eq!(screen.islands, CornerShape::new(12.0, 0.4));
    }

    #[test]
    fn island_corners_follow_what_they_face() {
        let screen = macbook(config::Islands::default());
        let (notch, free) = (CornerShape::new(10.0, 0.4), screen.islands);
        let shoulder = CornerShape::new(5.5, 0.4);
        let concentric = CornerShape::new(13.5, 0.65);
        let left = Place {
            screen_corner: Some(Side::Left),
            ..Place::default()
        };
        assert_eq!(screen.island_corners(left), [concentric, free, free, free]);
        let right = Place {
            screen_corner: Some(Side::Right),
            ..Place::default()
        };
        assert_eq!(screen.island_corners(right)[1], concentric);
        let before_notch = Place {
            notch: Some(Side::Right),
            ..Place::default()
        };
        assert_eq!(
            screen.island_corners(before_notch),
            [free, shoulder, notch, free]
        );
        let after_notch = Place {
            notch: Some(Side::Left),
            ..Place::default()
        };
        assert_eq!(
            screen.island_corners(after_notch),
            [shoulder, free, free, notch]
        );
        // A screen corner no rounder than the gap leaves the island's as it was.
        let square = Screen {
            corners: [CornerShape::new(4.0, 0.0); 4],
            ..screen
        };
        assert_eq!(square.island_corners(left), [free; 4]);
    }

    #[test]
    fn a_shown_notch_is_found_with_the_models_width() {
        let root = tree("shown", "apple,j314s", Some("Y"), Some(74));
        assert_eq!(
            find(&root),
            Some(Found {
                width: Some(370),
                height: 66
            })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_hidden_notch_or_none_at_all_is_nothing_to_go_around() {
        for (name, shown, height) in [
            ("hidden", Some("N"), Some(74)),
            ("old-kernel", None, Some(74)),
            ("no-notch", Some("Y"), None),
        ] {
            let root = tree(name, "apple,j314s", shown, height);
            assert_eq!(find(&root), None, "{name}");
            std::fs::remove_dir_all(root).unwrap();
        }
        assert_eq!(find(Path::new("/nonexistent")), None);
    }

    #[test]
    fn an_unmeasured_model_has_no_width() {
        let root = tree("unmeasured", "apple,j413", Some("Y"), Some(64));
        assert_eq!(
            find(&root),
            Some(Found {
                width: None,
                height: 64
            })
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
