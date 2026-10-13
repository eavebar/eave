//! The host: holds the widget instances, routes their messages, and owns the bar surfaces. A
//! popover isn't a surface of its own: the island holding its item morphs into it, and the bar
//! surface grows to make room while it's there. Widgets' notifications turn the right island at
//! the screen's edge into a panel the same way, while there are any. The dormer, the search
//! dialog for widgets' actions, drops out of the top of the screen the same way too (D-84).

use std::time::{Duration, Instant};

use eave_api::erased::{AnyMessage, ErasedWidget, PopoverRequest};
use eave_api::ui::{self as ui, Action, Icon, Notice, NoticeParts, Tone};
use eave_api::{NudgeRequest, Registry};
use iced::alignment::Horizontal;
use iced::widget::{Space, container, row, stack};
use iced::{
    Color, Element, Event, Length, Padding, Rectangle, Subscription, Task, Theme, keyboard, mouse,
    theme, window,
};
use iced_layershell::actions::ActionCallback;
use iced_layershell::reexport::KeyboardInteractivity;
use iced_layershell::to_layer_message;

use crate::apps::{self, AppIcon, FileIcon, FileKey, Frame, Icons, Wanted};
use crate::config::{self, Config, Entry, Zone};
use crate::countdown::Countdown;
use crate::cover::{self, Cover};
use crate::dormer::{self, Recent, Session, Source};
use crate::island::{self, Island, Placement, Toggle};
use crate::notch::{self, Notch, Place, Side};
use crate::slot::Slot;
use crate::squircle::CornerShape;
use crate::zones::Zones;
use crate::{icon, kit, theme as style, watch};

pub struct Bar {
    registry: Registry,
    /// The config the instances were built from, to skip reloads that change nothing.
    config: Config,
    instances: Vec<Instance>,
    /// Instances are addressed by id, not position, so a reload can't route a stale message to
    /// whichever widget now sits at the old index.
    next_id: u64,
    /// At most one open, plus any still morphing back into their islands.
    popovers: Vec<Popover>,
    /// Whether the pointer is over the bar, so a click that Hyprland reports is ours to handle,
    /// not an outside one. Tracked only while a popover is open.
    pointer_inside: bool,
    /// The bar surface holding the keyboard: the one with the open popover, so a key press can
    /// close it.
    keyboard: Option<window::Id>,
    /// How many nudges widgets have asked for, so each island can tell a new one from one it has
    /// already played.
    nudges: u64,
    /// App icons and plugins' icon files loaded so far, for every widget to share.
    icons: Icons,
    /// App ids whose icons are being loaded.
    loading: Vec<String>,
    /// Icon files being drawn.
    drawing: Vec<FileKey>,
    /// Covers being fetched, by URL.
    fetching: Vec<String>,
    /// Widgets' notifications, on screen or waiting to be, newest first.
    toasts: Vec<Toast>,
    /// The right island while it's a panel of notifications.
    notices: Option<NoticePanel>,
    /// The dormer, from the keybind until it has folded back.
    dormer: Option<Session>,
    /// The dormer's last picks, for as long as the bar runs.
    recent: Recent,
    /// Bar surfaces, in the order they opened.
    windows: Vec<window::Id>,
    /// Numbers the expiry timer, so only the one set last counts.
    timer: u64,
    /// When that timer goes off, if one is set.
    alarm: Option<Instant>,
    /// Islands showing their collapsible items. The rest start, and stay, collapsed.
    expanded: Vec<IslandId>,
    /// How many times a collapsed item has come out on its own, so only the last timer of each
    /// puts it back.
    reveals: u64,
    /// The icons an open popover shows, read from the popover its widget built while updating,
    /// so it isn't built again just to look for them.
    panel_wanted: Option<(u64, Wanted)>,
    /// What the bar goes around: the notch, if there is one (D-69), and what the bezel hides
    /// (D-71).
    screen: notch::Screen,
    /// The `notch`, `screen` and `islands` settings being, or last, looked into. `None` before
    /// the first.
    screen_setting: Option<(config::Notch, config::Screen, config::Islands)>,
}

/// One island: its zone, and which of the zone's islands it is, counted in bar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IslandId {
    zone: Zone,
    index: usize,
}

/// A divider's key among its island's items: its item's id with the top bit set, so it follows
/// that item.
const DIVIDER_KEY: u64 = 1 << 63;

struct Instance {
    id: u64,
    /// What the instance was built from. An unchanged entry keeps its instance across reloads.
    entry: Entry,
    widget: Box<dyn ErasedWidget>,
    /// Cached after every update, so drawing the bar never builds a popover just to ask.
    has_popover: bool,
    /// The bar the item was last clicked on, where its popover opens.
    window: Option<window::Id>,
    /// The last nudge it asked for, numbered by [`Bar::nudges`].
    nudge: Option<(u64, NudgeRequest)>,
    /// While a collapsed item shows because its widget nudged: which reveal, numbered by
    /// [`Bar::reveals`].
    revealed: Option<u64>,
}

impl Instance {
    fn island(&self) -> IslandId {
        IslandId {
            zone: self.entry.zone,
            index: self.entry.island,
        }
    }
}

/// Messages for widgets whose popover just opened or started closing, by instance id.
type Hooks = Vec<(u64, AnyMessage)>;

/// Sends each hook to its widget.
fn send(hooks: Hooks) -> Task<Message> {
    Task::batch(
        hooks
            .into_iter()
            .map(|(id, message)| Task::done(Message::Widget(id, message))),
    )
}

struct Popover {
    /// The id of the instance it belongs to.
    instance: u64,
    /// The bar surface whose island it grows out of.
    window: window::Id,
    /// False while it morphs back into its island, until the island says it's done.
    open: bool,
    /// Where the island put it, in surface coordinates. The surface grows to fit it.
    rect: Option<Rectangle>,
}

/// A notification: a widget's [`Notice`], as the host keeps it.
struct Toast {
    /// The id of the instance it belongs to.
    instance: u64,
    /// The notice as the widget last gave it. Kept after the widget drops it while the panel
    /// folds back into the island, to fade it out.
    notice: NoticeParts<AnyMessage>,
    /// How long it has left on screen while its clock is stopped. `None` once it has expired, or
    /// if it never does.
    left: Option<Duration>,
    /// When it expires, while its clock runs.
    deadline: Option<Instant>,
    /// The widget dropped it.
    leaving: bool,
    /// Its reply field was clicked and has the focus: the bar has the keyboard for it.
    engaged: bool,
}

/// The right island while it's a panel of notifications. Like a popover, but nobody opens it:
/// it's there while there are notifications, and the bar doesn't take the keyboard for it.
struct NoticePanel {
    /// False while it morphs back into its island, until the island says it's done.
    open: bool,
    /// Where the island put it, in surface coordinates. The surface grows to fit it.
    rect: Option<Rectangle>,
    /// Whether the pointer is on it. Nothing expires meanwhile.
    hovered: bool,
}

/// The panel's popover key: no widget instance has it.
const NOTICES: u64 = u64::MAX;
/// The dormer's popover key, and its island's one item's.
const DORMER: u64 = u64::MAX - 1;
/// Where the dormer's top goes when the screen's height can't be had, from the surface's top.
const DORMER_TOP: f32 = 160.0;

/// The time a notification gets back when the pointer leaves the panel, if it had less: enough
/// to read the last words or reach for its close button.
const TOAST_GRACE: Duration = Duration::from_secs(1);
/// How many covers are kept before the ones not on screen are let go. Each is about 260 KB.
const COVERS_KEPT: usize = 4;

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    Widget(u64, AnyMessage),
    /// A primary click on an item: the bar it's on, and the item's own `on_press`.
    Pressed(window::Id, u64, Option<AnyMessage>),
    /// A primary click on an item that something inside it handled, e.g. a workspace button.
    InnerPressed(u64),
    /// Any button went down on an item, on this bar: where its popover opens, if the press opens
    /// it. Sent before anything inside the item handles the press.
    Touched(window::Id, u64),
    /// A primary click on the bar that nothing handled: empty space, or a plain item.
    BarPressed,
    /// The toggle on an island with collapsible items was pressed: show them, or hide them.
    ToggleCollapse(IslandId),
    /// Hyprland saw a primary click somewhere ([`eave_hyprland::report_clicks`]).
    Clicked,
    PointerEntered,
    PointerLeft,
    /// A key was pressed while a popover had the keyboard.
    KeyPressed,
    /// A notification's reply field got the focus from a click, or lost it: the instance, the
    /// notice's key, and whether it has it.
    ReplyFocused(u64, u64, bool),
    /// An island placed the popover it's turning into.
    PopoverPlaced(u64, Rectangle),
    /// An island finished turning back from a popover.
    PopoverClosed(u64),
    /// The right island placed the panel of notifications it's turning into.
    NoticesPlaced(Rectangle),
    /// The right island finished turning back from the panel of notifications.
    NoticesClosed,
    /// The pointer came onto the panel of notifications, or left it.
    NoticesHovered(bool),
    /// The expiry timer with this number went off.
    ToastTimer(u64),
    /// Something happened to the dormer.
    Dormer(dormer::Event),
    /// A collapsed item's time out is up, if this is still its last reveal.
    Unreveal(u64, u64),
    /// A bar surface opened: it takes the pointer only over its strip.
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    /// `config.toml` changed on disk: the new contents, or why they couldn't be loaded.
    ConfigChanged(Result<Config, String>),
    /// The screen for the `notch`, `screen` and `islands` settings has been looked into.
    ScreenFound(
        (config::Notch, config::Screen, config::Islands),
        notch::Screen,
    ),
    /// An app's icon has loaded, or turned out not to exist.
    AppIcon(String, Option<AppIcon>),
    /// A plugin's icon file has been drawn, or turned out not to be one.
    FileIcon(FileKey, Option<FileIcon>),
    Frame(FileKey, Option<Frame>),
    /// A cover has been fetched and drawn, or couldn't be.
    Cover(String, Option<Cover>),
}

impl Bar {
    pub fn new(registry: Registry, config: Config) -> (Self, Task<Message>) {
        let mut bar = Self {
            registry,
            config: Config::default(),
            instances: Vec::new(),
            next_id: 0,
            popovers: Vec::new(),
            pointer_inside: false,
            keyboard: None,
            nudges: 0,
            icons: Icons::default(),
            loading: Vec::new(),
            drawing: Vec::new(),
            fetching: Vec::new(),
            toasts: Vec::new(),
            notices: None,
            dormer: None,
            recent: Recent::default(),
            windows: Vec::new(),
            timer: 0,
            alarm: None,
            expanded: Vec::new(),
            reveals: 0,
            panel_wanted: None,
            screen: notch::Screen::default(),
            screen_setting: None,
        };
        let task = bar.apply(config);
        (bar, task)
    }

    /// Makes the bar match `config`. Instances whose entry didn't change are kept as they are,
    /// state, subscription and open popover included; the rest are built fresh.
    fn apply(&mut self, config: Config) -> Task<Message> {
        for warning in &config.warnings {
            eprintln!("eave: config: {warning}");
        }
        // The icon theme may have changed since: look every app's icon up again. Icon files are
        // compiled in, so they stay.
        self.icons.apps.clear();
        self.loading.clear();
        let mut old = std::mem::take(&mut self.instances);
        let mut tasks = Vec::new();
        let setting = (config.notch, config.screen, config.islands);
        if self.screen_setting != Some(setting) {
            self.screen_setting = Some(setting);
            tasks.push(Task::perform(
                notch::resolve(setting.0, setting.1, setting.2),
                move |screen| Message::ScreenFound(setting, screen),
            ));
        }

        for entry in &config.entries {
            if let Some(position) = old.iter().position(|instance| instance.entry == *entry) {
                self.instances.push(old.remove(position));
                continue;
            }
            let Some(construct) = self.registry.get(config::widget_name(&entry.widget)) else {
                eprintln!(
                    "eave: widget `{}` isn't compiled in, skipping it. Run eave-build \
                     (eave-rebuild.path does it on save)",
                    entry.widget
                );
                continue;
            };
            match construct(entry.options.clone()) {
                Ok((widget, task)) => {
                    let id = self.next_id;
                    self.next_id += 1;
                    tasks.push(task.map(move |message| Message::Widget(id, message)));
                    self.instances.push(Instance {
                        id,
                        entry: entry.clone(),
                        has_popover: widget.popover().is_some(),
                        widget,
                        window: None,
                        nudge: None,
                        revealed: None,
                    });
                }
                Err(error) => eprintln!("eave: widget `{}`: {error}, skipping it", entry.widget),
            }
        }

        // Cards of an instance that's gone go with it; those of a new one come in.
        let instances = &self.instances;
        self.toasts.retain(|toast| {
            instances
                .iter()
                .any(|instance| instance.id == toast.instance)
        });
        let ids: Vec<_> = self.instances.iter().map(|instance| instance.id).collect();
        for id in ids {
            self.sync_notices(id);
        }
        if let Some(window) = self.toast_window() {
            tasks.push(self.fit(window));
        }

        // An island that no longer has collapsible items starts collapsed if it gets some again.
        let instances = &self.instances;
        self.expanded.retain(|island| {
            instances
                .iter()
                .any(|instance| instance.island() == *island && instance.entry.collapsible)
        });

        // A popover whose instance is gone has no island left to morph back into.
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.popovers)
            .into_iter()
            .partition(|popover| self.find(popover.instance).is_some());
        self.popovers = kept;
        tasks.extend(gone.iter().map(|popover| self.fit(popover.window)));
        self.refresh_dormer();
        self.config = config;
        Task::batch(tasks)
    }

    /// Starts loading the app icons that items and the open popover show and nobody has asked
    /// for yet. Only `changed`'s, if it's the one widget whose view may have changed: a widget's
    /// view comes from its own state alone.
    fn load_icons(&mut self, changed: Option<u64>) -> Vec<Task<Message>> {
        let mut wanted = Wanted::default();
        let read = self.panel_wanted.take();
        for instance in &self.instances {
            if changed.is_some_and(|id| id != instance.id) {
                continue;
            }
            if let Some(content) = instance.widget.item().into_parts().content {
                kit::wanted(content, &mut wanted);
            }
            if self.popover_of(instance.id).is_none() {
                continue;
            }
            match &read {
                Some((id, panel)) if *id == instance.id => wanted.extend(panel),
                _ => {
                    if let Some(panel) = instance.widget.popover() {
                        kit::wanted(panel.into_node(), &mut wanted);
                    }
                }
            }
        }
        if let Some(session) = self.dormer.as_ref().filter(|session| session.open) {
            for picture in dormer_pictures(self, session).into_iter().flatten() {
                kit::wanted(picture, &mut wanted);
            }
        }
        let notices = self.toasts.iter().map(|toast| &toast.notice);
        wanted.apps.extend(
            notices
                .clone()
                .filter_map(|notice| notice.app_icon.as_ref().or(notice.app.as_ref()).cloned()),
        );
        wanted
            .covers
            .extend(notices.filter_map(|notice| notice.image.clone()));
        let mut tasks = Vec::new();
        for file in wanted.files {
            let key = FileKey::of(file);
            if self.icons.files.contains_key(&key) || self.drawing.contains(&key) {
                continue;
            }
            self.drawing.push(key);
            tasks.push(Task::perform(apps::load_file(file), move |icon| {
                Message::FileIcon(key, icon)
            }));
        }
        for file in wanted.frames {
            let key = FileKey::of(file);
            if self.icons.frames.contains_key(&key) || self.drawing.contains(&key) {
                continue;
            }
            self.drawing.push(key);
            tasks.push(Task::perform(apps::load_frame(file), move |frame| {
                Message::Frame(key, frame)
            }));
        }
        // Covers come and go with tracks: past a few, keep only the ones on screen.
        if self.icons.covers.len() > COVERS_KEPT {
            self.icons
                .covers
                .retain(|url, _| wanted.covers.contains(url));
        }
        for url in wanted.covers {
            if self.icons.covers.contains_key(&url) || self.fetching.contains(&url) {
                continue;
            }
            self.fetching.push(url.clone());
            tasks.push(Task::perform(cover::load(url.clone()), move |cover| {
                Message::Cover(url.clone(), cover)
            }));
        }
        for app_id in wanted.apps {
            if self.icons.apps.contains_key(&app_id) || self.loading.contains(&app_id) {
                continue;
            }
            self.loading.push(app_id.clone());
            let id = app_id.clone();
            tasks.push(Task::perform(apps::load(app_id), move |icon| {
                Message::AppIcon(id.clone(), icon)
            }));
        }
        tasks
    }

    fn find(&self, id: u64) -> Option<&Instance> {
        self.instances.iter().find(|instance| instance.id == id)
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Instance> {
        self.instances.iter_mut().find(|instance| instance.id == id)
    }

    /// Opens `id`'s popover, closing any other. Returns the messages the widgets asked for on
    /// opening and closing ([`eave_api::ui::Panel::on_open`]).
    fn open_popover(&mut self, id: u64) -> Hooks {
        let Some(instance) = self.find(id) else {
            return Hooks::new();
        };
        // One in the dormer has no island to grow it out of.
        if !instance.has_popover || instance.entry.zone == Zone::Dormer {
            return Hooks::new();
        }
        // Opened from a notification or a signal, before its item was ever clicked: on the bar
        // the notifications show on.
        let Some(window) = instance.window.or_else(|| self.toast_window()) else {
            return Hooks::new();
        };
        let island = instance.island();

        let mut hooks = self.close_popover();
        // Popovers open from a click on the bar, so that's where the pointer is.
        self.pointer_inside = true;
        // An island turns into one popover at a time: one still closing in it gives way.
        let instances = &self.instances;
        self.popovers.retain(|popover| {
            let same_island = popover.window == window
                && instances
                    .iter()
                    .any(|other| other.id == popover.instance && other.island() == island);
            popover.instance == id || !same_island
        });
        match self
            .popovers
            .iter_mut()
            .find(|popover| popover.instance == id)
        {
            Some(popover) => popover.open = true,
            None => self.popovers.push(Popover {
                instance: id,
                window,
                open: true,
                rect: None,
            }),
        }
        let opened = self
            .find(id)
            .and_then(|instance| instance.widget.popover())
            .and_then(|panel| panel.on_open_message());
        hooks.extend(opened.map(|message| (id, message)));
        hooks
    }

    /// Starts closing whatever is open. It stays until its island has morphed back. Returns the
    /// messages the widgets asked for on closing ([`eave_api::ui::Panel::on_close`]).
    fn close_popover(&mut self) -> Hooks {
        // The dormer gives way the same way, and to the same things.
        if let Some(session) = &mut self.dormer {
            session.open = false;
        }
        let mut hooks = Hooks::new();
        for popover in self.popovers.iter_mut().filter(|popover| popover.open) {
            popover.open = false;
            let id = popover.instance;
            let message = self
                .instances
                .iter()
                .find(|instance| instance.id == id)
                .and_then(|instance| instance.widget.popover())
                .and_then(|panel| panel.on_close_message());
            hooks.extend(message.map(|message| (id, message)));
        }
        hooks
    }

    fn is_open(&self) -> bool {
        self.popovers.iter().any(|popover| popover.open) || self.dormer_open()
    }

    fn dormer_open(&self) -> bool {
        self.dormer.as_ref().is_some_and(|session| session.open)
    }

    /// Everything the dormer can list: each widget's actions, and its popover if it has one.
    /// Asked only while the dormer is open.
    fn dormer_entries(&self) -> Vec<dormer::Entry> {
        let mut entries = Vec::new();
        for instance in &self.instances {
            let actions = instance
                .widget
                .actions()
                .into_iter()
                .map(Action::into_parts);
            for (index, action) in actions.enumerate() {
                entries.push(dormer::Entry {
                    instance: instance.id,
                    source: Source::Action(index),
                    title: action.title,
                    subtitle: action.subtitle,
                    keywords: action.keywords,
                });
            }
            if instance.has_popover && instance.entry.zone != Zone::Dormer {
                entries.push(dormer::Entry {
                    instance: instance.id,
                    source: Source::Popover,
                    title: capitalized(config::widget_name(&instance.entry.widget)),
                    subtitle: Some("Open".into()),
                    keywords: Vec::new(),
                });
            }
        }
        entries
    }

    /// Asks the widgets again what the open dormer lists.
    fn refresh_dormer(&mut self) {
        if !self.dormer_open() {
            return;
        }
        let entries = self.dormer_entries();
        if let Some(session) = &mut self.dormer {
            session.refresh(entries, &self.recent);
        }
    }

    /// Picks the dormer's row at `index`: it folds back, and the row's widget gets its message,
    /// or opens its popover.
    fn run_dormer(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self
            .dormer
            .as_ref()
            .filter(|session| session.open)
            .and_then(|session| session.row(index))
            .cloned()
        else {
            return Task::none();
        };
        self.recent.push(&entry);
        let mut hooks = self.close_popover();
        let id = entry.instance;
        match entry.source {
            Source::Popover => {
                hooks.extend(self.open_popover(id));
                send(hooks)
            }
            Source::Action(index) => {
                // Asked again for its message, which the list doesn't keep. One that changed
                // meanwhile isn't the one that was picked.
                let message = self
                    .find(id)
                    .and_then(|instance| instance.widget.actions().into_iter().nth(index))
                    .map(Action::into_parts)
                    .filter(|action| action.title == entry.title)
                    .map(|action| action.message);
                match message {
                    Some(message) => {
                        Task::batch([send(hooks), handle(self, Message::Widget(id, message))])
                    }
                    None => send(hooks),
                }
            }
        }
    }

    /// Brings a collapsed item out for [`style::REVEAL_SHOWN`], or keeps it out that long again.
    /// Nothing to do for one that shows anyway.
    fn reveal(&mut self, id: u64) -> Task<Message> {
        let Some(instance) = self.instances.iter().find(|instance| instance.id == id) else {
            return Task::none();
        };
        if !instance.entry.collapsible || self.expanded.contains(&instance.island()) {
            return Task::none();
        }
        self.reveals += 1;
        let reveal = self.reveals;
        if let Some(instance) = self.find_mut(id) {
            instance.revealed = Some(reveal);
        }
        // Built inside the future: a tokio timer needs the runtime, which only runs tasks.
        let sleep = async { tokio::time::sleep(style::REVEAL_SHOWN).await };
        Task::perform(sleep, move |_| Message::Unreveal(id, reveal))
    }

    fn popover_of(&self, id: u64) -> Option<&Popover> {
        self.popovers
            .iter()
            .find(|popover| popover.instance == id && popover.open)
    }

    /// Sizes a bar surface to fit its popovers, and lets the pointer through everywhere else: the
    /// bar itself takes input across its whole strip, a popover only over its shape.
    fn fit(&self, window: window::Id) -> Task<Message> {
        let notices = self
            .notices
            .as_ref()
            .filter(|_| self.toast_window() == Some(window))
            .and_then(|panel| panel.rect);
        let dormer = self
            .dormer
            .as_ref()
            .filter(|_| self.toast_window() == Some(window))
            .and_then(|session| session.rect);
        let rects: Vec<Rectangle> = self
            .popovers
            .iter()
            .filter(|popover| popover.window == window)
            .filter_map(|popover| popover.rect)
            .chain(notices)
            .chain(dormer)
            .collect();
        let height = rects
            .iter()
            .map(|rect| (rect.y + rect.height).ceil() as u32 + u32::from(style::EDGE_GAP))
            .fold(self.screen.surface_height(), u32::max);
        let bar_height = self.screen.bar_height();
        let callback = ActionCallback::new(move |region| {
            // iced_layershell reuses one region and only clears the surface's current size, which
            // may be the old one: clear all of it, so no earlier popover's rect is left behind.
            region.subtract(0, 0, i32::MAX, i32::MAX);
            region.add(0, 0, i32::MAX, bar_height as i32);
            for rect in &rects {
                region.add(
                    rect.x.floor() as i32,
                    rect.y.floor() as i32,
                    rect.width.ceil() as i32,
                    rect.height.ceil() as i32,
                );
            }
        });
        Task::batch([
            // Width 0: stretched between the left and right anchors. The exclusive zone stays
            // the bar's, so windows never move.
            Task::done(Message::SizeChange {
                id: window,
                size: (0, height),
            }),
            Task::done(Message::SetInputRegion {
                id: window,
                callback,
            }),
        ])
    }

    /// Moves a bar surface off what the bezel hides (D-71): its margins are the insets, so the bar
    /// lays itself out on the screen that shows, and the windows are kept clear of it all, and of
    /// islands as tall as they are (D-76).
    fn inset(&self, window: window::Id) -> Task<Message> {
        let [top, right, _, left] = self.screen.inset.map(|inset| inset.round() as i32);
        Task::batch([
            Task::done(Message::MarginChange {
                id: window,
                margin: (top, right, 0, left),
            }),
            Task::done(Message::ExclusiveZoneChange {
                id: window,
                zone_size: self.screen.bar_height() as i32,
            }),
        ])
    }

    /// The bar whose right island shows the notifications: the focused monitor's. The bar only opens on the
    /// active output for now, so that's the first one; with a bar per output, it will follow
    /// Hyprland's focused monitor.
    fn toast_window(&self) -> Option<window::Id> {
        self.windows.first().copied()
    }

    /// The island that shows the notifications: the right zone's last, at the screen's edge.
    fn notice_island(&self) -> IslandId {
        let index = self
            .instances
            .iter()
            .filter(|instance| instance.entry.zone == Zone::Right)
            .map(|instance| instance.entry.island)
            .max()
            .unwrap_or(0);
        IslandId {
            zone: Zone::Right,
            index,
        }
    }

    /// Whether the notifications wait: their island is an open popover, and an island does one
    /// thing at a time.
    fn toasts_held(&self) -> bool {
        let island = self.notice_island();
        self.popovers.iter().any(|popover| {
            popover.open
                && self
                    .find(popover.instance)
                    .is_some_and(|instance| instance.island() == island)
        })
    }

    /// Takes in `id`'s notices: new ones come in on top, changed ones are updated in place, and
    /// ones it no longer lists leave.
    fn sync_notices(&mut self, id: u64) {
        let Some(instance) = self.find(id) else {
            return;
        };
        let fresh: Vec<_> = instance
            .widget
            .notices()
            .into_iter()
            .map(Notice::into_parts)
            .collect();
        for toast in self.toasts.iter_mut().filter(|toast| toast.instance == id) {
            if !fresh.iter().any(|notice| notice.key == toast.notice.key) {
                toast.leaving = true;
                toast.left = None;
                toast.deadline = None;
            }
        }
        let mut new = Vec::new();
        for notice in fresh {
            let expires = notice.expires.as_ref().map(|(after, _)| *after);
            let shown = self
                .toasts
                .iter_mut()
                .find(|toast| toast.instance == id && toast.notice.key == notice.key);
            match shown {
                Some(toast) => {
                    // Replaced with something new to read, or brought back: its time starts over.
                    let changed =
                        toast.notice.summary != notice.summary || toast.notice.body != notice.body;
                    if changed || toast.leaving {
                        toast.left = expires;
                        toast.deadline = None;
                    }
                    toast.leaving = false;
                    toast.notice = notice;
                }
                None => new.push(Toast {
                    instance: id,
                    notice,
                    left: expires,
                    deadline: None,
                    leaving: false,
                    engaged: false,
                }),
            }
        }
        new.append(&mut self.toasts);
        self.toasts = new;
    }

    /// Makes the right island a panel of notifications while there are any, and nothing else
    /// wants it. An open popover there takes the island over, and the notifications wait. One
    /// turning back gives way: the island morphs straight on to the notifications.
    fn sync_panel(&mut self) -> Task<Message> {
        let Some(window) = self.toast_window() else {
            self.notices = None;
            return Task::none();
        };
        let mut refit = false;
        if self.toasts_held() {
            refit |= self.notices.take().is_some();
            self.toasts.retain(|toast| !toast.leaving);
        } else if self.toasts.iter().any(|toast| !toast.leaving) {
            let panel = self.notices.get_or_insert(NoticePanel {
                open: true,
                rect: None,
                hovered: false,
            });
            panel.open = true;
            // Shown, the panel holds only what's still listed: the rest just goes.
            self.toasts.retain(|toast| !toast.leaving);
            let island = self.notice_island();
            let instances = &self.instances;
            let before = self.popovers.len();
            self.popovers.retain(|popover| {
                popover.window != window
                    || !instances.iter().any(|instance| {
                        instance.id == popover.instance && instance.island() == island
                    })
            });
            refit |= self.popovers.len() != before;
        } else if let Some(panel) = &mut self.notices {
            // The last ones are gone: back to the island, showing them as it goes.
            panel.open = false;
            panel.hovered = false;
        } else {
            self.toasts.clear();
        }
        if refit {
            self.fit(window)
        } else {
            Task::none()
        }
    }

    /// Starts and stops each notification's clock: it runs while the panel is on screen and not
    /// pointed at. Then sets the timer for the next one to expire, unless it's already set.
    fn run_clocks(&mut self, now: Instant) -> Task<Message> {
        // Not while an answer is being typed either.
        let shown = !self.typing()
            && self
                .notices
                .as_ref()
                .is_some_and(|panel| panel.open && panel.rect.is_some() && !panel.hovered);
        for toast in &mut self.toasts {
            let running = shown && !toast.leaving;
            match (running, toast.deadline) {
                (true, None) => toast.deadline = toast.left.map(|left| now + left),
                (false, Some(deadline)) => {
                    toast.left = Some(deadline.saturating_duration_since(now));
                    toast.deadline = None;
                }
                _ => {}
            }
        }
        let next = self.toasts.iter().filter_map(|toast| toast.deadline).min();
        if next == self.alarm {
            return Task::none();
        }
        self.alarm = next;
        self.timer += 1;
        let Some(at) = next else {
            return Task::none();
        };
        let timer = self.timer;
        // Built inside the future: a tokio timer needs the runtime, which only runs tasks.
        let sleep = async move { tokio::time::sleep_until(at.into()).await };
        Task::perform(sleep, move |_| Message::ToastTimer(timer))
    }

    /// Whether an answer is being typed into a notification on screen: its field was clicked
    /// and has the focus. The bar has the keyboard meanwhile ([D-49](../../../docs/decisions.md),
    /// [D-62](../../../docs/decisions.md)).
    fn typing(&self) -> bool {
        self.notices.as_ref().is_some_and(|panel| panel.open)
            && !self.toasts_held()
            && self
                .toasts
                .iter()
                .any(|toast| !toast.leaving && toast.engaged && toast.notice.reply.is_some())
    }

    /// The messages that cancel every answer being typed.
    fn cancel_replies(&self) -> Hooks {
        self.toasts
            .iter()
            .filter(|toast| !toast.leaving)
            .filter_map(|toast| {
                let cancel = toast.notice.reply.as_ref()?.on_cancel.clone()?;
                Some((toast.instance, cancel))
            })
            .collect()
    }

    /// The notifications whose time is up by `now`: their widgets get the message they asked for.
    fn expire(&mut self, now: Instant) -> Hooks {
        self.alarm = None;
        let mut hooks = Hooks::new();
        for toast in &mut self.toasts {
            if toast.deadline.is_some_and(|deadline| deadline <= now) {
                toast.deadline = None;
                toast.left = None;
                let message = toast
                    .notice
                    .expires
                    .as_ref()
                    .map(|(_, message)| message.clone());
                hooks.extend(message.map(|message| (toast.instance, message)));
            }
        }
        hooks
    }
}

pub fn update(bar: &mut Bar, message: Message) -> Task<Message> {
    let changed = match &message {
        Message::Widget(id, _) => Some(*id),
        _ => None,
    };
    let was_open = bar.is_open();
    let mut tasks = vec![handle(bar, message)];
    let open = bar.is_open();
    if open != was_open {
        // Hyprland reports clicks anywhere only while a popover is open.
        let report = Task::future(async move {
            if let Err(error) = eave_hyprland::report_clicks(open).await {
                eprintln!("eave: can't watch for clicks outside the popover: {error}");
            }
        });
        tasks.push(report.discard());
    }
    tasks.push(bar.sync_panel());
    tasks.extend(hand_keyboard(bar));
    tasks.extend(bar.load_icons(changed));
    tasks.push(bar.run_clocks(Instant::now()));
    Task::batch(tasks)
}

/// Gives the keyboard to the bar surface with the open popover, or with a notification being
/// answered, and takes it back from the one that had it. Exclusive, not on demand: with `follow_mouse`, hovering a window would take the
/// focus away ([D-36](../../../docs/decisions.md)).
fn hand_keyboard(bar: &mut Bar) -> Vec<Task<Message>> {
    let wanted = bar
        .popovers
        .iter()
        .find(|popover| popover.open)
        .map(|popover| popover.window)
        .or_else(|| bar.dormer_open().then(|| bar.toast_window()).flatten())
        .or_else(|| bar.typing().then(|| bar.toast_window()).flatten());
    if wanted == bar.keyboard {
        return Vec::new();
    }
    let change = |id, keyboard_interactivity| {
        Task::done(Message::KeyboardInteractivityChange {
            id,
            keyboard_interactivity,
        })
    };
    let released = bar
        .keyboard
        .map(|window| change(window, KeyboardInteractivity::None));
    let taken = wanted.map(|window| change(window, KeyboardInteractivity::Exclusive));
    bar.keyboard = wanted;
    released.into_iter().chain(taken).collect()
}

fn handle(bar: &mut Bar, message: Message) -> Task<Message> {
    match message {
        Message::Widget(id, message) => {
            // Gone after a reload: its leftover messages have nowhere to go.
            let Some(instance) = bar.find_mut(id) else {
                return Task::none();
            };
            let (task, request, nudge) = instance.widget.update(message).into_parts();
            let panel = instance.widget.popover();
            instance.has_popover = panel.is_some();
            let has_popover = instance.has_popover;
            if let Some(panel) = panel
                && bar.popover_of(id).is_some()
            {
                let mut wanted = Wanted::default();
                kit::wanted(panel.into_node(), &mut wanted);
                bar.panel_wanted = Some((id, wanted));
            }
            bar.sync_notices(id);
            bar.refresh_dormer();
            let mut reveal = Task::none();
            if let Some(nudge) = nudge {
                bar.nudges += 1;
                let generation = bar.nudges;
                if let Some(instance) = bar.find_mut(id) {
                    instance.nudge = Some((generation, nudge));
                }
                // Something happened worth showing: a collapsed item comes out for a while.
                reveal = bar.reveal(id);
            }

            let popover = match request {
                Some(PopoverRequest::Open) => bar.open_popover(id),
                Some(PopoverRequest::Close) if bar.popover_of(id).is_some() => bar.close_popover(),
                // A widget that stops offering a popover closes it.
                _ if bar.popover_of(id).is_some() && !has_popover => bar.close_popover(),
                _ => Hooks::new(),
            };
            Task::batch([
                task.map(move |message| Message::Widget(id, message)),
                send(popover),
                reveal,
            ])
        }
        Message::Pressed(window, id, on_press) => {
            let Some(instance) = bar.find_mut(id) else {
                return Task::none();
            };
            instance.window = Some(window);
            match on_press {
                // Any other item's popover closes, as on a click anywhere else.
                Some(message) => {
                    let closed = if bar.popover_of(id).is_none() {
                        bar.close_popover()
                    } else {
                        Hooks::new()
                    };
                    Task::batch([send(closed), handle(bar, Message::Widget(id, message))])
                }
                None if bar.popover_of(id).is_some() => send(bar.close_popover()),
                None => send(bar.open_popover(id)),
            }
        }
        Message::Touched(window, id) => {
            if let Some(instance) = bar.find_mut(id) {
                instance.window = Some(window);
            }
            Task::none()
        }
        Message::AppIcon(app_id, icon) => {
            bar.loading.retain(|loading| *loading != app_id);
            bar.icons.apps.insert(app_id, icon);
            Task::none()
        }
        Message::FileIcon(key, icon) => {
            bar.drawing.retain(|drawing| *drawing != key);
            bar.icons.files.insert(key, icon);
            Task::none()
        }
        Message::Frame(key, frame) => {
            bar.drawing.retain(|drawing| *drawing != key);
            bar.icons.frames.insert(key, frame);
            Task::none()
        }
        Message::Cover(url, cover) => {
            bar.fetching.retain(|fetching| *fetching != url);
            bar.icons.covers.insert(url, cover);
            Task::none()
        }
        Message::InnerPressed(id) if bar.popover_of(id).is_none() => send(bar.close_popover()),
        // Clicks on our own surfaces are handled where they land.
        Message::BarPressed => send(bar.close_popover()),
        Message::ToggleCollapse(island) => {
            for instance in &mut bar.instances {
                if instance.island() == island {
                    instance.revealed = None;
                }
            }
            match bar.expanded.iter().position(|expanded| *expanded == island) {
                Some(position) => {
                    bar.expanded.remove(position);
                }
                None => bar.expanded.push(island),
            }
            send(bar.close_popover())
        }
        Message::Clicked if !bar.pointer_inside => send(bar.close_popover()),
        Message::PointerEntered => {
            bar.pointer_inside = true;
            Task::none()
        }
        Message::PointerLeft => {
            bar.pointer_inside = false;
            Task::none()
        }
        // Esc while answering a notification cancels the answer, and lets go of the field.
        Message::KeyPressed if bar.typing() => {
            let cancel = send(bar.cancel_replies());
            for toast in &mut bar.toasts {
                toast.engaged = false;
            }
            let unfocus = iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::unfocus(),
            );
            Task::batch([cancel, unfocus])
        }
        Message::ReplyFocused(instance, key, focused) => {
            if let Some(toast) = bar
                .toasts
                .iter_mut()
                .find(|toast| toast.instance == instance && toast.notice.key == key)
            {
                toast.engaged = focused;
            }
            Task::none()
        }
        Message::KeyPressed => send(bar.close_popover()),
        Message::PopoverPlaced(id, rect) => {
            let Some(popover) = bar
                .popovers
                .iter_mut()
                .find(|popover| popover.instance == id)
            else {
                return Task::none();
            };
            popover.rect = Some(rect);
            let window = popover.window;
            bar.fit(window)
        }
        Message::PopoverClosed(id) => {
            let Some(position) = bar
                .popovers
                .iter()
                .position(|popover| popover.instance == id && !popover.open)
            else {
                return Task::none();
            };
            let popover = bar.popovers.remove(position);
            bar.fit(popover.window)
        }
        Message::NoticesPlaced(rect) => {
            let window = bar.toast_window();
            match (&mut bar.notices, window) {
                (Some(panel), Some(window)) => {
                    panel.rect = Some(rect);
                    bar.fit(window)
                }
                _ => Task::none(),
            }
        }
        Message::NoticesHovered(hovered) => {
            if let Some(panel) = &mut bar.notices {
                panel.hovered = hovered;
            }
            // Their clocks stopped while pointed at; give them a moment once the pointer is off.
            if !hovered {
                for toast in &mut bar.toasts {
                    toast.left = toast.left.map(|left| left.max(TOAST_GRACE));
                }
            }
            Task::none()
        }
        Message::NoticesClosed => {
            if bar.notices.as_ref().is_none_or(|panel| panel.open) {
                return Task::none();
            }
            bar.notices = None;
            bar.toasts.retain(|toast| !toast.leaving);
            match bar.toast_window() {
                Some(window) => bar.fit(window),
                None => Task::none(),
            }
        }
        Message::ToastTimer(timer) if timer == bar.timer => send(bar.expire(Instant::now())),
        Message::Dormer(event) => handle_dormer(bar, event),
        Message::Unreveal(id, reveal) => {
            // Its popover is open: it stays out until that closes, and a while after.
            if bar.popover_of(id).is_some() {
                return bar.reveal(id);
            }
            if let Some(instance) = bar.find_mut(id)
                && instance.revealed == Some(reveal)
            {
                instance.revealed = None;
            }
            Task::none()
        }
        Message::WindowOpened(window) => {
            if !bar.windows.contains(&window) {
                bar.windows.push(window);
            }
            Task::batch([bar.fit(window), bar.inset(window)])
        }
        Message::WindowClosed(window) => {
            if bar.toast_window() == Some(window) {
                bar.notices = None;
                bar.dormer = None;
                bar.toasts.retain(|toast| !toast.leaving);
            }
            bar.windows.retain(|open| *open != window);
            bar.popovers.retain(|popover| popover.window != window);
            if bar.keyboard == Some(window) {
                bar.keyboard = None;
            }
            Task::none()
        }
        Message::ScreenFound(setting, screen) => {
            // Looked into for a setting that has changed since: its own answer is on the way.
            if bar.screen_setting != Some(setting) || bar.screen == screen {
                return Task::none();
            }
            let moved = bar.screen.inset != screen.inset
                || bar.screen.surface_height() != screen.surface_height()
                || bar.screen.bar_height() != screen.bar_height();
            bar.screen = screen;
            if moved {
                Task::batch(
                    bar.windows
                        .iter()
                        .flat_map(|window| [bar.fit(*window), bar.inset(*window)]),
                )
            } else {
                Task::none()
            }
        }
        Message::ConfigChanged(config) => match config {
            // Editors often fire several events per save; only a real change rebuilds anything.
            Ok(config) if config == bar.config => Task::none(),
            Ok(config) => {
                if config.renderer != bar.config.renderer {
                    eprintln!("eave: config: the new `renderer` is used once the bar restarts");
                }
                bar.apply(config)
            }
            Err(error) => {
                eprintln!("eave: config: {error}; keeping the current bar");
                Task::none()
            }
        },
        // Clicks that aren't outside, and the layer-shell variants added by `to_layer_message`.
        _ => Task::none(),
    }
}

fn handle_dormer(bar: &mut Bar, event: dormer::Event) -> Task<Message> {
    use dormer::Event;
    match event {
        Event::Toggle if bar.dormer_open() => send(bar.close_popover()),
        Event::Toggle if bar.toast_window().is_some() => {
            let inset = bar.screen.inset[0];
            Task::perform(dormer_top(inset), |top| Message::Dormer(Event::Opened(top)))
        }
        Event::Toggle => Task::none(),
        Event::Opened(top) => {
            if bar.dormer_open() || bar.toast_window().is_none() {
                return Task::none();
            }
            let hooks = bar.close_popover();
            // Never over the bar.
            let top = top.max(bar.screen.surface_height() as f32);
            // A keybind opened it, not a click: the pointer is wherever it was.
            bar.pointer_inside = false;
            match &mut bar.dormer {
                // Still folding back: it turns around, and starts over empty.
                Some(session) => {
                    session.open = true;
                    session.top = top;
                    session.search(String::new(), &bar.recent);
                }
                None => bar.dormer = Some(Session::new(top)),
            }
            bar.refresh_dormer();
            send(hooks)
        }
        Event::Query(query) => {
            if let Some(session) = bar.dormer.as_mut().filter(|session| session.open) {
                session.search(query, &bar.recent);
            }
            Task::none()
        }
        Event::Move(by) => {
            if let Some(session) = bar.dormer.as_mut().filter(|session| session.open) {
                session.step(by);
            }
            Task::none()
        }
        Event::Run(index) => bar.run_dormer(index),
        Event::Placed(rect) => {
            let window = bar.toast_window();
            match (&mut bar.dormer, window) {
                (Some(session), Some(window)) => {
                    session.rect = Some(rect);
                    bar.fit(window)
                }
                _ => Task::none(),
            }
        }
        Event::Closed => {
            if bar.dormer.as_ref().is_none_or(|session| session.open) {
                return Task::none();
            }
            bar.dormer = None;
            match bar.toast_window() {
                Some(window) => bar.fit(window),
                None => Task::none(),
            }
        }
    }
}

/// Where the dormer's top goes, from the surface's: a fifth of the way down the focused screen,
/// like a dialog's. `inset` is how far down the surface starts.
async fn dormer_top(inset: f32) -> f32 {
    let Some(monitor) = notch::focused().await else {
        return DORMER_TOP;
    };
    // Turned a quarter, the screen's height is the mode's width.
    let height = if monitor.transform % 2 == 1 {
        monitor.width
    } else {
        monitor.height
    };
    (height as f32 / monitor.scale / 5.0 - inset).round()
}

/// `name` with a capital first letter: a widget's name as a title.
fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Every surface is a bar.
pub fn view(bar: &Bar, window: window::Id) -> Element<'_, Message> {
    let notices = bar
        .notices
        .as_ref()
        .filter(|_| bar.toast_window() == Some(window));
    let notice_island = bar.notice_island();
    // Islands attached to the top flare out where they meet it, like the notch does (D-72). The
    // surface's top is the screen's that shows, and the edge the bar grows from there (D-81).
    let shoulders = bar
        .screen
        .notch
        .filter(|notch| notch.shoulder > 0.0)
        .map(|notch| CornerShape::new(notch.shoulder, notch.smoothing));

    // Each island is as wide as its items. One with nothing to show draws nothing, unless it has
    // collapsed items to bring back.
    let island =
        |island: IslandId, align: Horizontal| -> Option<(Island<'_, Message>, config::Edges)> {
            let instances = || {
                bar.instances
                    .iter()
                    .filter(move |instance| instance.island() == island)
            };
            let collapsible = instances().any(|instance| instance.entry.collapsible);
            let expanded = bar.expanded.contains(&island);
            let mut items = Vec::new();
            let mut groups = Vec::new();
            for instance in instances() {
                if instance.entry.collapsible && !expanded && instance.revealed.is_none() {
                    continue;
                }
                let Some(item) = item_view(instance, window, &bar.icons) else {
                    continue;
                };
                // Only between two items that show: one next to a hidden item goes with it.
                if instance.entry.divider && !items.is_empty() {
                    items.push((instance.id | DIVIDER_KEY, kit::divider()));
                    groups.push(None);
                }
                items.push((instance.id, item));
                groups.push(instance.entry.width_group.as_deref());
            }
            let nudges = instances()
                .filter_map(|instance| Some((instance.id, instance.nudge?)))
                .collect();
            // The open one wins over one still closing in the same island.
            let popover = bar
                .popovers
                .iter()
                .filter(|popover| popover.window == window)
                .filter(|popover| instances().any(|instance| instance.id == popover.instance))
                .max_by_key(|popover| popover.open);
            // A popover there wins: the notifications wait for it.
            let popover = match popover {
                Some(popover) => Some(popover_view(bar, popover)),
                None => notices
                    .filter(|_| island == notice_island)
                    .map(|panel| notices_view(bar, panel)),
            };

            if items.is_empty() && popover.is_none() && !collapsible {
                return None;
            }
            // On the side facing the middle of the screen; a center island's on its right.
            let side = match island.zone {
                Zone::Right => Horizontal::Left,
                Zone::Left | Zone::Center | Zone::Dormer => Horizontal::Right,
            };
            // Hovering shows it; while the items are out, it stays to put them back.
            let toggle = collapsible.then(|| Toggle {
                element: toggle_view(island, side, expanded),
                side,
                expanded,
            });
            let attach = instances()
                .next()
                .map(|instance| instance.entry.attach)
                .unwrap_or_default();
            Some((
                Island::new(items)
                    .align(align)
                    .popover(popover)
                    .nudges(nudges)
                    .width_groups(groups)
                    .toggle(toggle)
                    .live(instances().any(|instance| instance.entry.live))
                    .bleed(bleed(attach))
                    .shows(shows(attach))
                    .notch(bar.screen.notch.map(|notch| notch.width))
                    .shoulders(shoulders)
                    .corners(bar.screen.island_corners(Place::default()))
                    .height(bar.screen.island_height()),
                attach,
            ))
        };
    // A zone's islands sit side by side, and the zones across the bar: the center ones stay
    // centered on screen unless a side's islands reach them, and then they're pushed aside.
    let islands = |zone: Zone, align: Horizontal| -> Vec<(Island<'_, Message>, config::Edges)> {
        let mut indices: Vec<_> = bar
            .instances
            .iter()
            .filter(|instance| instance.entry.zone == zone)
            .map(|instance| instance.entry.island)
            .collect();
        // Notifications show even when no widget on the right does.
        if notices.is_some() && zone == notice_island.zone {
            indices.push(notice_island.index);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
            .into_iter()
            .filter_map(|index| island(IslandId { zone, index }, align))
            .collect()
    };

    // The first island on the left and the last on the right face the screen's top corners.
    let corner = |side| Place {
        screen_corner: Some(side),
        ..Place::default()
    };
    let mut left = islands(Zone::Left, Horizontal::Left);
    if !left.is_empty() {
        let (first, attach) = left.remove(0);
        left.insert(0, (placed(first, &bar.screen, corner(Side::Left)), attach));
    }
    let mut right = islands(Zone::Right, Horizontal::Right);
    if let Some((last, attach)) = right.pop() {
        right.push((placed(last, &bar.screen, corner(Side::Right)), attach));
    }
    let (left, right) = (zone(left), zone(right));
    let zones = match bar.screen.notch {
        None => Zones::new(left, zone(islands(Zone::Center, Horizontal::Center)), right),
        // The center's first half of islands goes before the notch, the rest after it.
        Some(notch) => {
            let mut before = islands(Zone::Center, Horizontal::Center);
            let mut after = before.split_off(before.len().div_ceil(2));
            if let Some((last, attach)) = before.pop() {
                let last = beside_notch(last, attach, Side::Right, &bar.screen, notch);
                before.push((last, attach));
            }
            if !after.is_empty() {
                let (first, attach) = after.remove(0);
                let first = beside_notch(first, attach, Side::Left, &bar.screen, notch);
                after.insert(0, (first, attach));
            }
            let fused = [before.last(), after.first()]
                .map(|next| next.is_some_and(|(_, attach)| attach.top));
            // From where the zones start, below the grown edge and the gap.
            let below = bar.screen.edge() + f32::from(style::EDGE_GAP);
            let at = bar.screen.notch_top(notch) - below;
            Zones::around_notch(left, [zone(before), zone(after)], right, notch, at, fused)
        }
    }
    .edge(
        bar.screen.grow,
        [bar.screen.corners[0], bar.screen.corners[1]],
    );

    // No background of its own: the surface is transparent around and between the islands, and
    // below them, where a dragged island has room to move and popovers grow.
    let islands = container(zones).padding(Padding {
        top: f32::from(style::EDGE_GAP) + bar.screen.grow,
        right: style::EDGE_GAP.into(),
        bottom: style::DRAG_ROOM.into(),
        left: style::EDGE_GAP.into(),
    });
    // Always a stack, so the islands' state stays put when the dormer comes and goes.
    stack![islands, dormer_view(bar, window)].into()
}

/// The dormer's island, in the middle at the top: the notch, if there is one, or a dot under the
/// top edge, which turns into the dormer. Only there while the dormer is, on the bar the
/// notifications show on.
fn dormer_view(bar: &Bar, window: window::Id) -> Element<'_, Message> {
    let Some(session) = bar
        .dormer
        .as_ref()
        .filter(|_| bar.toast_window() == Some(window))
    else {
        return Space::new().into();
    };
    let padding = 2.0 * f32::from(style::ISLAND_PADDING);
    let width = bar
        .screen
        .notch
        .map_or(0.0, |notch| (notch.width - padding).max(0.0));
    let top = config::Edges {
        top: true,
        ..config::Edges::default()
    };
    let island = Island::new([(DORMER, Space::new().width(width).into())])
        .align(Horizontal::Center)
        .popover(Some(dormer_popover(bar, session)))
        .bleed(bleed(top))
        .shows(shows(top))
        .corners(bar.screen.island_corners(Place::default()))
        .height(bar.screen.island_height());
    container(island)
        .center_x(Length::Fill)
        .padding(Padding {
            top: f32::from(style::EDGE_GAP) + bar.screen.grow,
            ..Padding::ZERO
        })
        .into()
}

/// The pictures before the dormer's rows, in the order they show. Asked of the widgets again,
/// since the list doesn't keep them.
fn dormer_pictures(bar: &Bar, session: &Session) -> Vec<Option<ui::Node<AnyMessage>>> {
    let mut asked: Vec<(u64, Vec<Option<ui::Node<AnyMessage>>>)> = Vec::new();
    session
        .shown
        .iter()
        .map(|&index| {
            let entry = &session.entries[index];
            let Source::Action(action) = entry.source else {
                return None;
            };
            let at = match asked.iter().position(|(id, _)| *id == entry.instance) {
                Some(at) => at,
                None => {
                    let pictures = bar
                        .find(entry.instance)
                        .map(|instance| {
                            let actions = instance.widget.actions().into_iter();
                            actions.map(|action| action.into_parts().leading).collect()
                        })
                        .unwrap_or_default();
                    asked.push((entry.instance, pictures));
                    asked.len() - 1
                }
            };
            asked[at].1.get_mut(action)?.take()
        })
        .collect()
}

fn dormer_popover<'a>(bar: &'a Bar, session: &Session) -> island::Popover<'a, Message> {
    use dormer::Event;
    let field = ui::text_input("Search", session.query.clone(), |query| {
        AnyMessage::new(Event::Query(query))
    })
    .on_submit(AnyMessage::new(Event::Run(session.selected)));
    let rows: Vec<_> = session
        .shown
        .iter()
        .zip(dormer_pictures(bar, session))
        .enumerate()
        .map(|(row, (&index, picture))| {
            let entry = &session.entries[index];
            let mut item = ui::list_item(entry.title.clone());
            if let Some(subtitle) = &entry.subtitle {
                item = item.subtitle(subtitle.clone());
            }
            if let Some(picture) = picture {
                item = item.leading(picture);
            }
            item.selected(row == session.selected)
                .on_press(AnyMessage::new(Event::Run(row)))
        })
        .collect();
    let node = if !rows.is_empty() {
        ui::column([field, ui::list(rows)])
    } else if session.query.is_empty() {
        field
    } else {
        ui::column([field, ui::label("Nothing found").tone(Tone::Dim)])
    };
    // Its rows hold pictures only, so every message in it is the dormer's own.
    let content = kit::panel(node, &bar.icons)
        .map(|message: AnyMessage| Message::Dormer(message.downcast().unwrap_or(Event::Move(0))));
    island::Popover {
        key: DORMER,
        content,
        open: session.open,
        on_placed: Box::new(|rect| Message::Dormer(Event::Placed(rect))),
        on_closed: Message::Dormer(Event::Closed),
        on_hover: None,
        width: style::PANEL_WIDE_WIDTH,
        placement: Placement::Centered { top: session.top },
        on_key: Some(Box::new(|key| {
            use keyboard::key::Named;
            match key {
                keyboard::Key::Named(Named::ArrowDown | Named::Tab) => {
                    Some(Message::Dormer(Event::Move(1)))
                }
                keyboard::Key::Named(Named::ArrowUp) => Some(Message::Dormer(Event::Move(-1))),
                _ => None,
            }
        })),
    }
}

/// A zone's islands, side by side.
fn zone(islands: Vec<(Island<'_, Message>, config::Edges)>) -> Element<'_, Message> {
    row(islands.into_iter().map(|(island, _)| Element::from(island)))
        .spacing(style::ISLAND_SPACING)
        .into()
}

/// `island` rounded for what it's next to (D-73). A corner that's attached somewhere is out of
/// sight, so it doesn't matter what it would be there.
fn placed<'a>(
    island: Island<'a, Message>,
    screen: &notch::Screen,
    place: Place,
) -> Island<'a, Message> {
    island.corners(screen.island_corners(place))
}

/// An island against the notch, which is on its `side`. Attached to the top, it's fused to the
/// notch: it ends as low as the notch does, and reaches under it to its middle, past any rounding
/// of its corners, so they read as one island. Otherwise it's rounded like the notch, a gap away
/// (D-73).
fn beside_notch<'a>(
    island: Island<'a, Message>,
    attach: config::Edges,
    side: Side,
    screen: &notch::Screen,
    notch: Notch,
) -> Island<'a, Message> {
    let place = Place {
        notch: Some(side),
        ..Place::default()
    };
    let island = placed(island, screen, place);
    if !attach.top {
        return island;
    }
    let mut bleed = bleed(attach);
    // Both from the screen's top edge.
    let island_bottom = screen.edge() + f32::from(style::EDGE_GAP) + screen.island_height();
    bleed.bottom = screen.notch_top(notch) + notch.height - island_bottom;
    // What reaches under the notch is out of sight; so is what a shorter notch leaves below it.
    let shows = Padding {
        bottom: bleed.bottom,
        ..shows(attach)
    };
    match side {
        Side::Right => bleed.right = notch.width / 2.0,
        Side::Left => bleed.left = notch.width / 2.0,
    }
    island.bleed(bleed).shows(shows)
}

/// How far an island attached to `edges` can be seen past its layout: across the gap to each edge,
/// and no further.
fn shows(edges: config::Edges) -> Padding {
    let gap = |attached: bool| {
        if attached {
            f32::from(style::EDGE_GAP)
        } else {
            0.0
        }
    };
    Padding {
        top: gap(edges.top),
        right: gap(edges.right),
        bottom: 0.0,
        left: gap(edges.left),
    }
}

/// How far an island attached to `edges` reaches past its layout: across the gap to each edge,
/// and on past it (D-70).
fn bleed(edges: config::Edges) -> Padding {
    let reach = |attached: bool| {
        if attached {
            f32::from(style::EDGE_GAP) + style::ATTACH_OVERHANG
        } else {
            0.0
        }
    };
    Padding {
        top: reach(edges.top),
        right: reach(edges.right),
        bottom: 0.0,
        left: reach(edges.left),
    }
}

/// The chevron that shows and hides an island's collapsible items. It points the way they come
/// out while they're hidden, and back once they show.
fn toggle_view<'a>(island: IslandId, side: Horizontal, expanded: bool) -> Element<'a, Message> {
    let icon = if (side == Horizontal::Left) != expanded {
        Icon::Back
    } else {
        Icon::Forward
    };
    let content = container(icon::view(icon, style::tone(Tone::Dim)))
        .padding([0, style::ITEM_PADDING])
        .center_y(Length::Fill);
    Slot::new(content)
        .on_press(Message::ToggleCollapse(island))
        .into()
}

fn item_view<'a>(
    instance: &'a Instance,
    window: window::Id,
    icons: &Icons,
) -> Option<Element<'a, Message>> {
    let id = instance.id;
    let parts = instance.widget.item().into_parts();
    let on_press = parts.on_press;
    let content: Element<'_, AnyMessage> = container(kit::node(parts.content?, icons))
        .padding([0, style::ITEM_PADDING])
        .center_y(Length::Fill)
        .into();

    let wrap = move |message| Message::Widget(id, message);
    let mut slot = Slot::new(content.map(wrap))
        .on_touch(Message::Touched(window, id))
        .on_inner_press(Message::InnerPressed(id))
        .on_secondary(parts.on_secondary.map(wrap))
        .on_scroll(parts.on_scroll.map(|(up, down)| (wrap(up), wrap(down))));
    if on_press.is_some() || instance.has_popover {
        slot = slot.on_press(Message::Pressed(window, id, on_press));
    }
    Some(slot.into())
}

fn popover_view<'a>(bar: &'a Bar, popover: &Popover) -> island::Popover<'a, Message> {
    let id = popover.instance;
    // A widget can stop offering its popover while it closes: the island morphs back empty.
    let mut width = style::PANEL_WIDTH;
    let content: Element<'_, AnyMessage> =
        match bar.find(id).and_then(|instance| instance.widget.popover()) {
            Some(panel) => {
                if panel.is_wide() {
                    width = style::PANEL_WIDE_WIDTH;
                }
                kit::panel(panel.into_node(), &bar.icons)
            }
            None => Space::new().into(),
        };
    island::Popover {
        key: id,
        content: content.map(move |message| Message::Widget(id, message)),
        open: popover.open,
        on_placed: Box::new(move |rect| Message::PopoverPlaced(id, rect)),
        on_closed: Message::PopoverClosed(id),
        on_hover: None,
        width,
        placement: Placement::Below,
        on_key: None,
    }
}

fn notices_view<'a>(bar: &'a Bar, panel: &NoticePanel) -> island::Popover<'a, Message> {
    // While it folds back, it fades out the last ones it showed.
    let content = kit::notices(
        bar.toasts
            .iter()
            .filter(|toast| toast.leaving != panel.open)
            .map(|toast| {
                let instance = toast.instance;
                let countdown = toast.notice.expires.as_ref().map(|(total, _)| Countdown {
                    total: *total,
                    left: toast.left.unwrap_or_default(),
                    ends: toast.deadline,
                });
                let key = toast.notice.key;
                kit::notice(
                    toast.notice.clone(),
                    countdown,
                    &bar.icons,
                    move |message| Message::Widget(instance, message),
                    move |focused| Message::ReplyFocused(instance, key, focused),
                )
            }),
    );
    island::Popover {
        key: NOTICES,
        content,
        open: panel.open,
        on_placed: Box::new(Message::NoticesPlaced),
        on_closed: Message::NoticesClosed,
        on_hover: Some(Box::new(Message::NoticesHovered)),
        width: style::PANEL_WIDTH,
        placement: Placement::Below,
        on_key: None,
    }
}

pub fn subscription(bar: &Bar) -> Subscription<Message> {
    let widgets = bar.instances.iter().map(|instance| {
        instance
            .widget
            .subscription()
            .with(instance.id)
            .map(|(id, message)| Message::Widget(id, message))
    });
    let mut subscriptions: Vec<_> = widgets.collect();
    subscriptions.push(window::open_events().map(Message::WindowOpened));
    subscriptions.push(window::close_events().map(Message::WindowClosed));
    subscriptions.push(watch::config_changes().map(Message::ConfigChanged));
    // Pointer, click and key tracking only while a popover is open: otherwise every hover over the
    // bar would wake us.
    if bar.is_open() || bar.typing() {
        subscriptions.push(iced::event::listen_with(popover_event));
    }
    // Always: the dormer's keybind comes this way. Every other event stops in the filter.
    subscriptions.push(Subscription::run(hyprland_events).filter_map(custom));
    Subscription::batch(subscriptions)
}

fn popover_event(event: Event, status: iced::event::Status, _: window::Id) -> Option<Message> {
    match event {
        // The bar takes the keyboard only while a popover is open, so it can't learn of clicks
        // through focus loss: they're caught here instead. Handled clicks on items arrive as
        // `Pressed` or `InnerPressed`, and the popover's island takes the ones on it.
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            if status == iced::event::Status::Ignored =>
        {
            Some(Message::BarPressed)
        }
        Event::Mouse(mouse::Event::CursorEntered) => Some(Message::PointerEntered),
        Event::Mouse(mouse::Event::CursorLeft) => Some(Message::PointerLeft),
        // Any key closes it, Esc or not, unless the popover kept it for a text field (D-41).
        Event::Keyboard(keyboard::Event::KeyPressed { .. })
            if status == iced::event::Status::Ignored =>
        {
            Some(Message::KeyPressed)
        }
        _ => None,
    }
}

fn hyprland_events() -> impl eave_hyprland::Stream<Item = eave_hyprland::Update<()>> {
    eave_hyprland::watch(|| async { Ok(()) })
}

/// Our own events: the clicks Hyprland reports while a popover is open, and the dormer's keybind.
fn custom(update: eave_hyprland::Update<()>) -> Option<Message> {
    let eave_hyprland::Update::Event(eave_hyprland::Event::Custom(data)) = update else {
        return None;
    };
    match data.as_str() {
        eave_hyprland::CLICK => Some(Message::Clicked),
        eave_hyprland::DORMER => Some(Message::Dormer(dormer::Event::Toggle)),
        _ => None,
    }
}

/// Surfaces are transparent: the islands draw their own backgrounds, so they can have rounded
/// corners and the wallpaper shows around them.
pub fn style(_: &Bar, _: &Theme) -> theme::Style {
    theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: style::tone(eave_api::ui::Tone::Normal),
    }
}

#[cfg(test)]
mod tests {
    use eave_api::ui::label;
    use eave_api::{Effect, Widget};

    use super::*;

    struct Dummy;

    impl Widget for Dummy {
        type Config = toml::Table;
        type Message = ();

        fn new(_: Self::Config) -> (Self, eave_api::Task<()>) {
            (Self, eave_api::Task::none())
        }

        fn update(&mut self, _: ()) -> Effect<()> {
            Effect::none()
        }

        fn item(&self) -> eave_api::ui::Item<()> {
            eave_api::ui::Item::new(label("dummy"))
        }
    }

    fn config(source: &str) -> Config {
        crate::config::parse(source).unwrap()
    }

    fn ids(bar: &Bar) -> Vec<(&str, u64)> {
        bar.instances
            .iter()
            .map(|instance| (instance.entry.widget.as_str(), instance.id))
            .collect()
    }

    #[test]
    fn reload_keeps_unchanged_instances_and_replaces_the_rest() {
        let mut registry = Registry::new();
        registry.widget::<Dummy>("a");
        registry.widget::<Dummy>("b");
        registry.widget::<Dummy>("c");
        let (mut bar, _) = Bar::new(
            registry,
            config(
                "[[left]]\nwidget = \"a\"\nx = 1\n[[left]]\nwidget = \"b\"\n[[left]]\nwidget = \"b\"",
            ),
        );
        assert_eq!(ids(&bar), [("a", 0), ("b", 1), ("b", 2)]);

        // `a` changes an option, one `b` goes, `c` is new: only the changed ones are rebuilt,
        // and the order follows the file.
        let _ = bar.apply(config(
            "[[left]]\nwidget = \"c\"\n[[left]]\nwidget = \"b\"\n[[left]]\nwidget = \"a\"\nx = 2",
        ));
        assert_eq!(ids(&bar), [("c", 3), ("b", 1), ("a", 4)]);

        // A leftover message for a removed instance goes nowhere instead of reaching another widget.
        let _ = update(&mut bar, Message::Widget(0, AnyMessage::new(())));
        let _ = update(&mut bar, Message::Widget(2, AnyMessage::new(())));
    }

    #[test]
    fn an_unparsable_config_keeps_the_bar() {
        let mut registry = Registry::new();
        registry.widget::<Dummy>("a");
        let (mut bar, _) = Bar::new(registry, config("[[left]]\nwidget = \"a\""));
        let _ = update(&mut bar, Message::ConfigChanged(Err("broken".into())));
        assert_eq!(ids(&bar), [("a", 0)]);
    }

    struct WithPopover;

    impl Widget for WithPopover {
        type Config = toml::Table;
        type Message = ();

        fn new(_: Self::Config) -> (Self, eave_api::Task<()>) {
            (Self, eave_api::Task::none())
        }

        fn update(&mut self, _: ()) -> Effect<()> {
            Effect::none()
        }

        fn item(&self) -> eave_api::ui::Item<()> {
            eave_api::ui::Item::new(label("item"))
        }

        fn popover(&self) -> Option<eave_api::ui::Panel<()>> {
            Some(eave_api::ui::Panel::new(label("panel")))
        }
    }

    /// A bar with `WithPopover`s 0 (left) and 2 (right) and a `Dummy` 1 (left), and 0's popover
    /// open and placed.
    fn open_bar() -> Bar {
        let mut registry = Registry::new();
        registry.widget::<WithPopover>("p");
        registry.widget::<Dummy>("d");
        let (mut bar, _) = Bar::new(
            registry,
            config("[[left]]\nwidget = \"p\"\n[[left]]\nwidget = \"d\"\n[[right]]\nwidget = \"p\""),
        );
        let _ = update(&mut bar, Message::Pressed(window::Id::unique(), 0, None));
        let _ = update(&mut bar, Message::PopoverPlaced(0, placed()));
        assert!(bar.popover_of(0).is_some());
        bar
    }

    fn placed() -> Rectangle {
        Rectangle::new(iced::Point::new(6.0, 6.0), iced::Size::new(200.0, 100.0))
    }

    #[test]
    fn a_click_outside_our_surfaces_closes_the_popover() {
        let mut bar = open_bar();
        // Still on the bar, where the click that opened it happened: ours, not outside.
        let _ = update(&mut bar, Message::Clicked);
        assert!(bar.is_open());
        let _ = update(&mut bar, Message::PointerLeft);
        let _ = update(&mut bar, Message::Clicked);
        assert!(!bar.is_open());
    }

    #[test]
    fn clicks_elsewhere_on_the_bar_close_the_popover() {
        let mut bar = open_bar();
        // Its own item handling a click inside it: stays.
        let _ = update(&mut bar, Message::InnerPressed(0));
        assert!(bar.is_open());
        // Another item: closes.
        let _ = update(&mut bar, Message::InnerPressed(1));
        assert!(!bar.is_open());

        let mut bar = open_bar();
        let _ = update(&mut bar, Message::BarPressed);
        assert!(!bar.is_open());
    }

    #[test]
    fn a_closing_popover_stays_until_its_island_is_back() {
        let mut bar = open_bar();
        let _ = update(&mut bar, Message::BarPressed);
        assert_eq!(bar.popovers.len(), 1, "still morphing back");
        // Reopened before the island got back: the same popover turns around.
        let window = bar.popovers[0].window;
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        assert!(bar.popover_of(0).is_some());
        // The island reporting it's back while it's open again changes nothing.
        let _ = update(&mut bar, Message::PopoverClosed(0));
        assert_eq!(bar.popovers.len(), 1);

        let _ = update(&mut bar, Message::BarPressed);
        let _ = update(&mut bar, Message::PopoverClosed(0));
        assert!(bar.popovers.is_empty());
    }

    #[test]
    fn another_islands_popover_opens_while_the_first_closes() {
        let mut bar = open_bar();
        let window = bar.popovers[0].window;
        let _ = update(&mut bar, Message::Pressed(window, 2, None));
        assert!(bar.popover_of(2).is_some());
        assert!(bar.popover_of(0).is_none());
        assert_eq!(bar.popovers.len(), 2, "0 is still morphing back");
    }

    #[test]
    fn a_key_press_closes_the_popover() {
        let mut bar = open_bar();
        let _ = update(&mut bar, Message::KeyPressed);
        assert!(!bar.is_open());
    }

    #[test]
    fn the_open_popover_has_the_keyboard() {
        let mut bar = open_bar();
        assert_eq!(bar.keyboard, Some(bar.popovers[0].window));
        // Given back as soon as it starts closing, not when its island is back.
        let _ = update(&mut bar, Message::BarPressed);
        assert_eq!(bar.popovers.len(), 1, "still morphing back");
        assert_eq!(bar.keyboard, None);
    }

    #[test]
    fn a_popover_opens_before_its_item_was_ever_clicked() {
        // Asked for by a notification's action or a signal: it opens on the first bar.
        let mut bar = notifying_bar();
        let window = bar.windows[0];
        assert_eq!(bar.find(1).and_then(|instance| instance.window), None);
        let _ = bar.open_popover(1);
        assert_eq!(bar.popovers.len(), 1);
        assert_eq!(bar.popovers[0].window, window);
    }

    #[test]
    fn a_reload_that_drops_the_instance_drops_its_popover() {
        let mut bar = open_bar();
        let _ = bar.apply(config("[[left]]\nwidget = \"d\""));
        assert!(bar.popovers.is_empty());
    }

    struct Hooked;

    impl Widget for Hooked {
        type Config = toml::Table;
        type Message = &'static str;

        fn new(_: Self::Config) -> (Self, eave_api::Task<&'static str>) {
            (Self, eave_api::Task::none())
        }

        fn update(&mut self, _: &'static str) -> Effect<&'static str> {
            Effect::none()
        }

        fn item(&self) -> eave_api::ui::Item<&'static str> {
            eave_api::ui::Item::new(label("item"))
        }

        fn popover(&self) -> Option<eave_api::ui::Panel<&'static str>> {
            Some(
                eave_api::ui::Panel::new(label("panel"))
                    .on_open("opened")
                    .on_close("closed"),
            )
        }
    }

    fn said(hooks: Hooks) -> Vec<(u64, &'static str)> {
        hooks
            .into_iter()
            .map(|(id, message)| (id, message.downcast().unwrap()))
            .collect()
    }

    #[test]
    fn popovers_tell_their_widget_when_they_open_and_close() {
        let mut registry = Registry::new();
        registry.widget::<Hooked>("h");
        let (mut bar, _) = Bar::new(
            registry,
            config("[[left]]\nwidget = \"h\"\n[[right]]\nwidget = \"h\""),
        );
        let window = window::Id::unique();
        for id in [0, 1] {
            bar.find_mut(id).unwrap().window = Some(window);
        }
        assert_eq!(said(bar.open_popover(0)), [(0, "opened")]);
        // Opening another closes the first.
        assert_eq!(said(bar.open_popover(1)), [(0, "closed"), (1, "opened")]);
        assert_eq!(said(bar.close_popover()), [(1, "closed")]);
        assert_eq!(said(bar.close_popover()), [], "already closing");
        // Turning around while it closes opens it again.
        assert_eq!(said(bar.open_popover(1)), [(1, "opened")]);
    }

    /// Shows a notice per key it was told to add, each expiring after 5 s, and a reply field on
    /// the one being answered.
    struct Notifying(Vec<u64>, Option<u64>);

    #[derive(Debug, Clone, PartialEq)]
    enum Note {
        Add(u64),
        Drop(u64),
        Expired(u64),
        Answer(u64),
        Typed(String),
        Cancel,
    }

    impl Widget for Notifying {
        type Config = toml::Table;
        type Message = Note;

        fn new(_: Self::Config) -> (Self, eave_api::Task<Note>) {
            (Self(Vec::new(), None), eave_api::Task::none())
        }

        fn update(&mut self, message: Note) -> Effect<Note> {
            match message {
                Note::Add(key) => self.0.insert(0, key),
                Note::Drop(key) | Note::Expired(key) => self.0.retain(|shown| *shown != key),
                Note::Answer(key) => self.1 = Some(key),
                Note::Typed(_) => {}
                Note::Cancel => self.1 = None,
            }
            Effect::none()
        }

        fn item(&self) -> eave_api::ui::Item<Note> {
            eave_api::ui::Item::new(label("bell"))
        }

        fn notices(&self) -> Vec<Notice<Note>> {
            self.0
                .iter()
                .map(|&key| {
                    let notice = Notice::new(key, format!("notice {key}"))
                        .expires(Duration::from_secs(5), Note::Expired(key));
                    match self.1 {
                        Some(answering) if answering == key => notice.reply(
                            eave_api::ui::Reply::new("Reply", "", Note::Typed)
                                .on_cancel(Note::Cancel),
                        ),
                        _ => notice,
                    }
                })
                .collect()
        }
    }

    /// A bar with a `Notifying` 0 and a `WithPopover` 1 on the right, on one surface.
    fn notifying_bar() -> Bar {
        let mut registry = Registry::new();
        registry.widget::<Notifying>("n");
        registry.widget::<WithPopover>("p");
        let (mut bar, _) = Bar::new(
            registry,
            config("[[right]]\nwidget = \"n\"\n[[right]]\nwidget = \"p\""),
        );
        let _ = update(&mut bar, Message::WindowOpened(window::Id::unique()));
        bar
    }

    fn tell(bar: &mut Bar, note: Note) {
        let _ = update(bar, Message::Widget(0, AnyMessage::new(note)));
    }

    fn keys(bar: &Bar) -> Vec<(u64, bool)> {
        bar.toasts
            .iter()
            .map(|toast| (toast.notice.key, toast.leaving))
            .collect()
    }

    fn panel(bar: &Bar) -> Option<bool> {
        bar.notices.as_ref().map(|panel| panel.open)
    }

    #[test]
    fn notices_make_the_right_island_a_panel_while_there_are_any() {
        let mut bar = notifying_bar();
        assert_eq!(panel(&bar), None);
        tell(&mut bar, Note::Add(1));
        tell(&mut bar, Note::Add(2));
        assert_eq!(panel(&bar), Some(true));
        assert_eq!(keys(&bar), [(2, false), (1, false)]);

        // One goes while others stay: the panel just holds one less.
        tell(&mut bar, Note::Drop(1));
        assert_eq!(keys(&bar), [(2, false)]);

        // The last one goes: the panel folds back, still showing it, until the island is back.
        tell(&mut bar, Note::Drop(2));
        assert_eq!(panel(&bar), Some(false));
        assert_eq!(keys(&bar), [(2, true)]);
        let _ = update(&mut bar, Message::NoticesClosed);
        assert_eq!(panel(&bar), None);
        assert!(bar.toasts.is_empty());
    }

    #[test]
    fn a_new_notice_while_the_panel_folds_back_opens_it_again() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Add(1));
        tell(&mut bar, Note::Drop(1));
        assert_eq!(panel(&bar), Some(false));
        tell(&mut bar, Note::Add(2));
        assert_eq!(panel(&bar), Some(true));
        assert_eq!(keys(&bar), [(2, false)], "the one leaving is gone");
        // The island reporting it's back while it's open again changes nothing.
        let _ = update(&mut bar, Message::NoticesClosed);
        assert_eq!(panel(&bar), Some(true));
    }

    #[test]
    fn a_notice_expires_only_while_the_panel_shows_and_isnt_pointed_at() {
        let mut bar = notifying_bar();
        let start = Instant::now();
        tell(&mut bar, Note::Add(1));
        // Not placed yet: the clock hasn't started.
        let _ = bar.run_clocks(start);
        assert_eq!(bar.toasts[0].deadline, None);

        // `handle`, not `update`, so the clocks run only at the times given here.
        let _ = handle(&mut bar, Message::NoticesPlaced(placed()));
        let _ = bar.run_clocks(start);
        assert_eq!(bar.toasts[0].deadline, Some(start + Duration::from_secs(5)));

        // Pointed at after 4.5 s: stops with half a second left, and gets a second back.
        let _ = handle(&mut bar, Message::NoticesHovered(true));
        let _ = bar.run_clocks(start + Duration::from_millis(4500));
        assert_eq!(bar.toasts[0].deadline, None);
        assert_eq!(bar.toasts[0].left, Some(Duration::from_millis(500)));
        let _ = handle(&mut bar, Message::NoticesHovered(false));
        let later = start + Duration::from_secs(10);
        let _ = bar.run_clocks(later);
        assert_eq!(bar.toasts[0].deadline, Some(later + TOAST_GRACE));

        let expired: Vec<(u64, Note)> = bar
            .expire(later + TOAST_GRACE)
            .into_iter()
            .map(|(id, message)| (id, message.downcast().unwrap()))
            .collect();
        assert_eq!(expired, [(0, Note::Expired(1))]);
        assert!(bar.expire(later + TOAST_GRACE).is_empty(), "only once");
    }

    #[test]
    fn notices_wait_for_a_popover_on_their_island() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Add(1));
        let _ = update(&mut bar, Message::NoticesPlaced(placed()));
        let window = bar.windows[0];
        let _ = update(&mut bar, Message::Pressed(window, 1, None));
        assert!(bar.toasts_held());
        assert_eq!(panel(&bar), None, "the popover took the island over");
        assert_eq!(bar.toasts[0].deadline, None, "its clock stopped");

        // A notice that arrives meanwhile waits too.
        tell(&mut bar, Note::Add(2));
        assert_eq!(panel(&bar), None);
        assert_eq!(keys(&bar), [(2, false), (1, false)]);

        // Closing the popover: the island morphs straight on to the notices, and the popover
        // stops waiting to be told the island is back.
        let _ = update(&mut bar, Message::BarPressed);
        assert_eq!(panel(&bar), Some(true));
        assert!(bar.popovers.is_empty());
    }

    #[test]
    fn notices_go_to_the_right_island_at_the_edge_and_others_popovers_dont_hold_them() {
        let mut registry = Registry::new();
        registry.widget::<Notifying>("n");
        registry.widget::<WithPopover>("p");
        let (mut bar, _) = Bar::new(
            registry,
            config(
                "[[right]]\nwidget = \"p\"\n[[right]]\nisland = true\n[[right]]\nwidget = \"n\"",
            ),
        );
        let _ = update(&mut bar, Message::WindowOpened(window::Id::unique()));
        assert_eq!(
            bar.notice_island(),
            IslandId {
                zone: Zone::Right,
                index: 1
            }
        );
        let _ = update(&mut bar, Message::Widget(1, AnyMessage::new(Note::Add(1))));
        let window = bar.windows[0];
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        assert!(bar.popover_of(0).is_some());
        assert!(!bar.toasts_held(), "the popover is on the other island");
        assert_eq!(panel(&bar), Some(true));
    }

    #[test]
    fn a_popover_on_one_island_leaves_another_islands_closing_one() {
        let mut registry = Registry::new();
        registry.widget::<WithPopover>("p");
        let (mut bar, _) = Bar::new(
            registry,
            config(
                "[[right]]\nwidget = \"p\"\n[[right]]\nisland = true\n[[right]]\nwidget = \"p\"",
            ),
        );
        let window = window::Id::unique();
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        let _ = update(&mut bar, Message::Pressed(window, 1, None));
        assert!(bar.popover_of(1).is_some());
        assert_eq!(
            bar.popovers.len(),
            2,
            "0 is still morphing back into its own island"
        );
    }

    #[test]
    fn islands_collapse_until_toggled_and_stay_toggled_across_reloads() {
        let mut registry = Registry::new();
        registry.widget::<Dummy>("a");
        registry.widget::<Dummy>("b");
        let source = "[[left]]\nwidget = \"a\"\ncollapsible = true\n[[left]]\nwidget = \"b\"";
        let (mut bar, _) = Bar::new(registry, config(source));
        let island = IslandId {
            zone: Zone::Left,
            index: 0,
        };
        assert!(bar.expanded.is_empty(), "islands start collapsed");
        let _ = update(&mut bar, Message::ToggleCollapse(island));
        assert_eq!(bar.expanded, [island]);

        // Another option on `b` keeps the island and its collapsible item.
        let _ = bar.apply(config(&format!("{source}\nx = 1")));
        assert_eq!(bar.expanded, [island]);

        // Without collapsible items, it forgets.
        let _ = bar.apply(config("[[left]]\nwidget = \"a\""));
        assert!(bar.expanded.is_empty());
        let _ = bar.apply(config(source));
        assert!(bar.expanded.is_empty());
        let _ = update(&mut bar, Message::ToggleCollapse(island));
        let _ = update(&mut bar, Message::ToggleCollapse(island));
        assert!(bar.expanded.is_empty());
    }

    struct Nudger;

    impl Widget for Nudger {
        type Config = toml::Table;
        type Message = ();

        fn new(_: Self::Config) -> (Self, eave_api::Task<()>) {
            (Self, eave_api::Task::none())
        }

        fn update(&mut self, _: ()) -> Effect<()> {
            Effect::none().nudge(eave_api::Nudge::Up)
        }

        fn item(&self) -> eave_api::ui::Item<()> {
            eave_api::ui::Item::new(label("nudger"))
        }
    }

    #[test]
    fn a_collapsed_item_that_nudges_comes_out_until_its_last_reveal_is_up() {
        let mut registry = Registry::new();
        registry.widget::<Nudger>("a");
        let source = "[[left]]\nwidget = \"a\"\ncollapsible = true";
        let (mut bar, _) = Bar::new(registry, config(source));
        let nudge = |bar: &mut Bar| {
            let _ = update(bar, Message::Widget(0, AnyMessage::new(())));
        };
        assert_eq!(bar.instances[0].revealed, None);

        nudge(&mut bar);
        let first = bar.instances[0].revealed.expect("it comes out");
        nudge(&mut bar);
        let second = bar.instances[0].revealed.unwrap();
        assert_ne!(first, second);
        let _ = update(&mut bar, Message::Unreveal(0, first));
        assert_eq!(
            bar.instances[0].revealed,
            Some(second),
            "a later nudge keeps it out"
        );
        let _ = update(&mut bar, Message::Unreveal(0, second));
        assert_eq!(bar.instances[0].revealed, None);

        // An expanded island shows it anyway.
        let island = bar.instances[0].island();
        let _ = update(&mut bar, Message::ToggleCollapse(island));
        nudge(&mut bar);
        assert_eq!(bar.instances[0].revealed, None);
    }

    #[test]
    fn a_reload_that_drops_the_instance_drops_its_notices() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Add(1));
        let _ = update(
            &mut bar,
            Message::ConfigChanged(Ok(config("[[right]]\nwidget = \"p\""))),
        );
        assert!(bar.toasts.is_empty());
        assert_eq!(panel(&bar), Some(false), "folds back empty");
    }

    #[test]
    fn answering_a_notice_takes_the_keyboard_and_stops_the_clock() {
        let mut bar = notifying_bar();
        let window = bar.windows[0];
        tell(&mut bar, Note::Add(1));
        let _ = handle(&mut bar, Message::NoticesPlaced(placed()));
        assert_eq!(bar.keyboard, None, "a notice alone never takes it");

        tell(&mut bar, Note::Answer(1));
        assert!(!bar.typing(), "a field alone doesn't take it either");
        assert_eq!(bar.keyboard, None);

        // A click on the field.
        let _ = update(&mut bar, Message::ReplyFocused(0, 1, true));
        assert!(bar.typing());
        assert_eq!(bar.keyboard, Some(window));
        let _ = bar.run_clocks(Instant::now());
        assert_eq!(bar.toasts[0].deadline, None, "no expiring while typing");

        // Esc cancels the answer, and the keyboard goes back.
        let cancels: Vec<(u64, Note)> = bar
            .cancel_replies()
            .into_iter()
            .map(|(id, message)| (id, message.downcast().unwrap()))
            .collect();
        assert_eq!(cancels, [(0, Note::Cancel)]);
        tell(&mut bar, Note::Cancel);
        assert!(!bar.typing());
        assert_eq!(bar.keyboard, None);
    }

    #[test]
    fn esc_lets_go_of_the_field_and_the_keyboard() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Add(1));
        let _ = handle(&mut bar, Message::NoticesPlaced(placed()));
        tell(&mut bar, Note::Answer(1));
        let _ = update(&mut bar, Message::ReplyFocused(0, 1, true));
        assert!(bar.keyboard.is_some());
        let _ = update(&mut bar, Message::KeyPressed);
        assert!(!bar.typing());
        assert_eq!(
            bar.keyboard, None,
            "the field stays, the keyboard goes back"
        );
        assert!(bar.toasts[0].notice.reply.is_some());
    }

    thread_local! {
        /// How many times an `Acting` on this test's thread was asked for its actions.
        static ASKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Act {
        Mute,
        Show,
        Tick,
    }

    #[derive(Default)]
    struct Acting {
        muted: bool,
    }

    impl Widget for Acting {
        type Config = toml::Table;
        type Message = Act;

        fn new(_: Self::Config) -> (Self, eave_api::Task<Act>) {
            (Self::default(), eave_api::Task::none())
        }

        fn update(&mut self, message: Act) -> Effect<Act> {
            match message {
                Act::Mute => self.muted = !self.muted,
                Act::Show => return Effect::none().open_popover(),
                Act::Tick => {}
            }
            Effect::none()
        }

        fn item(&self) -> eave_api::ui::Item<Act> {
            eave_api::ui::Item::new(label("acting"))
        }

        fn popover(&self) -> Option<eave_api::ui::Panel<Act>> {
            Some(eave_api::ui::Panel::new(label("panel")))
        }

        fn actions(&self) -> Vec<Action<Act>> {
            ASKED.set(ASKED.get() + 1);
            let title = if self.muted { "Unmute" } else { "Mute" };
            vec![
                ui::action(title, Act::Mute).keywords(["sound"]),
                ui::action("Show it", Act::Show),
            ]
        }
    }

    /// A bar with an `Acting` (0) on the left and a `Dummy` (1) on the right, on a surface.
    fn acting_bar() -> (Bar, window::Id) {
        let mut registry = Registry::new();
        registry.widget::<Acting>("acting");
        registry.widget::<Dummy>("d");
        let (mut bar, _) = Bar::new(
            registry,
            config("[[left]]\nwidget = \"acting\"\n[[right]]\nwidget = \"d\""),
        );
        let window = window::Id::unique();
        let _ = update(&mut bar, Message::WindowOpened(window));
        (bar, window)
    }

    fn dormer(bar: &mut Bar, event: dormer::Event) {
        let _ = update(bar, Message::Dormer(event));
    }

    fn rows(bar: &Bar) -> Vec<String> {
        let session = bar.dormer.as_ref().unwrap();
        (0..session.shown.len())
            .map(|index| session.row(index).unwrap().title.clone())
            .collect()
    }

    #[test]
    fn the_dormer_opens_with_the_keyboard_and_closes_back() {
        let (mut bar, window) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0));
        assert!(bar.dormer_open() && bar.is_open());
        assert_eq!(bar.keyboard, Some(window));
        dormer(&mut bar, dormer::Event::Placed(placed()));
        assert_eq!(bar.dormer.as_ref().unwrap().rect, Some(placed()));

        // Any key it leaves alone closes it, like a popover; it stays until its island is back.
        let _ = update(&mut bar, Message::KeyPressed);
        assert!(!bar.is_open());
        assert_eq!(bar.keyboard, None);
        assert!(bar.dormer.is_some());
        dormer(&mut bar, dormer::Event::Closed);
        assert!(bar.dormer.is_none());
    }

    #[test]
    fn the_dormer_never_covers_the_bar() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(-50.0));
        let top = bar.dormer.as_ref().unwrap().top;
        assert_eq!(top, bar.screen.surface_height() as f32);
    }

    #[test]
    fn widgets_are_asked_for_actions_only_while_the_dormer_is_open() {
        let (mut bar, _) = acting_bar();
        ASKED.set(0);
        tell_acting(&mut bar, Act::Tick);
        let _ = update(&mut bar, Message::Pressed(window::Id::unique(), 0, None));
        let _ = update(&mut bar, Message::BarPressed);
        assert_eq!(ASKED.get(), 0);
        dormer(&mut bar, dormer::Event::Opened(200.0));
        assert!(ASKED.get() > 0);
        assert!(rows(&bar).contains(&"Mute".to_string()));
    }

    fn tell_acting(bar: &mut Bar, message: Act) {
        let _ = update(bar, Message::Widget(0, AnyMessage::new(message)));
    }

    #[test]
    fn typing_narrows_the_list_and_arrows_pick_a_row() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0));
        // Its actions, and its popover by the widget's name.
        assert_eq!(rows(&bar), ["Mute", "Show it", "Acting"]);
        dormer(&mut bar, dormer::Event::Query("sound".into()));
        assert_eq!(rows(&bar), ["Mute"]);
        dormer(&mut bar, dormer::Event::Query("x".into()));
        assert!(rows(&bar).is_empty());
        dormer(&mut bar, dormer::Event::Query(String::new()));
        dormer(&mut bar, dormer::Event::Move(-1));
        assert_eq!(bar.dormer.as_ref().unwrap().selected, 2);
    }

    #[test]
    fn picking_an_action_sends_it_to_its_widget_and_closes_the_dormer() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0));
        dormer(&mut bar, dormer::Event::Query("mute".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(!bar.is_open());

        // The widget got it: its action follows its state now, and comes first as a recent pick.
        dormer(&mut bar, dormer::Event::Opened(200.0));
        assert_eq!(rows(&bar)[0], "Unmute");
        // The list follows the widget while it's open, too.
        tell_acting(&mut bar, Act::Mute);
        assert!(rows(&bar).contains(&"Mute".to_string()));
    }

    #[test]
    fn an_action_or_a_row_can_open_a_popover() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0));
        dormer(&mut bar, dormer::Event::Query("show".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(bar.popover_of(0).is_some());
        assert!(!bar.dormer_open());

        let _ = update(&mut bar, Message::BarPressed);
        dormer(&mut bar, dormer::Event::Opened(200.0));
        dormer(&mut bar, dormer::Event::Query("acting".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(bar.popover_of(0).is_some());
    }

    #[test]
    fn the_dormer_and_popovers_take_turns() {
        let (mut bar, window) = acting_bar();
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        dormer(&mut bar, dormer::Event::Opened(200.0));
        assert!(bar.popover_of(0).is_none() && bar.dormer_open());
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        assert!(bar.popover_of(0).is_some() && !bar.dormer_open());
    }

    #[test]
    fn a_reload_takes_a_gone_widgets_rows_with_it() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0));
        let _ = bar.apply(config("[[right]]\nwidget = \"d\""));
        assert!(rows(&bar).is_empty());
        // A row picked from before goes nowhere.
        dormer(&mut bar, dormer::Event::Run(0));
    }

    #[test]
    fn a_widget_in_the_dormer_lists_its_actions_but_has_no_popover() {
        let mut registry = Registry::new();
        registry.widget::<Acting>("acting");
        let (mut bar, _) = Bar::new(registry, config("[[dormer]]\nwidget = \"acting\""));
        let _ = update(&mut bar, Message::WindowOpened(window::Id::unique()));
        dormer(&mut bar, dormer::Event::Opened(200.0));
        assert_eq!(rows(&bar), ["Mute", "Show it"]);
        dormer(&mut bar, dormer::Event::Run(1));
        assert!(!bar.is_open(), "no island to grow a popover out of");
    }
}
