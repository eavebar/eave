//! The eave host. A binary built with plugins calls [`run`] with each plugin's `register` function.

mod app;
mod config;
mod kit;
mod measure;
mod slot;
mod switcher;
mod theme;

use eave_api::Registry;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};

pub use eave_api;
pub use iced_layershell::Error;

/// Builds the registry from `plugins`, reads `config.toml`, and runs the bar until it exits.
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

    let config = config::load();
    for warning in &config.warnings {
        eprintln!("eave: config: {warning}");
    }
    let entries = config.entries;

    iced_layershell::daemon(
        move || app::Bar::new(&registry, &entries),
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
            size: Some((0, theme::BAR_HEIGHT)),
            exclusive_zone: theme::BAR_HEIGHT as i32,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}
