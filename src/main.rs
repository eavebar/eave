mod app;

use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};

const BAR_HEIGHT: u32 = 32;

fn main() -> Result<(), iced_layershell::Error> {
    iced_layershell::application(app::App::new, "eave", app::update, app::view)
        .subscription(app::subscription)
        .style(app::style)
        .settings(Settings {
            layer_settings: LayerShellSettings {
                anchor: Anchor::Top | Anchor::Left | Anchor::Right,
                layer: Layer::Top,
                size: Some((0, BAR_HEIGHT)),
                exclusive_zone: BAR_HEIGHT as i32,
                keyboard_interactivity: KeyboardInteractivity::None,
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
