//! The default binary: the host plus the plugins that ship with eave. Third-party plugins are added
//! by `eave-build`, which generates a main like this one.

fn main() -> Result<(), eave::Error> {
    eave::run(&[eave_clock::register])
}
