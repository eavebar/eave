//! The default binary: the host plus the plugins that ship with eave. Third-party plugins are added
//! by `eave-build`, which generates a main like this one.

fn main() -> Result<(), eave::Error> {
    eave::run(&[
        eave_appearance::register,
        eave_apps::register,
        eave_battery::register,
        eave_brightness::register,
        eave_clock::register,
        eave_keyboard::register,
        eave_media::register,
        eave_network::register,
        eave_volume::register,
        eave_window::register,
        eave_workspaces::register,
    ])
}
