//! eave_glass_v1 on the bar's own Wayland connection. Objects live on an event queue of their own,
//! dispatched by a small thread, so nothing here waits on the UI thread or makes it wait. Requests
//! go out from the UI thread as a frame is drawn, before it's committed: they land with it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use iced::futures::channel::mpsc;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use super::protocol::eave_glass_manager_v1::EaveGlassManagerV1;
use super::protocol::eave_glass_surface_v1::{self, EaveGlassSurfaceV1, Ink};
use super::tone::Backdrop;
use crate::config;

/// What the compositor's glass says.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The compositor has glass to give: the bar should draw a frame, which asks for it.
    Ready,
    /// Whether there's glass behind the bar. Without it, islands draw as they would anyway.
    Active(bool),
    /// How bright it is behind the bar.
    Backdrop(Backdrop),
}

/// Whether the plugin draws glass behind the bar right now.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the thread has been started, so it's tried once.
static STARTED: AtomicBool = AtomicBool::new(false);
static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
static EVENTS: Mutex<Option<mpsc::Sender<Event>>> = Mutex::new(None);

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

struct Shared {
    queue: QueueHandle<State>,
    manager: Option<EaveGlassManagerV1>,
    glass: Option<Attached>,
}

/// The glass of one surface, and what it was last told, so it's only told what changed.
struct Attached {
    surface: WlSurface,
    object: EaveGlassSurfaceV1,
    regions: Option<Vec<u8>>,
    /// The reliefs last sent (D-126).
    reliefs: Option<Vec<u8>>,
    look: Option<config::Glass>,
    report: Option<u32>,
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.object.destroy();
    }
}

fn shared() -> MutexGuard<'static, Option<Shared>> {
    SHARED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn send(event: Event) {
    if let Some(sender) = EVENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        // At most ten reports a second: a full channel means the bar is stuck, and the next one
        // will do.
        let _ = sender.try_send(event);
    }
}

fn set_active(active: bool) {
    if ACTIVE.swap(active, Ordering::Relaxed) != active {
        send(Event::Active(active));
    }
}

/// The glass events, for a subscription.
pub fn events() -> impl iced::futures::Stream<Item = Event> {
    iced::stream::channel(16, async |sender| {
        *EVENTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sender);
        if active() {
            send(Event::Active(true));
        }
        iced::futures::future::pending::<()>().await;
    })
}

/// Tells the glass about the frame being drawn: called as it's drawn, after everything else in it.
/// `look` is `None` without the glass theme: then only the notch and the grown edge are told, so
/// the pointer goes under them (D-113).
pub fn present(
    look: Option<&config::Glass>,
    records: Vec<super::frame::Record>,
    reliefs: Vec<super::frame::Relief>,
) {
    let Some(surface) = layershellev::layer_surfaces()
        .into_iter()
        .next()
        .map(|(_, s)| s)
    else {
        return;
    };
    if !STARTED.swap(true, Ordering::Relaxed) {
        start(&surface);
        return;
    }
    let mut guard = shared();
    let Some(shared) = guard.as_mut() else {
        return;
    };
    let Some(manager) = &shared.manager else {
        return;
    };
    // A new surface (the bar moved to another output) gets glass of its own.
    if shared
        .glass
        .as_ref()
        .is_none_or(|glass| glass.surface != surface)
    {
        shared.glass = None;
        if !surface.is_alive() {
            return;
        }
        let object = manager.get_glass(&surface, &shared.queue, ());
        shared.glass = Some(Attached {
            surface,
            object,
            regions: None,
            reliefs: None,
            look: None,
            report: None,
        });
    }
    let Some(glass) = shared.glass.as_mut() else {
        return;
    };
    if let Some(look) = look
        && glass.look.as_ref() != Some(look)
    {
        glass.object.set_look(
            f64::from(look.band),
            f64::from(look.thickness),
            f64::from(look.chroma),
            f64::from(look.rim),
            f64::from(look.frost),
            look.depth,
            f64::from(look.tint),
            match look.ink {
                config::Ink::Own => Ink::Own,
                config::Ink::Invert => Ink::Invert,
                config::Ink::Readable => Ink::Readable,
            },
            f64::from(look.contrast),
            f64::from(look.ink_blur),
        );
        // A compositor from before the gap (version 2) keeps the glass on what's behind it.
        if glass.object.version() >= 3 {
            glass.object.set_gap(f64::from(look.gap));
        }
        glass.look = Some(*look);
    }
    // No reports without glass: nothing would read them.
    let report = look.map_or(0, |_| crate::theme::GLASS_REPORT_CELL);
    if glass.report.unwrap_or(0) != report {
        glass.object.set_report(report);
        glass.report = Some(report);
    }
    // Never more than fits in one message, which `fit` sees to: past it, the request would cut the
    // bar's connection.
    debug_assert!(records.len() <= crate::theme::GLASS_MAX_REGIONS);
    // A compositor from before bezel records (version 1) would take them for nothing.
    let mut records = records;
    if glass.object.version() < 2 {
        records.retain(|record| record.kind != super::frame::Kind::Bezel);
    }
    let regions =
        super::frame::encode(&records[..records.len().min(crate::theme::GLASS_MAX_REGIONS)]);
    if glass.regions.as_ref() != Some(&regions) {
        glass.object.set_regions(regions.clone());
        glass.regions = Some(regions);
    }
    // Controls shape the glass from version 4 (D-126); before it, they're drawn as they are.
    if glass.object.version() >= 4 {
        debug_assert!(reliefs.len() <= crate::theme::GLASS_MAX_RELIEFS);
        let reliefs = super::frame::encode_reliefs(
            &reliefs[..reliefs.len().min(crate::theme::GLASS_MAX_RELIEFS)],
        );
        if glass.reliefs.as_ref() != Some(&reliefs) {
            glass.object.set_reliefs(reliefs.clone());
            glass.reliefs = Some(reliefs);
        }
    }
}

/// Binds the glass, if the compositor has it, and dispatches its events from now on.
fn start(surface: &WlSurface) {
    let Some(backend) = surface.backend().upgrade() else {
        return;
    };
    let connection = Connection::from_backend(backend);
    let spawned = std::thread::Builder::new()
        .name("eave-glass".into())
        .stack_size(128 * 1024)
        .spawn(move || {
            if let Err(error) = run(connection) {
                eprintln!("eave: glass: {error}");
            }
            set_active(false);
        });
    if let Err(error) = spawned {
        eprintln!("eave: glass: {error}");
    }
}

fn run(connection: Connection) -> Result<(), String> {
    let (globals, mut queue) =
        registry_queue_init::<State>(&connection).map_err(|error| error.to_string())?;
    let handle = queue.handle();
    let manager = globals
        .bind::<EaveGlassManagerV1, _, _>(&handle, 1..=4, ())
        .ok();
    let mut state = State {
        name: globals.contents().with_list(|list| {
            list.iter()
                .find(|global| global.interface == EaveGlassManagerV1::interface().name)
                .map(|global| global.name)
        }),
    };
    let ready = manager.is_some();
    *shared() = Some(Shared {
        queue: handle,
        manager,
        glass: None,
    });
    if ready {
        send(Event::Ready);
    }
    loop {
        queue
            .blocking_dispatch(&mut state)
            .map_err(|error| error.to_string())?;
    }
}

struct State {
    /// The manager's global, so its going away is noticed.
    name: Option<u32>,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            // hypr-eave loaded while the bar runs.
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == EaveGlassManagerV1::interface().name && state.name.is_none() => {
                let manager =
                    registry.bind::<EaveGlassManagerV1, _, _>(name, version.min(4), handle, ());
                state.name = Some(name);
                if let Some(shared) = shared().as_mut() {
                    shared.glass = None;
                    shared.manager = Some(manager);
                }
                send(Event::Ready);
            }
            // And unloaded: what it made is inert, and goes.
            wl_registry::Event::GlobalRemove { name } if state.name == Some(name) => {
                state.name = None;
                if let Some(shared) = shared().as_mut() {
                    shared.glass = None;
                    if let Some(manager) = shared.manager.take() {
                        manager.destroy();
                    }
                }
                set_active(false);
            }
            _ => {}
        }
    }
}

impl Dispatch<EaveGlassManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &EaveGlassManagerV1,
        _: <EaveGlassManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<EaveGlassSurfaceV1, ()> for State {
    fn event(
        _: &mut Self,
        object: &EaveGlassSurfaceV1,
        event: eave_glass_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Only the current surface's glass speaks for the bar.
        let current = shared()
            .as_ref()
            .and_then(|shared| shared.glass.as_ref())
            .is_some_and(|glass| glass.object == *object);
        if !current {
            return;
        }
        match event {
            eave_glass_surface_v1::Event::Active { active } => set_active(active != 0),
            eave_glass_surface_v1::Event::Backdrop {
                cell,
                cols,
                rows,
                cells,
            } => {
                if let Some(backdrop) = Backdrop::new(cell, cols, rows, &cells) {
                    send(Event::Backdrop(backdrop));
                }
            }
        }
    }
}
