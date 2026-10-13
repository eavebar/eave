//! The notch at the top of a MacBook's screen (D-69). Asahi hides it unless the kernel is told to
//! show it (`appledrm.show_notch=1`): then the panel's full height is the output's, and its top
//! center is a hole the bar has to go around.
//!
//! The kernel knows how tall it is (the display controller's `apple,notch-height` in the device
//! tree), but not how wide: that comes from a table of the models someone has measured. Found once,
//! when the bar starts: none of it changes while it runs.

use std::path::Path;

use crate::config;

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
const UNKNOWN_WIDTH: f32 = 260.0;

/// The notch the bar goes around, as config.toml asks: found, given, or none.
pub async fn resolve(setting: config::Notch) -> Option<Notch> {
    let bar_height = f32::from(crate::theme::EDGE_GAP) + crate::theme::ISLAND_HEIGHT as f32;
    match setting {
        config::Notch::Off => None,
        config::Notch::Fixed(shape) => {
            let scale = focused().await.map_or(1.0, |monitor| monitor.scale);
            Some(Notch {
                width: shape.width / scale,
                height: shape.height.map_or(bar_height, |height| height / scale),
                radius: shape.radius / scale,
                smoothing: shape.smoothing,
                shoulder: shape.shoulder / scale,
                top: shape.top / scale,
            })
        }
        config::Notch::Auto => detect().await,
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
