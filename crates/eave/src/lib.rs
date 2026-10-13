//! The eave host. A binary built with plugins calls [`run`] with each plugin's `register` function.

mod alternate;
mod app;
mod apps;
mod calibrate;
mod clip;
mod code;
mod config;
mod countdown;
mod counter;
mod cover;
mod dormer;
mod ease;
mod field;
mod glide;
mod gulp;
mod handover;
mod highlight;
mod icon;
mod island;
mod jelly;
mod kit;
mod label;
mod markdown;
mod meter;
mod notch;
mod pip;
mod slider;
mod slot;
mod spring;
mod sprite;
mod squircle;
mod squish;
mod switcher;
mod tabs;
mod text_area;
mod theme;
mod timeline;
mod toggle;
mod watch;
mod zones;

use eave_api::Registry;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};

pub use eave_api;
pub use iced_layershell::Error;

/// Points the text engine's sans-serif at the first family in [`theme::SANS_FAMILIES`] that's
/// installed, so every weight of the bar's text comes from one sans face.
fn use_installed_sans() {
    let mut fonts = iced::advanced::graphics::text::font_system()
        .write()
        .expect("the font system");
    let db = fonts.raw().db_mut();
    let installed = |family: &str| {
        db.faces()
            .any(|face| face.families.iter().any(|(name, _)| name == family))
    };
    match theme::SANS_FAMILIES.iter().find(|family| installed(family)) {
        Some(family) => db.set_sans_serif_family(*family),
        None => eprintln!(
            "eave: none of the usual sans-serif fonts is installed; bold text may not match"
        ),
    }
}

/// A renderer for laying widgets out in tests: tiny-skia, with or without the `gpu` feature, so no
/// test needs a GPU.
#[cfg(test)]
fn test_renderer() -> iced::Renderer {
    use iced::advanced::renderer::Headless;
    let renderer = <iced::Renderer as Headless>::new(
        iced::Font::DEFAULT,
        iced::Pixels(14.0),
        Some("tiny-skia"),
    );
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(renderer)
        .expect("tiny-skia")
}

const USAGE: &str = "\
Usage: eave [command]

With no command, runs the bar.

Commands:
  calibrate   Measure the screen's corners and notch, and save them to config.toml";

/// Sets up what the bar and the calibrator both draw with: the renderer config.toml asks for, and
/// the sans-serif that's installed.
fn prepare(config: &config::Config) {
    // iced reads the renderer to use from ICED_BACKEND when it creates its compositor. One set by
    // hand wins over config.toml. Only a build with both renderers looks at it (D-67).
    if std::env::var_os("ICED_BACKEND").is_none() {
        // SAFETY: nothing else runs yet: no thread has started, so none can read the environment
        // while it changes.
        unsafe { std::env::set_var("ICED_BACKEND", config.renderer.backends()) };
    }
    use_installed_sans();
}

/// Builds the registry from `plugins`, reads `config.toml` (and keeps watching it), and runs the
/// bar until it exits. `eave calibrate` runs the calibrator instead (D-71).
pub fn run(plugins: &[fn(&mut Registry)]) -> Result<(), Error> {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("calibrate") => return calibrate::run(),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return Ok(());
        }
        Some(other) => {
            eprintln!("eave: unknown argument `{other}` (try --help)");
            std::process::exit(2);
        }
    }

    let mut registry = Registry::new();
    for register in plugins {
        register(&mut registry);
    }
    let duplicates = registry.duplicates();
    if !duplicates.is_empty() {
        eprintln!(
            "eave: more than one plugin registers the widget name(s): {}",
            duplicates.join(", ")
        );
        std::process::exit(1);
    }

    let config = config::load().unwrap_or_else(|error| {
        eprintln!("eave: config: {error}");
        config::Config::default()
    });

    prepare(&config);

    iced_layershell::daemon(
        move || app::Bar::new(registry.clone(), config.clone()),
        || "eave".to_string(),
        app::update,
        app::view,
    )
    .subscription(app::subscription)
    .style(app::style)
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: Anchor::Top | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            // Until the screen is found: then they follow its islands' height (D-76).
            size: Some((0, theme::surface_height(0.0, theme::ISLAND_HEIGHT as f32))),
            exclusive_zone: theme::bar_height(0.0, theme::ISLAND_HEIGHT as f32) as i32,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}
