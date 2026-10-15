//! The eave host. A binary built with plugins calls [`run`] with each plugin's `register` function.

mod alternate;
mod app;
mod apps;
mod bezel;
mod calibrate;
mod clip;
mod code;
mod config;
mod countdown;
mod counter;
mod cover;
mod dialog;
mod doctor;
mod dormer;
mod ease;
mod field;
mod glide;
mod gulp;
mod hand;
mod handover;
mod highlight;
mod icon;
mod island;
mod jelly;
mod kit;
mod label;
mod lift;
mod markdown;
mod meter;
mod neck;
mod notch;
mod pip;
mod shortcut;
mod slider;
mod slot;
mod spring;
mod sprite;
mod squircle;
mod squish;
mod steps;
mod switcher;
mod tabs;
mod text_area;
mod theme;
mod timeline;
mod toggle;
mod wall;
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
  calibrate   Measure the screen's corners and notch, and save them to config.toml
  doctor [--report <path>]
              Check config.toml, its widgets and keys, the screen and the systemd units, and
              say what's wrong. --report also writes what it found to <path>, as TOML
  dormer      Open the running bar's dormer, or close it. A Hyprland bind can do it directly:
              bind = SUPER, SPACE, event, eave-dormer
  popover <widget>
              Open a widget's popover, or close it. As a Hyprland bind:
              bind = SUPER, C, event, eave-popover claude
  action <widget> <id>
              Run a widget's action, by the id its README gives. As a Hyprland bind:
              bind = SUPER, M, event, eave-action volume mute";

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
/// bar until it exits. `eave calibrate` runs the calibrator instead (D-71), and `eave doctor` the
/// checks (D-96).
pub fn run(plugins: &[fn(&mut Registry)]) -> Result<(), Error> {
    match std::env::args().nth(1).as_deref() {
        None | Some("doctor") => {}
        Some("calibrate") => return calibrate::run(),
        Some("dormer") => return send(eave_hyprland::DORMER.to_owned()),
        Some(command @ ("popover" | "action")) => {
            let words: Vec<String> = std::env::args().skip(2).collect();
            let (event, wanted) = match command {
                "popover" => (eave_hyprland::POPOVER, 1),
                _ => (eave_hyprland::ACTION, 2),
            };
            // A name with a space in it would read as two words on the other side.
            if words.len() != wanted
                || words
                    .iter()
                    .any(|word| word.is_empty() || word.contains(char::is_whitespace))
            {
                eprintln!("eave: {command} takes {wanted} name(s) (try --help)");
                std::process::exit(2);
            }
            return send(format!("{event} {}", words.join(" ")));
        }
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
    // Needs the registry: it checks the widgets config.toml names against it.
    if std::env::args().nth(1).as_deref() == Some("doctor") {
        doctor::run(&registry);
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
    .theme(app::theme)
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

/// `eave dormer`, `eave popover` and `eave action`: send `data` to the running bar as a Hyprland
/// custom event, the way a bind with the `event` dispatcher would.
fn send(data: String) -> Result<(), Error> {
    let event = format!("hl.dsp.event(\"{data}\")");
    let legacy = format!("event {data}");
    let sent = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map(|runtime| runtime.block_on(eave_hyprland::dispatch(&event, &legacy)));
    match sent {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) | Err(error) => {
            eprintln!("eave: can't reach Hyprland: {error}");
            std::process::exit(1);
        }
    }
}
