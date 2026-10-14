//! The notch at the top of a MacBook's screen (D-69). Asahi hides it unless the kernel is told to
//! show it (`appledrm.show_notch=1`): then the panel's full height is the output's, and its top
//! center is a hole the bar has to go around.
//!
//! The kernel knows how tall it is (the display controller's `apple,notch-height` in the device
//! tree), but not how wide: that comes from a table of the models someone has measured. Found once,
//! when the bar starts: none of it changes while it runs.

use std::path::Path;

use crate::config;
use iced::{Point, Rectangle, Size};

use crate::squircle::{self, CornerShape};
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
    /// How much thicker the bar draws the top edge than it is (D-81): everything that hangs from
    /// it, a notch included, hangs this much lower.
    pub grow: f32,
    /// The screen's own corners: top left, top right, bottom right, bottom left.
    pub corners: [CornerShape; 4],
    /// An island's corners where they face neither a screen corner nor the notch.
    pub islands: CornerShape,
    /// Whether config.toml says how the screen is shaped. Unmeasured, islands keep the theme's
    /// corners; measured, they follow the screen's, square on a square screen (D-95).
    pub measured: bool,
    /// How much further in than the gap the first island on the left, and the last on the
    /// right, sit: room for a screen corner too round for theirs to follow (D-95).
    pub shift: [f32; 2],
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            notch: None,
            inset: [0.0; 4],
            grow: 0.0,
            corners: [CornerShape::SQUARE; 4],
            islands: CornerShape::new(theme::ISLAND_RADIUS, theme::ISLAND_SMOOTHING),
            measured: false,
            shift: [0.0; 2],
        }
    }
}

/// How far short of a whole gap an island may come to a screen corner before it moves in (D-95).
const CLEAR_ENOUGH: f32 = 0.25;

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
            Some(notch) => {
                (self.notch_top(notch) + notch.height - self.edge() - f32::from(theme::EDGE_GAP))
                    .max(theme::MIN_ISLAND_HEIGHT)
            }
            None => theme::ISLAND_HEIGHT as f32,
        }
    }

    /// Where the top edge shows from, grown (D-81): below the rows the bezel hides and the ones
    /// the bar adds to it. The islands are a gap below it.
    pub fn edge(&self) -> f32 {
        self.inset[0] + self.grow
    }

    /// Where `notch` starts: one hanging from the top hangs from it grown (D-81), a cut-out is
    /// where the hardware has it.
    pub fn notch_top(&self, notch: Notch) -> f32 {
        if notch.top > 0.0 {
            notch.top
        } else {
            self.grow
        }
    }

    /// The bar's exclusive zone and input strip, from the top of the bar's surface: the grown
    /// edge, the gap and the islands, and a little room below them.
    pub fn bar_height(&self) -> u32 {
        theme::bar_height(self.grow, self.island_height())
    }

    /// The bar's surface at rest: the grown edge, the gap, the islands and all the room below.
    pub fn surface_height(&self) -> u32 {
        theme::surface_height(self.grow, self.island_height())
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
    /// same all the way round: square if the screen's is no rounder than the gap. None if the
    /// screen isn't measured.
    fn concentric(&self, index: usize) -> Option<CornerShape> {
        let corner = self.corners[index];
        let radius = (corner.radius - f32::from(theme::EDGE_GAP)).max(0.0);
        self.measured
            .then(|| CornerShape::new(radius, corner.smoothing))
    }

    /// How far in the island at the screen's corner on `side` has to move for the gap to it to
    /// stay a whole gap: none unless the corner is rounder than an island can follow, whose
    /// corners reach at most halfway down it.
    fn corner_shift(&self, side: Side) -> f32 {
        let gap = f32::from(theme::EDGE_GAP);
        let (index, place) = match side {
            Side::Left => (
                0,
                Place {
                    screen_corner: Some(Side::Left),
                    ..Place::default()
                },
            ),
            Side::Right => (
                1,
                Place {
                    screen_corner: Some(Side::Right),
                    ..Place::default()
                },
            ),
        };
        let screen = self.corners[index];
        if !self.measured || screen.radius <= gap {
            return 0.0;
        }
        // Mirrored for the right: the island's corner facing the screen's is its top left, and
        // the one below it its bottom left.
        let corners = self.island_corners(place);
        let [facing, below] = match side {
            Side::Left => [corners[0], corners[3]],
            Side::Right => [corners[1], corners[2]],
        };
        // The screen from its corner on: far enough on that its other corners don't count.
        let far = 1.0e4;
        let mut outer = squircle::Polygons::default();
        squircle::rectangle(
            &mut outer,
            Rectangle::new(Point::ORIGIN, Size::new(far, far)),
            [
                screen,
                CornerShape::SQUARE,
                CornerShape::SQUARE,
                CornerShape::SQUARE,
            ],
        );
        let height = self.island_height();
        // Wide enough that its far corners are well past the screen's.
        let width = height.max(2.0 * (1.0 + screen.smoothing) * screen.radius + height);
        let clearance = |shift: f32| {
            let mut island = squircle::Polygons::default();
            squircle::rectangle(
                &mut island,
                Rectangle::new(Point::new(gap + shift, gap), Size::new(width, height)),
                [facing, CornerShape::SQUARE, CornerShape::SQUARE, below],
            );
            squircle::clearance(&outer, &island)
        };
        // A quarter of a pixel short is as good as there: a squircle's corner one gap in already
        // strays a tenth of one from the gap (D-95), and a corner squeezed into an island's
        // height a little more.
        let enough = gap - CLEAR_ENOUGH;
        if clearance(0.0) >= enough {
            return 0.0;
        }
        let (mut near, mut far) = (0.0, screen.radius * (1.0 + screen.smoothing));
        for _ in 0..20 {
            let middle = (near + far) / 2.0;
            if clearance(middle) >= enough {
                far = middle;
            } else {
                near = middle;
            }
        }
        far
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

/// The screen as config.toml has it for `output` (the focused one, without one), in its logical
/// pixels; its notch found, given, or none; and the islands' corners. And the output's name.
pub async fn resolve(screens: config::Screens, output: Option<String>) -> (Option<String>, Screen) {
    let monitor = match &output {
        Some(name) => match by_name(name).await {
            Some(monitor) => Some(monitor),
            None => focused().await,
        },
        None => focused().await,
    };
    let config::ScreenSetting {
        notch,
        screen,
        islands,
    } = match &monitor {
        Some(monitor) => screens.screen_for(&monitor.name, &monitor.description),
        None => screens.top,
    };
    // Physical pixels to logical.
    let scale = monitor.as_ref().map_or(1.0, |monitor| monitor.scale);
    let notch = match notch {
        config::Notch::Off => None,
        config::Notch::Fixed(shape) => Some(fixed(shape, scale)),
        config::Notch::Auto => match &monitor {
            Some(monitor) => detect(monitor).await,
            None => None,
        },
    };
    let measured = screen.is_some();
    let screen = screen.unwrap_or_default();
    let mut resolved = Screen {
        notch,
        measured,
        inset: screen.inset.map(|inset| inset / scale),
        grow: screen.grow / scale,
        corners: screen
            .corners
            .map(|radius| CornerShape::new(radius / scale, screen.smoothing)),
        ..Screen::default()
    };
    resolved.resolve_islands(islands, scale);
    resolved.shift = [Side::Left, Side::Right].map(|side| resolved.corner_shift(side));
    (output.or(monitor.map(|monitor| monitor.name)), resolved)
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

/// This machine's notch, on `monitor`, if it shows there.
async fn detect(monitor: &eave_hyprland::types::Monitor) -> Option<Notch> {
    let found = tokio::task::spawn_blocking(|| find(Path::new("/")))
        .await
        .ok()??;
    // Only the built-in panel has the notch, and only upright is it at the top.
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
    monitor(|monitor| monitor.focused).await
}

/// The output with this connector name, like `eDP-1`.
async fn by_name(name: &str) -> Option<eave_hyprland::types::Monitor> {
    monitor(|monitor| monitor.name == name).await
}

async fn monitor(
    which: impl Fn(&eave_hyprland::types::Monitor) -> bool,
) -> Option<eave_hyprland::types::Monitor> {
    let monitors: Vec<eave_hyprland::types::Monitor> =
        eave_hyprland::query("monitors").await.ok()?;
    monitors
        .into_iter()
        .find(|monitor| which(monitor) && monitor.scale > 0.0)
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
            measured: true,
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
    fn a_grown_edge_takes_a_hanging_notch_with_it() {
        let mut screen = macbook(config::Islands::default());
        let bar = screen.bar_height();
        screen.grow = 3.0;
        // Everything hangs 3 lower, and the islands are as tall.
        assert_eq!(screen.edge(), 3.0);
        assert_eq!(screen.notch_top(screen.notch.unwrap()), 3.0);
        assert_eq!(screen.island_height(), 26.0);
        assert_eq!(screen.bar_height(), bar + 3);
        // A cut-out stays where it is, so the islands beside it end there.
        screen.notch.as_mut().unwrap().top = 2.0;
        assert_eq!(screen.island_height(), 25.0);
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

    /// A screen measured with these corners, and its islands as they'd be resolved.
    fn measured(radius: f32, smoothing: f32) -> Screen {
        let mut screen = Screen {
            corners: [CornerShape::new(radius, smoothing); 4],
            measured: true,
            ..Screen::default()
        };
        screen.resolve_islands(config::Islands::default(), 1.0);
        screen
    }

    #[test]
    fn a_measured_square_screen_has_square_islands() {
        let screen = measured(1.0, 0.65);
        let place = Place {
            screen_corner: Some(Side::Left),
            ..Place::default()
        };
        let square = CornerShape::new(0.0, 0.65);
        assert_eq!(screen.island_corners(place), [square; 4]);
        assert_eq!(screen.corner_shift(Side::Left), 0.0);
        // Unless config.toml rounds them.
        let mut screen = measured(1.0, 0.65);
        screen.resolve_islands(
            config::Islands {
                radius: Some(8.0),
                smoothing: None,
            },
            1.0,
        );
        assert_eq!(screen.islands, CornerShape::new(8.0, 0.65));
    }

    #[test]
    fn islands_stay_where_they_are_at_corners_they_can_follow() {
        for (radius, smoothing) in [(20.0, 0.65), (19.5, 0.65), (13.5, 0.0), (6.0, 1.0)] {
            let screen = measured(radius, smoothing);
            assert_eq!(screen.corner_shift(Side::Left), 0.0, "{radius}");
            assert_eq!(screen.corner_shift(Side::Right), 0.0, "{radius}");
        }
    }

    #[test]
    fn islands_move_in_from_corners_too_round_to_follow() {
        let screen = measured(39.0, 0.65);
        let shift = screen.corner_shift(Side::Left);
        assert!(shift > 1.0, "{shift}");
        assert_eq!(screen.corner_shift(Side::Right), shift);
        // As far as it takes, and no further: the gap is whole at the shift, a pixel less isn't.
        let gap = f32::from(theme::EDGE_GAP);
        let clearance = |shift: f32| {
            let mut outer = squircle::Polygons::default();
            squircle::rectangle(
                &mut outer,
                Rectangle::new(Point::ORIGIN, Size::new(1.0e4, 1.0e4)),
                [
                    screen.corners[0],
                    CornerShape::SQUARE,
                    CornerShape::SQUARE,
                    CornerShape::SQUARE,
                ],
            );
            let corners = screen.island_corners(Place {
                screen_corner: Some(Side::Left),
                ..Place::default()
            });
            let mut island = squircle::Polygons::default();
            squircle::rectangle(
                &mut island,
                Rectangle::new(Point::new(gap + shift, gap), Size::new(200.0, 30.0)),
                corners,
            );
            squircle::clearance(&outer, &island)
        };
        let enough = gap - CLEAR_ENOUGH;
        assert!(clearance(shift) >= enough, "{}", clearance(shift));
        assert!(
            clearance(shift - 1.0) < enough,
            "{}",
            clearance(shift - 1.0)
        );
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
        // A screen corner no rounder than the gap makes the island's square.
        let square = Screen {
            corners: [CornerShape::new(4.0, 0.0); 4],
            ..screen
        };
        assert_eq!(
            square.island_corners(left),
            [CornerShape::SQUARE, free, free, free]
        );
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
