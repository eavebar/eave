//! `eave calibrate`: measures the screen's corners and notch, pixel by pixel (D-71).
//!
//! It covers the screen with a model of what the panel shows: a one-pixel red line just inside its
//! edge, blue over what's hidden. You change the model with the keys until every red pixel shows
//! and no blue one does, and it's saved to config.toml, where the bar reads it.

mod panel;
mod rows;
mod save;
pub mod shape;

use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::widget::{canvas, image, stack};
use iced::{Color, ContentFit, Element, Length, Subscription, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::to_layer_message;

use crate::config::{self, NotchShape};
use crate::{notch, theme};

use panel::Panel;
use rows::{Editing, Line, Row};
use shape::Shape;

/// The output being calibrated: its name, its size in physical pixels as it's shown (rotated, if
/// it is), and its scale.
#[derive(Debug, Clone)]
struct Output {
    name: String,
    width: u32,
    height: u32,
    scale: f32,
}

struct Calibrator {
    output: Output,
    editing: Editing,
    selected: Row,
    hidden: bool,
    pattern: image::Handle,
    /// Counts the patterns asked for, so one that's late for a newer model is dropped.
    drawing: u64,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    /// A key: as pressed, and with Shift applied, and the modifiers.
    Key(Key, Key, Modifiers),
    /// The pattern for the `drawing`th model.
    Drawn(u64, image::Handle),
}

/// Runs the calibrator on the focused output until it's saved or left.
pub fn run() -> Result<(), iced_layershell::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let Some(monitor) = runtime.block_on(notch::focused()) else {
        eprintln!("eave calibrate: no focused output. Is Hyprland running?");
        std::process::exit(1);
    };
    drop(runtime);
    // Odd transforms turn the output on its side.
    let (width, height) = if monitor.transform % 2 == 1 {
        (monitor.height, monitor.width)
    } else {
        (monitor.width, monitor.height)
    };
    let output = Output {
        name: monitor.name,
        width,
        height,
        scale: monitor.scale,
    };

    let config = config::load().unwrap_or_else(|error| {
        eprintln!("eave calibrate: config: {error}");
        config::Config::default()
    });
    crate::prepare(&config);
    let (shape, notch) = start(&config, &output);
    let editing = Editing::new(shape, notch);
    eprintln!(
        "eave calibrate: {} at {}×{} pixels, scale {}. Change the model until every red pixel \
         shows and no blue one does.",
        output.name, output.width, output.height, output.scale
    );

    let target = output.name.clone();
    iced_layershell::application(
        move || {
            let pattern = image::Handle::from_rgba(
                output.width,
                output.height,
                shape::pattern(&editing.shape, output.width, output.height),
            );
            Calibrator {
                output: output.clone(),
                editing,
                selected: Row::Corners,
                hidden: false,
                pattern,
                drawing: 0,
            }
        },
        || "eave-calibrate".to_string(),
        update,
        view,
    )
    .subscription(subscription)
    .style(|_: &Calibrator, _: &Theme| iced::theme::Style {
        background_color: Color::BLACK,
        text_color: Color::WHITE,
    })
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Overlay,
            exclusive_zone: -1,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            start_mode: StartMode::TargetScreen(target),
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}

/// The model to start from, and the notch to bring in if there's none yet: what config.toml says,
/// or what the hardware does.
fn start(config: &config::Config, output: &Output) -> (Shape, NotchShape) {
    let scale = output.scale;
    let bar_height = f32::from(theme::EDGE_GAP) + theme::ISLAND_HEIGHT as f32;
    let found = notch::hardware().map(|(width, height)| NotchShape {
        width: width.unwrap_or((notch::UNKNOWN_WIDTH * scale).round()),
        height: Some(height),
        ..NotchShape::default()
    });
    let notch = match config.notch {
        config::Notch::Fixed(notch) => Some(NotchShape {
            height: Some(notch.height.unwrap_or((bar_height * scale).round())),
            ..notch
        }),
        config::Notch::Off => None,
        config::Notch::Auto => found,
    };
    let shape = Shape {
        screen: config.screen,
        notch,
    };
    let fallback = notch.or(found).unwrap_or(NotchShape {
        width: (200.0 * scale).round(),
        height: Some((bar_height * scale).round()),
        ..NotchShape::default()
    });
    (shape, fallback)
}

fn update(calibrator: &mut Calibrator, message: Message) -> Task<Message> {
    match message {
        Message::Key(key, modified, modifiers) => key_pressed(calibrator, key, modified, modifiers),
        Message::Drawn(drawing, pattern) => {
            if drawing == calibrator.drawing {
                calibrator.pattern = pattern;
            }
            Task::none()
        }
        // The layer-shell requests `to_layer_message` adds; nothing here sends them.
        _ => Task::none(),
    }
}

fn key_pressed(
    calibrator: &mut Calibrator,
    key: Key,
    modified: Key,
    modifiers: Modifiers,
) -> Task<Message> {
    let before = calibrator.editing.shape;
    let rows = rows::rows(before.notch.is_some());
    let at = rows
        .iter()
        .position(|row| *row == calibrator.selected)
        .unwrap_or(0);
    let pick = |by: isize| rows[(at as isize + by).rem_euclid(rows.len() as isize) as usize];
    let big = modifiers.shift();
    let row = calibrator.selected;

    match (key.as_ref(), modified.as_ref()) {
        (Key::Named(Named::ArrowUp), _) | (Key::Character("k"), _) => {
            calibrator.selected = pick(-1);
        }
        (Key::Named(Named::Tab), _) if big => calibrator.selected = pick(-1),
        (Key::Named(Named::ArrowDown | Named::Tab), _) | (Key::Character("j"), _) => {
            calibrator.selected = pick(1);
        }
        (Key::Named(Named::ArrowLeft), _) | (Key::Character("h"), _) => {
            calibrator.editing.step(row, -1.0, big);
        }
        (Key::Named(Named::ArrowRight), _) | (Key::Character("l"), _) => {
            calibrator.editing.step(row, 1.0, big);
        }
        (Key::Named(Named::Space), _) => calibrator.editing.toggle(row),
        (_, Key::Character("?")) | (Key::Named(Named::F1), _) => {
            calibrator.hidden = !calibrator.hidden;
        }
        (Key::Named(Named::Enter), _) | (Key::Character("s"), _) => {
            let Some(path) = config::dir().map(|dir| dir.join(config::FILE)) else {
                eprintln!("eave calibrate: neither XDG_CONFIG_HOME nor HOME is set, not saved");
                return Task::none();
            };
            match save::save(&path, &calibrator.editing.shape) {
                Ok(()) => {
                    println!("eave calibrate: saved to {}", path.display());
                    return iced::exit();
                }
                Err(error) => eprintln!("eave calibrate: not saved: {error}"),
            }
        }
        (Key::Named(Named::Escape), _) | (Key::Character("q"), _) => return iced::exit(),
        _ => {}
    }

    if calibrator.editing.shape == before {
        return Task::none();
    }
    // The pattern is redrawn off the UI thread: a few milliseconds for a whole screen.
    calibrator.drawing += 1;
    let drawing = calibrator.drawing;
    let shape = calibrator.editing.shape;
    let Output { width, height, .. } = calibrator.output;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || shape::pattern(&shape, width, height))
                .await
                .unwrap_or_default()
        },
        move |rgba| Message::Drawn(drawing, image::Handle::from_rgba(width, height, rgba)),
    )
}

fn subscription(_: &Calibrator) -> Subscription<Message> {
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            ..
        } => Some(Message::Key(key, modified_key, modifiers)),
        _ => None,
    })
}

fn view(calibrator: &Calibrator) -> Element<'_, Message> {
    let shape = &calibrator.editing.shape;
    let lines = rows::lines(shape.notch.is_some());
    let selected = lines
        .iter()
        .filter_map(|line| match line {
            Line::Row(row) => Some(*row),
            Line::Heading(_) => None,
        })
        .position(|row| row == calibrator.selected)
        .unwrap_or(0);
    let panel = Panel {
        lines: lines
            .into_iter()
            .map(|line| {
                let value = match line {
                    Line::Row(row) => row.value(shape),
                    Line::Heading(_) => String::new(),
                };
                (line, value)
            })
            .collect(),
        selected,
        hidden: calibrator.hidden,
    };
    // One pattern pixel to each of the screen's: drawn at its full size, without filtering.
    let pattern = image(calibrator.pattern.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(ContentFit::Fill)
        .filter_method(image::FilterMethod::Nearest);
    stack![
        pattern,
        canvas(panel).width(Length::Fill).height(Length::Fill)
    ]
    .into()
}
