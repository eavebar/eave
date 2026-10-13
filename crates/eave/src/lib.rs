//! The eave host. A binary built with plugins calls [`run`] with each plugin's `register` function.

mod alternate;
mod app;
mod apps;
mod clip;
mod config;
mod countdown;
mod counter;
mod cover;
mod ease;
mod field;
mod gulp;
mod handover;
mod icon;
mod island;
mod jelly;
mod kit;
mod label;
mod markdown;
mod meter;
mod slider;
mod slot;
mod sprite;
mod squircle;
mod squish;
mod switcher;
mod tabs;
mod text_area;
mod theme;
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

/// Builds the registry from `plugins`, reads `config.toml` (and keeps watching it), and runs the
/// bar until it exits.
pub fn run(plugins: &[fn(&mut Registry)]) -> Result<(), Error> {
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

    use_installed_sans();

    let config = config::load().unwrap_or_else(|error| {
        eprintln!("eave: config: {error}");
        config::Config::default()
    });

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
            size: Some((0, theme::SURFACE_HEIGHT)),
            exclusive_zone: theme::BAR_HEIGHT as i32,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}
