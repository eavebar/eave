//! The plugin API for eave.
//!
//! A plugin is a crate that implements [`Widget`] for one or more types and exports a
//! `register` function:
//!
//! ```ignore
//! pub fn register(registry: &mut eave_api::Registry) {
//!     registry.widget::<Clock>("clock");
//! }
//! ```
//!
//! Widgets describe their UI with the closed vocabulary in [`ui`]. They never pick colors, fonts or
//! spacing, and they never touch the bar's layout or surfaces: the host owns all of that.

mod effect;
mod registry;
pub mod ui;
mod widget;

#[doc(hidden)]
pub mod erased;

pub use effect::{Effect, Nudge, NudgeRequest};
pub use iced_futures::Subscription;
pub use iced_runtime::Task;
pub use registry::Registry;
pub use widget::Widget;

/// Timers, for the few things that have no event source (clocks, `/proc` counters), and one-shot
/// delays (`Task::future(sleep(..))`, e.g. to show something for a moment).
pub mod time {
    pub use iced_futures::backend::native::tokio::time::every;
    pub use iced_futures::core::time::{Duration, Instant};

    /// Waits `duration`. Nothing starts until the task runs it, so it's safe to build in `update`.
    pub async fn sleep(duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// The [`Widget::Config`] of a widget without options. Rejects any option it is given.
#[derive(Debug, Default, Clone, Copy, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoConfig {}

/// The version of this API. `eave-build` checks plugin manifests against it.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");
