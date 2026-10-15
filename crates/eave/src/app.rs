//! The host: holds the widget instances, routes their messages, and owns the bar surfaces. A
//! popover isn't a surface of its own: the island holding its item morphs into it, and the bar
//! surface grows to make room while it's there. Widgets' notifications turn the right island at
//! the screen's edge into a panel the same way, while there are any. The dormer, the search
//! dialog for widgets' actions, drops out of the top of the screen the same way too (D-84). So do
//! widgets' dialogs, which come out of whatever was acted on to open them (D-90).

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use eave_api::erased::{AnyMessage, DialogRequest, ErasedWidget, PopoverRequest};
use eave_api::ui::{self as ui, Action, Icon, Notice, NoticeParts, Pixels, Tone, Urgency};
use eave_api::{NudgeRequest, Registry};
use eave_dbus::portal::{self, ColorScheme};
use iced::alignment::Horizontal;
use iced::widget::{Space, container, row, stack};
use iced::{
    Color, Element, Event, Length, Padding, Point, Rectangle, Size, Subscription, Task, Theme,
    keyboard, mouse, theme, window,
};
use iced_layershell::actions::ActionCallback;
use iced_layershell::reexport::{KeyboardInteractivity, Layer};
use iced_layershell::to_layer_message;

use crate::apps::{self, AppIcon, FileIcon, FileKey, Frame, Icons, Wanted};
use crate::bezel::{self, Bezel};
use crate::config::{self, BarCorners, Config, Entry, Held, Showing, Style, Zone};
use crate::countdown::Countdown;
use crate::cover::{self, Cover};
use crate::dialog::{self, Dialog, Origin, Touch, Touched};
use crate::dormer::{self, Recent, Session, Source};
use crate::island::{self, Island, Placement, Toggle};
use crate::lift::{self, Lift};
use crate::notch::{self, Notch, Place, Side};
use crate::shortcut;
use crate::slot::Slot;
use crate::squircle::CornerShape;
use crate::theme::{Palette, Scheme};
use crate::wall::Walls;
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
    /// A widget's dialog, from when it opens until it's back where it came from (D-90).
    dialog: Option<Dialog>,
    /// Each island's shape as it was last drawn, for a dialog to come out of and go back into.
    shapes: RefCell<HashMap<IslandId, island::Shape>>,
    /// The dialog asked for, while where its top goes is looked up: whose, and where it comes
    /// out of.
    asked: Option<(u64, Origin)>,
    /// What was last acted on, which a dialog asked for soon after comes out of.
    touched: Option<Touched>,
    /// What's been said about the dormer's keys, so it's said once.
    warned: Vec<String>,
    /// What the dormer's drop last did to the notch or the edge it comes out of, numbered (D-86).
    notch_kick: Option<(u64, bezel::Kick)>,
    /// How far the notch's bottom (or the edge's) is drawn from where it rests, on the dormer's
    /// bar: the drop's neck flows from there.
    notch_sag: bezel::Sag,
    /// Bar surfaces, in the order they opened.
    windows: Vec<window::Id>,
    /// How wide each bar surface is, once laid out: where the notch is on it.
    widths: HashMap<window::Id, f32>,
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
    /// The `notch`, `screen` and `islands` settings, the top level's and each monitor's, being or
    /// last looked into. `None` before the first.
    screen_setting: Option<config::Screens>,
    /// The connector of the output the bar is on, once the first look into the screen found it:
    /// the one its settings are picked for (D-94), whichever has focus later.
    output: Option<String>,
    /// Whether a window covers the bar's screen, fullscreen (D-88).
    fullscreen: bool,
    /// Numbers the looks into whether one does, so only the last one's answer counts.
    checks: u64,
    /// Whether the pointer is on a bar that shows on hover, or left it less than
    /// [`style::HIDE_AFTER`] ago.
    hovered: bool,
    /// Numbers the times the pointer left the bar, so only the last one's timer hides it.
    leaves: u64,
    /// While a hidden bar is out for a moment: which peek, numbered by [`Bar::peeks`].
    peek: Option<u64>,
    peeks: u64,
    /// Whether the bar was out when the surfaces were last fitted to it.
    shown: bool,
    /// The layer the surfaces are on: `Overlay` while the bar shows over a fullscreen window.
    layer: Layer,
    /// While the bar slides away before going back under a fullscreen window: which time,
    /// numbered by [`Bar::sinks`].
    sinking: Option<u64>,
    sinks: u64,
    /// The desktop's color scheme, which `theme = "system"` follows (D-92).
    desktop: ColorScheme,
}

/// One island: its zone, and which of the zone's islands it is, counted in bar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Its body shows in full: "Show all" was clicked.
    unfolded: bool,
    /// The question it asked before this one, and since when, while that slides away.
    previous: Option<(NoticeParts<AnyMessage>, Instant)>,
}

impl Toast {
    /// What it stacks by: notices of the same widget, app and thread, and as urgent, that come
    /// one after another share a card. One without a thread has a card of its own.
    fn thread(&self) -> Option<(u64, Option<&str>, &str, bool)> {
        let notice = &self.notice;
        Some((
            self.instance,
            notice.app.as_deref(),
            notice.thread.as_deref()?,
            notice.urgency == Urgency::Critical,
        ))
    }
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
/// A widget dialog's popover key.
const DIALOG: u64 = u64::MAX - 2;
/// How tall the screen is taken to be, from the surface's top, when its height can't be had.
const CENTER_ROOM: f32 = 800.0;

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
    /// The pointer came onto the bar's surface, or left it.
    PointerEntered,
    PointerLeft,
    /// The pointer is on the top strip of the bar's surface, which brings a hidden bar out
    /// (D-88). Coming onto the notch or the edge doesn't (D-106).
    PointerAtTop,
    /// How wide a bar surface is, as its bezel was laid out.
    Width(window::Id, f32),
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
    /// A widget's message from something acted on in this island: its popover, or a notification.
    /// A dialog it asks for comes out of there.
    Acted(IslandId, u64, AnyMessage),
    /// Something happened to the dialog.
    Dialog(dialog::Event),
    /// `eave popover <widget>`: open that widget's popover, or close it (D-91).
    TogglePopover(String),
    /// `eave action <widget> <id>`: run that widget's action (D-91).
    RunAction(String, String),
    /// A collapsed item's time out is up, if this is still its last reveal.
    Unreveal(u64, u64),
    /// Hyprland's windows or workspaces changed: whether one covers the screen may have too.
    CheckFullscreen,
    /// Whether a window covers the screen, as of the check with this number.
    Fullscreen(u64, bool),
    /// The focused workspace changed.
    WorkspaceChanged,
    /// The pointer left a bar that shows on hover this long ago, if it hasn't come back since.
    Unhover(u64),
    /// A peek's moment is up, if it's still the last one.
    Unpeek(u64),
    /// The bar has slid away, and can go back under the fullscreen window, if it still should.
    Sink(u64),
    /// A bar surface opened: it takes the pointer only over its strip.
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    /// `config.toml` changed on disk: the new contents, or why they couldn't be loaded.
    ConfigChanged(Result<Config, String>),
    /// The desktop's color scheme, now or since it changed.
    ColorScheme(ColorScheme),
    /// The screen for the `notch`, `screen` and `islands` settings has been looked into, on this
    /// output.
    ScreenFound(config::Screens, Option<String>, notch::Screen),
    /// An app's icon has loaded, or turned out not to exist.
    AppIcon(String, Option<AppIcon>),
    /// A plugin's icon file has been drawn, or turned out not to be one.
    FileIcon(FileKey, Option<FileIcon>),
    Frame(FileKey, Option<Frame>),
    /// A cover has been fetched and drawn, or couldn't be.
    Cover(String, Option<Cover>),
    /// Messages for several widgets at once: closing a card of several notifications.
    Many(Vec<(u64, AnyMessage)>),
    /// "Show all" on a notification: its instance and key.
    Unfold(u64, u64),
    /// The line under the cards that says how many more wait: opens this instance's popover.
    MoreNotices(u64),
    /// A minute went by while notifications show: their times move on.
    Minute,
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
            notch_kick: None,
            notch_sag: bezel::Sag::default(),
            recent: Recent::default(),
            dialog: None,
            shapes: RefCell::default(),
            asked: None,
            touched: None,
            warned: Vec::new(),
            windows: Vec::new(),
            widths: HashMap::new(),
            timer: 0,
            alarm: None,
            expanded: Vec::new(),
            reveals: 0,
            panel_wanted: None,
            screen: notch::Screen::default(),
            screen_setting: None,
            output: None,
            fullscreen: false,
            checks: 0,
            hovered: false,
            leaves: 0,
            peek: None,
            peeks: 0,
            shown: true,
            layer: Layer::Top,
            sinking: None,
            sinks: 0,
            desktop: ColorScheme::NoPreference,
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
        let setting = config.screens();
        if self.screen_setting.as_ref() != Some(&setting) {
            self.screen_setting = Some(setting.clone());
            tasks.push(Task::perform(
                notch::resolve(setting.clone(), self.output.clone()),
                move |(output, screen)| Message::ScreenFound(setting.clone(), output, screen),
            ));
        }

        for entry in &config.entries {
            // Keys aren't the widget's: a widget whose keys changed keeps running with the new ones.
            if let Some(position) = old
                .iter()
                .position(|instance| instance.entry.same_but_keys(entry))
            {
                let mut instance = old.remove(position);
                instance.entry.keys = entry.keys.clone();
                self.instances.push(instance);
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
        if let Some(dialog) = &self.dialog
            && self.find(dialog.instance).is_none()
        {
            self.dialog = None;
            tasks.extend(self.toast_window().map(|window| self.fit(window)));
        }
        self.refresh_dormer();
        self.config = config;
        // Fitted to the new config: how the bar shows and how it's drawn size the surfaces.
        tasks.extend(self.windows.iter().map(|window| self.fit(*window)));
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
            if self.dialog_of(instance.id)
                && let Some(panel) = instance.widget.dialog()
            {
                kit::wanted(panel.into_node(), &mut wanted);
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
        // An app's icon by its id, or a file it gave for it, drawn like a cover.
        for app in notices
            .clone()
            .filter_map(|notice| notice.app_icon.as_ref().or(notice.app.as_ref()))
        {
            if kit::is_file(app) {
                wanted.covers.push(app.clone());
            } else {
                wanted.apps.push(app.clone());
            }
        }
        // A notification's own picture: by URL, or by where its pixels are.
        let mut pixels: HashMap<String, Pixels> = HashMap::new();
        for notice in notices {
            if let Some(picture) = &notice.pixels {
                pixels.insert(cover::pixels_key(picture), picture.clone());
            }
            wanted.covers.extend(cover::key(notice));
        }
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
        // Pixels are held while their picture is kept, so no other picture takes their address.
        let covers = &self.icons.covers;
        self.icons.pixels.retain(|key, _| covers.contains_key(key));
        for url in wanted.covers {
            if self.icons.covers.contains_key(&url) || self.fetching.contains(&url) {
                continue;
            }
            self.fetching.push(url.clone());
            let load = match pixels.remove(&url) {
                Some(picture) => {
                    self.icons.pixels.insert(url.clone(), picture.clone());
                    Task::perform(cover::load_pixels(picture), {
                        let url = url.clone();
                        move |cover| Message::Cover(url.clone(), cover)
                    })
                }
                None => Task::perform(cover::load(url.clone()), {
                    let url = url.clone();
                    move |cover| Message::Cover(url.clone(), cover)
                }),
            };
            tasks.push(load);
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

    /// The first widget in config.toml that `name` names, as `eave popover` and `eave action`
    /// do. Says so when there's none.
    fn named(&self, name: &str) -> Option<&Instance> {
        let found = self
            .instances
            .iter()
            .find(|instance| config::widget_name(&instance.entry.widget) == name);
        if found.is_none() {
            eprintln!("eave: no widget `{name}` in config.toml");
        }
        found
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
        let mut hooks = self.close_dialog();
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
        self.popovers.iter().any(|popover| popover.open) || self.dormer_open() || self.dialog_open()
    }

    fn dialog_open(&self) -> bool {
        self.dialog.as_ref().is_some_and(|dialog| dialog.open)
    }

    /// Whether `id`'s dialog is open.
    fn dialog_of(&self, id: u64) -> bool {
        self.dialog
            .as_ref()
            .is_some_and(|dialog| dialog.open && dialog.instance == id)
    }

    /// Notes that something was just acted on: a dialog asked for soon comes out of it.
    fn touch(&mut self, touch: Touch) {
        self.touched = Some(Touched {
            touch,
            at: Instant::now(),
        });
    }

    /// Starts sending the dialog back where it came from. Returns the message its widget asked
    /// for on closing.
    fn close_dialog(&mut self) -> Hooks {
        let Some(dialog) = self.dialog.as_mut().filter(|dialog| dialog.open) else {
            return Hooks::new();
        };
        dialog.open = false;
        let id = dialog.instance;
        self.find(id)
            .and_then(|instance| instance.widget.dialog())
            .and_then(|panel| panel.on_close_message())
            .map(|message| (id, message))
            .into_iter()
            .collect()
    }

    /// The message `id` asked for on its dialog opening.
    fn dialog_opened(&self, id: u64) -> Hooks {
        self.find(id)
            .and_then(|instance| instance.widget.dialog())
            .and_then(|panel| panel.on_open_message())
            .map(|message| (id, message))
            .into_iter()
            .collect()
    }

    /// Opens `id`'s dialog, out of what was just acted on. One on screen takes it in where it is,
    /// and the dormer turns into it. Otherwise the screen's height is looked up first, then it
    /// comes out of the island acted on, popover and all, or drops out of the top.
    fn ask_dialog(&mut self, id: u64) -> Task<Message> {
        let wanted = self
            .find(id)
            .is_some_and(|instance| instance.widget.dialog().is_some());
        if !wanted || self.dialog_of(id) || self.toast_window().is_none() {
            return Task::none();
        }
        let touch = Touched::fresh(self.touched, Instant::now());
        // Another's dialog, or one going back: it turns into this one, where it is.
        if self.dialog.is_some() {
            let mut hooks = self.close_popover();
            if let Some(dialog) = &mut self.dialog {
                dialog.instance = id;
                dialog.open = true;
            }
            hooks.extend(self.dialog_opened(id));
            return send(hooks);
        }
        // Picked in the dormer: the dormer turns into it, and it goes back into the notch.
        if touch == Some(Touch::Dormer)
            && let Some(room) = self.dormer.as_ref().map(|session| session.room)
        {
            let mut hooks = self.close_popover();
            self.dormer = None;
            self.dialog = Some(Dialog::new(id, room, Origin::Top));
            self.pointer_inside = false;
            hooks.extend(self.dialog_opened(id));
            return send(hooks);
        }
        let origin = match touch {
            Some(Touch::Island(island)) if self.island_shows(island) => Origin::Island(island),
            _ => Origin::Top,
        };
        self.asked = Some((id, origin));
        Task::perform(center(self.screen.inset[0]), |(_, room)| {
            Message::Dialog(dialog::Event::Opened(room))
        })
    }

    /// Where `island`'s shape is put as it's drawn.
    fn shape(&self, island: IslandId) -> island::Shape {
        self.shapes.borrow_mut().entry(island).or_default().clone()
    }

    /// Whether `island` is on the bar the dialog shows on, to come out of it.
    fn island_shows(&self, island: IslandId) -> bool {
        island.zone != Zone::Dormer
            && (self
                .instances
                .iter()
                .any(|instance| instance.island() == island)
                || island == self.notice_island() && self.notices.is_some())
    }

    /// Squishes the notch, or the edge, on the dormer's bar: the drop leaves it or lands in it.
    fn kick_notch(&mut self, kick: bezel::Kick) {
        let number = self.notch_kick.map_or(1, |(number, _)| number + 1);
        self.notch_kick = Some((number, kick));
    }

    fn dormer_open(&self) -> bool {
        self.dormer.as_ref().is_some_and(|session| session.open)
    }

    /// Everything the dormer can list: each widget's actions, with their keys, and its popover if
    /// it has one. Asked only while the dormer is open. Also says which keys were dropped.
    fn dormer_entries(&self) -> (Vec<dormer::Entry>, Vec<String>) {
        let mut entries = Vec::new();
        let mut wanted = Vec::new();
        for instance in &self.instances {
            let actions = instance
                .widget
                .actions()
                .into_iter()
                .map(Action::into_parts);
            for (index, action) in actions.enumerate() {
                wanted.push((entries.len(), action.id, action.shortcut, instance));
                entries.push(dormer::Entry {
                    instance: instance.id,
                    source: Source::Action(index),
                    title: action.title,
                    subtitle: action.subtitle,
                    keywords: action.keywords,
                    key: None,
                });
            }
            if instance.has_popover && instance.entry.zone != Zone::Dormer {
                entries.push(dormer::Entry {
                    instance: instance.id,
                    source: Source::Popover,
                    title: capitalized(config::widget_name(&instance.entry.widget)),
                    subtitle: Some("Open".into()),
                    keywords: Vec::new(),
                    key: None,
                });
            }
        }
        let candidates: Vec<_> = wanted
            .iter()
            .map(|(_, id, default, instance)| shortcut::Candidate {
                id: id.as_deref(),
                default: *default,
                keys: &instance.entry.keys,
                widget: config::widget_name(&instance.entry.widget),
            })
            .collect();
        let (keys, warnings) = shortcut::resolve_actions(&candidates);
        for ((at, ..), key) in wanted.iter().zip(keys) {
            entries[*at].key = key;
        }
        (entries, warnings)
    }

    /// Asks the widgets again what the open dormer lists.
    fn refresh_dormer(&mut self) {
        if !self.dormer_open() {
            return;
        }
        let (entries, warnings) = self.dormer_entries();
        for warning in warnings {
            if !self.warned.contains(&warning) {
                eprintln!("eave: keys: {warning}");
                self.warned.push(warning);
            }
        }
        if let Some(session) = &mut self.dormer {
            session.refresh(entries, &self.recent);
        }
    }

    /// Picks the dormer's row at `index`: it folds back, and the row's widget gets its message,
    /// or opens its popover.
    fn run_dormer(&mut self, index: usize) -> Task<Message> {
        let entry = self
            .dormer
            .as_ref()
            .filter(|session| session.open)
            .and_then(|session| session.row(index))
            .cloned();
        self.run_entry(entry)
    }

    /// Runs the dormer's entry whose key was pressed, as if its row was picked.
    fn run_dormer_key(&mut self, index: usize) -> Task<Message> {
        let entry = self
            .dormer
            .as_ref()
            .filter(|session| session.open)
            .and_then(|session| session.entries.get(index))
            .cloned();
        self.run_entry(entry)
    }

    fn run_entry(&mut self, entry: Option<dormer::Entry>) -> Task<Message> {
        let Some(entry) = entry else {
            return Task::none();
        };
        self.recent.push(&entry);
        self.touch(Touch::Dormer);
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
    /// Where the notch and the grown edge are on `window`, while they stay out with the bar
    /// hidden: not over a fullscreen window, and not as a solid bar's band (D-106).
    fn bezel_rects(&self, window: window::Id) -> Vec<Rectangle> {
        let Some(&width) = self.widths.get(&window) else {
            return Vec::new();
        };
        if self.fullscreen || self.config.bar.style == Style::Solid {
            return Vec::new();
        }
        let edge = (self.screen.grow > 0.0)
            .then(|| Rectangle::new(Point::ORIGIN, Size::new(width, self.screen.grow)));
        let notch = self.screen.notch.map(|notch| {
            let reach = notch.width + 2.0 * notch.shoulder;
            let bottom = self.screen.notch_top(notch) - self.screen.inset[0] + notch.height;
            Rectangle::new(
                Point::new((width - reach) / 2.0, 0.0),
                Size::new(reach, bottom),
            )
        });
        edge.into_iter().chain(notch).collect()
    }

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
        let dialog = self
            .dialog
            .as_ref()
            .filter(|_| self.toast_window() == Some(window))
            .and_then(|dialog| dialog.rect);
        let rects: Vec<Rectangle> = self
            .popovers
            .iter()
            .filter(|popover| popover.window == window)
            .filter_map(|popover| popover.rect)
            .chain(notices)
            .chain(dormer)
            .chain(dialog)
            .collect();
        let height = rects
            .iter()
            .map(|rect| (rect.y + rect.height).ceil() as u32 + u32::from(style::EDGE_GAP))
            .fold(self.surface_height(), u32::max);
        // A hidden bar takes the pointer only at the top edge, if hovering brings it out (D-88),
        // and on the notch and the grown edge, which stay (D-106).
        let strip = if self.out() {
            self.screen.bar_height()
        } else if self.showing() == Showing::Hover {
            style::REVEAL_EDGE
        } else {
            0
        };
        let rects: Vec<Rectangle> = if self.out() {
            rects
        } else {
            rects.into_iter().chain(self.bezel_rects(window)).collect()
        };
        let callback = ActionCallback::new(move |region| {
            // iced_layershell reuses one region and only clears the surface's current size, which
            // may be the old one: clear all of it, so no earlier popover's rect is left behind.
            region.subtract(0, 0, i32::MAX, i32::MAX);
            if strip > 0 {
                region.add(0, 0, i32::MAX, strip as i32);
            }
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
    /// islands as tall as they are (D-76). A bar that shows on hover keeps them clear of nothing,
    /// even while it's out, so they never move (D-88).
    fn inset(&self, window: window::Id) -> Task<Message> {
        let zone = match self.config.bar.show {
            Showing::Hover => 0,
            Showing::Always | Showing::Never => self.screen.bar_height() as i32,
        };
        let [top, right, _, left] = self.screen.inset.map(|inset| inset.round() as i32);
        Task::batch([
            Task::done(Message::MarginChange {
                id: window,
                margin: (top, right, 0, left),
            }),
            Task::done(Message::ExclusiveZoneChange {
                id: window,
                zone_size: zone,
            }),
        ])
    }

    /// The bar surface at rest: the screen's, or down to where a solid bar's rounded ends end,
    /// so they're drawn whole (D-89). Only the bar's strip takes the pointer.
    fn surface_height(&self) -> u32 {
        let surface = self.screen.surface_height();
        if self.config.bar.style != Style::Solid || self.config.bar.corners != BarCorners::Round {
            return surface;
        }
        let reach = self.screen.corners[..2]
            .iter()
            .map(|corner| corner.radius * (1.0 + corner.smoothing))
            .fold(0.0, f32::max);
        surface.max((band_bottom(&self.screen) + reach).ceil() as u32)
    }

    /// How the bar shows right now: outside fullscreen, or over a fullscreen window (D-88).
    /// Which way round the palette is: as `theme` says, or for `"system"` as the desktop
    /// prefers, dark unless it prefers light.
    pub fn scheme(&self) -> Scheme {
        match (self.config.theme, self.desktop) {
            (config::Theme::Light, _) | (config::Theme::System, ColorScheme::Light) => {
                Scheme::Light
            }
            _ => Scheme::Dark,
        }
    }

    fn palette(&self) -> &'static Palette {
        self.scheme().palette()
    }

    fn showing(&self) -> Showing {
        if self.fullscreen {
            self.config.bar.fullscreen
        } else {
            self.config.bar.show
        }
    }

    /// Whether the bar is out: always, or while it's hovered, peeking, or has something to show
    /// that grows out of it, until that has folded back.
    fn out(&self) -> bool {
        match self.showing() {
            Showing::Always => true,
            Showing::Hover if self.hovered => true,
            Showing::Hover | Showing::Never => {
                self.peek.is_some()
                    || !self.popovers.is_empty()
                    || self.notices.is_some()
                    || self.dormer.is_some()
                    || self.dialog.is_some()
            }
        }
    }

    /// Whether a notification waits for fullscreen to end (D-88). It doesn't show, and its time
    /// doesn't run.
    fn held(&self, toast: &Toast) -> bool {
        self.fullscreen
            && match self.config.notifications.fullscreen {
                Held::None => false,
                Held::Uncritical => toast.notice.urgency != Urgency::Critical,
                Held::All => true,
            }
    }

    /// Brings a hidden bar that shows on hover out for [`style::REVEAL_SHOWN`], or keeps it out
    /// that long again. One that's out anyway stays as it is, and one that's hidden on purpose
    /// stays hidden.
    fn peek(&mut self) -> Task<Message> {
        if self.showing() != Showing::Hover || (self.out() && self.peek.is_none()) {
            return Task::none();
        }
        self.peeks += 1;
        let peek = self.peeks;
        self.peek = Some(peek);
        let sleep = async { tokio::time::sleep(style::REVEAL_SHOWN).await };
        Task::perform(sleep, move |_| Message::Unpeek(peek))
    }

    /// Tells the surfaces what changed about the bar showing: the pointer it takes, and the
    /// layer it's on. Over a fullscreen window it goes on the overlay layer while it shows or
    /// waits for the pointer, and back under once it's hidden on purpose and has slid away, so
    /// the window can have the screen to itself (D-88).
    fn sync_shown(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let out = self.out();
        if out != self.shown {
            self.shown = out;
            tasks.extend(self.windows.iter().map(|window| self.fit(*window)));
        }
        let layer = if self.fullscreen && (out || self.showing() != Showing::Never) {
            Layer::Overlay
        } else {
            Layer::Top
        };
        if layer == self.layer {
            self.sinking = None;
        } else if layer == Layer::Top && self.fullscreen {
            if self.sinking.is_none() {
                self.sinks += 1;
                let sink = self.sinks;
                self.sinking = Some(sink);
                let sleep = async { tokio::time::sleep(lift::slide_time()).await };
                tasks.push(Task::perform(sleep, move |_| Message::Sink(sink)));
            }
        } else {
            self.sinking = None;
            tasks.push(self.move_to(layer));
        }
        Task::batch(tasks)
    }

    /// Puts every bar surface on `layer`.
    fn move_to(&mut self, layer: Layer) -> Task<Message> {
        self.layer = layer;
        Task::batch(
            self.windows
                .iter()
                .map(|&id| Task::done(Message::LayerChange { id, layer })),
        )
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
    /// thing at a time. Not for a dialog that came out of it: the island is back meanwhile.
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
                    // On to another question: the one before slides away.
                    let step = |notice: &NoticeParts<AnyMessage>| {
                        notice.step.as_ref().map(|step| step.index)
                    };
                    if step(&toast.notice) != step(&notice) && step(&notice).is_some() {
                        let before = std::mem::replace(&mut toast.notice, notice.clone());
                        toast.previous = Some((before, Instant::now()));
                        toast.unfolded = false;
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
                    unfolded: false,
                    previous: None,
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
        } else if self
            .toasts
            .iter()
            .any(|toast| !toast.leaving && !self.held(toast))
        {
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
            // Those waiting for fullscreen to end stay.
            self.toasts.retain(|toast| !toast.leaving);
        }
        if refit {
            self.fit(window)
        } else {
            Task::none()
        }
    }

    /// The panel's cards, in the order they show, each the indices of its notifications in
    /// [`toasts`](Self::toasts), newest first. Critical ones come first. A notification that
    /// follows one of the same thread joins its card ([D-85](../../../docs/decisions.md)). Only the
    /// first [`style::NOTICES_SHOWN`] show; the rest wait. While the panel folds back, the cards
    /// are the ones it last showed.
    fn cards(&self) -> Vec<Vec<usize>> {
        let open = self.notices.as_ref().is_none_or(|panel| panel.open);
        let mut order: Vec<usize> = (0..self.toasts.len())
            .filter(|&index| self.toasts[index].leaving != open && !self.held(&self.toasts[index]))
            .collect();
        order.sort_by_key(|&index| self.toasts[index].notice.urgency != Urgency::Critical);
        let mut cards: Vec<Vec<usize>> = Vec::new();
        for index in order {
            let thread = self.toasts[index].thread();
            match cards.last_mut() {
                Some(card) if thread.is_some() && self.toasts[card[0]].thread() == thread => {
                    card.push(index);
                }
                _ => cards.push(vec![index]),
            }
        }
        cards
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
        // Only a card's newest notice counts, and only on the cards that show: the earlier ones
        // go with it, and the cards waiting haven't been seen.
        let counted: Vec<usize> = self
            .cards()
            .iter()
            .take(style::NOTICES_SHOWN)
            .map(|card| card[0])
            .collect();
        for (index, toast) in self.toasts.iter_mut().enumerate() {
            let running = shown && !toast.leaving && counted.contains(&index);
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
    /// The earlier notices of a card go with its newest.
    fn expire(&mut self, now: Instant) -> Hooks {
        self.alarm = None;
        let mut hooks = Hooks::new();
        for card in self.cards() {
            let due = self.toasts[card[0]]
                .deadline
                .is_some_and(|deadline| deadline <= now);
            if !due {
                continue;
            }
            for index in card {
                let toast = &mut self.toasts[index];
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
    tasks.push(bar.sync_shown());
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
        .or_else(|| {
            (bar.dormer_open() || bar.dialog_open())
                .then(|| bar.toast_window())
                .flatten()
        })
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
            let effect = instance.widget.update(message);
            let dialog = effect.dialog_request();
            let (task, request, nudge) = effect.into_parts();
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
                // Something happened worth showing: a collapsed item comes out for a while, and
                // a hidden bar with it.
                reveal = Task::batch([bar.reveal(id), bar.peek()]);
            }

            let popover = match request {
                Some(PopoverRequest::Open) => bar.open_popover(id),
                Some(PopoverRequest::Close) if bar.popover_of(id).is_some() => bar.close_popover(),
                // A widget that stops offering a popover closes it.
                _ if bar.popover_of(id).is_some() && !has_popover => bar.close_popover(),
                _ => Hooks::new(),
            };
            let dialog = match dialog {
                Some(DialogRequest::Open) => bar.ask_dialog(id),
                Some(DialogRequest::Close) if bar.dialog_of(id) => send(bar.close_dialog()),
                // A widget that stops offering its dialog closes it.
                _ if bar.dialog_of(id)
                    && bar
                        .find(id)
                        .is_none_or(|instance| instance.widget.dialog().is_none()) =>
                {
                    send(bar.close_dialog())
                }
                _ => Task::none(),
            };
            Task::batch([
                task.map(move |message| Message::Widget(id, message)),
                send(popover),
                dialog,
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
                let island = instance.island();
                bar.touch(Touch::Island(island));
            }
            Task::none()
        }
        Message::Acted(island, id, message) => {
            bar.touch(Touch::Island(island));
            handle(bar, Message::Widget(id, message))
        }
        Message::Dialog(event) => handle_dialog(bar, event),
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
        Message::Many(hooks) => send(hooks),
        Message::Unfold(instance, key) => {
            if let Some(toast) = bar
                .toasts
                .iter_mut()
                .find(|toast| toast.instance == instance && toast.notice.key == key)
            {
                toast.unfolded = true;
            }
            Task::none()
        }
        Message::MoreNotices(id) => send(bar.open_popover(id)),
        Message::Minute => Task::none(),
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
            // Back on a bar that's still out: it stays. A hidden one takes the pointer on the
            // notch and the edge too, which don't bring it out: only the top strip does.
            if bar.showing() == Showing::Hover && bar.out() {
                bar.hovered = true;
                bar.leaves += 1;
            }
            Task::none()
        }
        Message::PointerAtTop => {
            if bar.showing() == Showing::Hover && !bar.hovered {
                bar.hovered = true;
                bar.leaves += 1;
            }
            Task::none()
        }
        Message::Width(window, width) => {
            if bar.widths.insert(window, width) == Some(width) {
                return Task::none();
            }
            bar.fit(window)
        }
        Message::PointerLeft => {
            bar.pointer_inside = false;
            if !bar.hovered {
                return Task::none();
            }
            bar.leaves += 1;
            let leave = bar.leaves;
            let sleep = async { tokio::time::sleep(style::HIDE_AFTER).await };
            Task::perform(sleep, move |_| Message::Unhover(leave))
        }
        Message::Unhover(leave) => {
            if leave == bar.leaves {
                bar.hovered = false;
            }
            Task::none()
        }
        Message::Unpeek(peek) => {
            if bar.peek == Some(peek) {
                bar.peek = None;
            }
            Task::none()
        }
        Message::Sink(sink) => {
            if bar.sinking != Some(sink) {
                return Task::none();
            }
            bar.sinking = None;
            bar.move_to(Layer::Top)
        }
        Message::CheckFullscreen => {
            bar.checks += 1;
            let check = bar.checks;
            Task::perform(covered(), move |covered| {
                Message::Fullscreen(check, covered)
            })
        }
        Message::Fullscreen(check, covered) => {
            if check != bar.checks || covered == bar.fullscreen {
                return Task::none();
            }
            bar.fullscreen = covered;
            // Wherever the pointer is, the bar starts over from how it shows now.
            bar.hovered = false;
            bar.peek = None;
            Task::none()
        }
        Message::WorkspaceChanged => {
            let peek = if bar.config.bar.peek {
                bar.peek()
            } else {
                Task::none()
            };
            Task::batch([handle(bar, Message::CheckFullscreen), peek])
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
        Message::TogglePopover(name) => {
            let Some(instance) = bar.named(&name) else {
                return Task::none();
            };
            let id = instance.id;
            if !instance.has_popover || instance.entry.zone == Zone::Dormer {
                eprintln!("eave: `{name}` has no popover to open");
                return Task::none();
            }
            if bar.popover_of(id).is_some() {
                send(bar.close_popover())
            } else {
                send(bar.open_popover(id))
            }
        }
        Message::RunAction(name, action) => {
            let Some(instance) = bar.named(&name) else {
                return Task::none();
            };
            let id = instance.id;
            let message = instance
                .widget
                .actions()
                .into_iter()
                .find(|candidate| candidate.id_str() == Some(action.as_str()))
                .map(|found| found.into_parts().message);
            match message {
                Some(message) => handle(bar, Message::Widget(id, message)),
                None => {
                    eprintln!("eave: `{name}` has no action `{action}` right now");
                    Task::none()
                }
            }
        }
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
                bar.dialog = None;
                bar.toasts.retain(|toast| !toast.leaving);
            }
            bar.windows.retain(|open| *open != window);
            bar.widths.remove(&window);
            bar.popovers.retain(|popover| popover.window != window);
            if bar.keyboard == Some(window) {
                bar.keyboard = None;
            }
            Task::none()
        }
        Message::ScreenFound(setting, output, screen) => {
            if bar.output.is_none() {
                bar.output = output;
            }
            // Looked into for a setting that has changed since: its own answer is on the way.
            if bar.screen_setting.as_ref() != Some(&setting) || bar.screen == screen {
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
        Message::ColorScheme(scheme) => {
            bar.desktop = scheme;
            Task::none()
        }
        Message::ConfigChanged(config) => match config {
            // Editors often fire several events per save; only a real change rebuilds anything.
            Ok(config) if config == bar.config => Task::none(),
            Ok(config) => {
                if config.renderer != bar.config.renderer {
                    eprintln!("eave: config: the new `renderer` is used once the bar restarts");
                }
                let show = bar.config.bar.show;
                let applied = bar.apply(config);
                // Hovering or not, it keeps windows clear of it or doesn't.
                if bar.config.bar.show == show {
                    return applied;
                }
                bar.hovered = false;
                let insets = bar.windows.iter().map(|window| bar.inset(*window));
                Task::batch(std::iter::once(applied).chain(insets))
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
            Task::perform(center(inset), |(top, room)| {
                Message::Dormer(Event::Opened(top, room))
            })
        }
        Event::Toggle => Task::none(),
        Event::Opened(top, room) => {
            if bar.dormer_open() || bar.toast_window().is_none() {
                return Task::none();
            }
            let hooks = bar.close_popover();
            // It takes over the island in the middle from a dialog going back.
            bar.dialog = None;
            // Never over the bar.
            let top = top.max(bar.screen.surface_height() as f32);
            // A keybind opened it, not a click: the pointer is wherever it was.
            bar.pointer_inside = false;
            match &mut bar.dormer {
                // Still folding back: it turns around, and starts over empty.
                Some(session) => {
                    session.open = true;
                    session.top = top;
                    session.room = room;
                    session.search(String::new(), &bar.recent);
                }
                None => bar.dormer = Some(Session::new(top, room)),
            }
            bar.refresh_dormer();
            send(hooks)
        }
        // What it comes out of reacts the moment the drop leaves it or touches it again.
        Event::Neck(caught) => {
            bar.kick_notch(if caught {
                bezel::Kick::Flatten
            } else {
                bezel::Kick::Pinch
            });
            Task::none()
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
        Event::Key(index) => bar.run_dormer_key(index),
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

fn handle_dialog(bar: &mut Bar, event: dialog::Event) -> Task<Message> {
    use dialog::Event;
    match event {
        Event::Opened(room) => {
            let Some((id, origin)) = bar.asked.take() else {
                return Task::none();
            };
            let wanted = bar
                .find(id)
                .is_some_and(|instance| instance.widget.dialog().is_some());
            if !wanted || bar.dialog_open() || bar.toast_window().is_none() {
                return Task::none();
            }
            // The popover it comes out of goes at once, under the dialog's shape as that sets
            // off: there's nothing left of it to fold back, and the island is back. The dormer
            // gives way.
            let hooks = bar.close_popover();
            if let Origin::Island(island) = origin {
                let instances = &bar.instances;
                bar.popovers.retain(|popover| {
                    !instances.iter().any(|instance| {
                        instance.id == popover.instance && instance.island() == island
                    })
                });
            }
            let mut hooks = hooks;
            bar.dormer = None;
            bar.dialog = Some(Dialog::new(id, room, origin));
            // Out of the bar, it was opened by a click there; out of the top, by nothing there.
            bar.pointer_inside = origin != Origin::Top;
            hooks.extend(bar.dialog_opened(id));
            send(hooks)
        }
        // The notch reacts the moment the drop leaves it or touches it again.
        Event::Neck(caught) => {
            bar.kick_notch(if caught {
                bezel::Kick::Flatten
            } else {
                bezel::Kick::Pinch
            });
            Task::none()
        }
        Event::Placed(rect) => {
            let window = bar.toast_window();
            match (&mut bar.dialog, window) {
                (Some(dialog), Some(window)) => {
                    dialog.rect = Some(rect);
                    bar.fit(window)
                }
                _ => Task::none(),
            }
        }
        Event::Closed => {
            if bar.dialog.as_ref().is_none_or(|dialog| dialog.open) {
                return Task::none();
            }
            bar.dialog = None;
            match bar.toast_window() {
                Some(window) => bar.fit(window),
                None => Task::none(),
            }
        }
    }
}

/// Where the dormer's top goes, from the surface's: a fifth of the way down the focused screen;
/// and how far that screen reaches down from the surface's top, which a dialog is centered in.
/// `inset` is how far down the surface starts.
async fn center(inset: f32) -> (f32, f32) {
    let height = match notch::focused().await {
        // Turned a quarter, the screen's height is the mode's width.
        Some(monitor) => {
            let height = if monitor.transform % 2 == 1 {
                monitor.width
            } else {
                monitor.height
            };
            height as f32 / monitor.scale
        }
        None => CENTER_ROOM + inset,
    };
    ((height / 5.0 - inset).round(), (height - inset).round())
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
    let palette = bar.palette();
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

    // A solid bar is black down to the islands' bottom, and they flow out of it (D-89).
    let band = (bar.config.bar.style == Style::Solid).then(|| band_bottom(&bar.screen));
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
                let Some(item) = item_view(instance, window, &bar.icons, palette) else {
                    continue;
                };
                // Only between two items that show: one next to a hidden item goes with it.
                if instance.entry.divider && !items.is_empty() {
                    items.push((instance.id | DIVIDER_KEY, kit::divider(palette)));
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
            // A popover wins: the notifications wait for it.
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
                element: toggle_view(island, side, expanded, palette),
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
                    .bleed(match band {
                        // Up into the bar, so nothing moving ever comes apart from it.
                        Some(_) => Padding {
                            top: band_bottom(&bar.screen),
                            ..bleed(attach)
                        },
                        None => bleed(attach),
                    })
                    .shows(shows(attach))
                    .notch(bar.screen.notch.map(|notch| notch.width))
                    // Flowing out of a solid bar, as round where it leaves as the island is.
                    .shoulders(match band {
                        Some(_) => Some(bar.screen.islands),
                        None => shoulders,
                    })
                    .band(band)
                    .corners(bar.screen.island_corners(Place::default()))
                    .height(bar.screen.island_height())
                    .shape(bar.shape(island))
                    // The screen's edges are walls, but where it's attached (D-104).
                    .walls(Walls {
                        top: (!attach.top && band.is_none()).then_some(bar.screen.grow),
                        left: !attach.left,
                        right: !attach.right,
                        corners: [bar.screen.corners[0], bar.screen.corners[1]],
                    }),
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
    // Pushed in from a corner too round for its island to follow, unless attached to that side
    // (D-95).
    let mut shift = bar.screen.shift;
    if left.first().is_some_and(|(_, attach)| attach.left) {
        shift[0] = 0.0;
    }
    if !left.is_empty() {
        let (first, attach) = left.remove(0);
        left.insert(0, (placed(first, &bar.screen, corner(Side::Left)), attach));
    }
    let mut right = islands(Zone::Right, Horizontal::Right);
    if right.last().is_some_and(|(_, attach)| attach.right) {
        shift[1] = 0.0;
    }
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
            Zones::around_notch(left, [zone(before), zone(after)], right, notch, fused)
        }
    };
    // The notch and the grown edge, from the surface's top, which is below what the bezel hides.
    let bezel = Bezel::default()
        .notch(
            bar.screen.notch,
            bar.screen.notch.map_or(0.0, |notch| {
                bar.screen.notch_top(notch) - bar.screen.inset[0]
            }),
        )
        .edge(
            bar.screen.grow,
            match (band, bar.config.bar.corners) {
                (Some(_), BarCorners::Square) => [CornerShape::SQUARE; 2],
                _ => [bar.screen.corners[0], bar.screen.corners[1]],
            },
        )
        .solid(band.map(|bottom| bottom - bar.screen.grow))
        .kick(
            bar.notch_kick
                .filter(|_| bar.toast_window() == Some(window)),
        )
        .sag((bar.toast_window() == Some(window)).then(|| bar.notch_sag.clone()))
        // A click on the notch, or the middle of the top, brings the dormer out (D-106).
        .on_click(Message::Dormer(dormer::Event::Toggle))
        .on_width(move |width| Message::Width(window, width));

    // No background of its own: the surface is transparent around and between the islands, and
    // below them, where a dragged island has room to move and popovers grow.
    let islands = container(zones).padding(Padding {
        top: f32::from(style::EDGE_GAP) + bar.screen.grow,
        right: f32::from(style::EDGE_GAP) + shift[1],
        bottom: style::DRAG_ROOM.into(),
        left: f32::from(style::EDGE_GAP) + shift[0],
    });
    // Lifted off the screen while the bar hides, far enough to clear it (D-88). The notch and the
    // grown edge aren't the bar and stay, unless the bar is solid, when the band is the bar, or
    // over a fullscreen window, which they'd draw over (D-106).
    let by = bar.surface_height() as f32;
    let stays = band.is_none() && !bar.fullscreen;
    let bezel = Lift::new(bezel, stays || bar.out(), by);
    let islands = Lift::new(islands, bar.out(), by);
    // Always a stack, so the islands' state stays put when the dormer comes and goes.
    stack![bezel, islands, center_view(bar, window)].into()
}

/// The island in the middle at the top, for the dormer or a widget's dialog: a dot where it comes
/// out of the notch, or the top edge, which drops out as a ball and unfolds into it (D-86). The
/// same island for both, so the dormer can turn into a dialog (D-90). A dialog that comes out of
/// an island on the bar starts as that island's shape instead, and the island stays. Only there
/// while one is, on the bar the notifications show on.
fn center_view(bar: &Bar, window: window::Id) -> Element<'_, Message> {
    if bar.toast_window() != Some(window) {
        return Space::new().into();
    }
    let popover = match (&bar.dialog, &bar.dormer) {
        (Some(dialog), _) => dialog_popover(bar, dialog),
        (None, Some(session)) => dormer_popover(bar, session),
        (None, None) => return Space::new().into(),
    };
    let island = Island::new([(DORMER, Space::new().into())])
        .align(Horizontal::Center)
        .popover(Some(popover))
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

/// Where a solid bar ends, from the surface's top: the grown edge, the gap and the islands
/// (D-89). The islands rest flush with it.
fn band_bottom(screen: &notch::Screen) -> f32 {
    screen.grow + f32::from(style::EDGE_GAP) + screen.island_height()
}

/// A widget's dialog, as the island in the middle turns into it: out of the shape of the island
/// acted on, its popover or its items, which morphs straight into it; or out of the notch like
/// the dormer.
fn dialog_popover<'a>(bar: &'a Bar, dialog: &Dialog) -> island::Popover<'a, Message> {
    use dialog::Event;
    let id = dialog.instance;
    // The scroll gives way on a short screen, so it always fits.
    let scroll = (dialog.room - style::DIALOG_CHROME).clamp(0.0, style::DIALOG_SCROLL_MAX_HEIGHT);
    // A widget can stop offering its dialog while it goes back: it goes back empty.
    let mut keys = Vec::new();
    let content: Element<'_, AnyMessage> = match bar
        .find(id)
        .and_then(|instance| Some((instance.widget.dialog()?, &instance.entry.keys)))
    {
        Some((panel, user_keys)) => kit::dialog(
            panel_keys(panel.into_node(), user_keys, &mut keys),
            &bar.icons,
            bar.palette(),
            scroll,
        ),
        None => Space::new().into(),
    };
    let home = match dialog.origin {
        Origin::Island(island) => Some(bar.shape(island)),
        Origin::Top => None,
    };
    let top = home.is_none();
    island::Popover {
        key: DIALOG,
        content: content.map(move |message| Message::Widget(id, message)),
        open: dialog.open,
        on_placed: Box::new(|rect| Message::Dialog(Event::Placed(rect))),
        on_closed: Message::Dialog(Event::Closed),
        on_hover: None,
        width: style::DIALOG_WIDTH,
        placement: Placement::Middle {
            room: dialog.room,
            min_top: bar.screen.surface_height() as f32,
        },
        drop: top.then(|| drop_top(bar)),
        drop_color: drop_color(bar).filter(|_| top),
        on_neck: top.then(|| -> Box<dyn Fn(bool) -> Message> {
            Box::new(|caught| Message::Dialog(Event::Neck(caught)))
        }),
        sag: top.then(|| bar.notch_sag.clone()),
        home,
        on_key: rows_on_key(id, keys),
    }
}

/// Where a drop comes out, the dormer's or a dialog's: out of a solid bar's bottom, or like the
/// dormer.
fn drop_top(bar: &Bar) -> f32 {
    match bar.config.bar.style {
        Style::Solid => band_bottom(&bar.screen),
        Style::Islands => dormer_drop(&bar.screen),
    }
}

/// The color of what a drop comes out of, when it isn't the islands': the notch's black, or the
/// top edge's (D-92). A solid bar's band is the islands' color.
fn drop_color(bar: &Bar) -> Option<Color> {
    match bar.config.bar.style {
        Style::Solid => None,
        Style::Islands => Some(style::BEZEL),
    }
}

/// Where the dormer comes out, from the surface's top: the bottom of the notch as the bar draws it,
/// grown, or the bottom of the grown top edge. Its neck flows out of that.
fn dormer_drop(screen: &notch::Screen) -> f32 {
    match screen.notch {
        Some(notch) => screen.notch_top(notch) + notch.height - screen.inset[0],
        None => screen.grow,
    }
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
    let keys: Vec<(ui::Shortcut, usize)> = session
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| Some((entry.key?, index)))
        .collect();
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
            // Already the key that works: the name only matters for config.toml.
            if let Some(key) = entry.key {
                item = item.shortcut(entry.title.clone(), key);
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
    let content = kit::panel(node, &bar.icons, bar.palette())
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
        drop: Some(drop_top(bar)),
        drop_color: drop_color(bar),
        on_neck: Some(Box::new(|caught| Message::Dormer(Event::Neck(caught)))),
        sag: Some(bar.notch_sag.clone()),
        home: None,
        on_key: Some(Box::new(move |key, modifiers, _| {
            use keyboard::key::Named;
            match key {
                keyboard::Key::Named(Named::ArrowDown | Named::Tab) => {
                    Some(Message::Dormer(Event::Move(1)))
                }
                keyboard::Key::Named(Named::ArrowUp) => Some(Message::Dormer(Event::Move(-1))),
                // Every action's key, shown or not. None of them types (D-91).
                _ => keys
                    .iter()
                    .find(|(shortcut, _)| shortcut::matches(shortcut, key, modifiers))
                    .map(|&(_, index)| Message::Dormer(Event::Key(index))),
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
fn toggle_view<'a>(
    island: IslandId,
    side: Horizontal,
    expanded: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let icon = if (side == Horizontal::Left) != expanded {
        Icon::Back
    } else {
        Icon::Forward
    };
    let content = container(icon::view(icon, palette.dim))
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
    palette: &'static Palette,
) -> Option<Element<'a, Message>> {
    let id = instance.id;
    let parts = instance.widget.item().into_parts();
    let on_press = parts.on_press;
    let content: Element<'_, AnyMessage> = container(kit::node(parts.content?, icons, palette))
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

/// A panel's rows with the keys config.toml gives them, so the keycaps show the keys that work,
/// and those keys collected into `keys` (D-91).
fn panel_keys(
    mut node: ui::Node<AnyMessage>,
    user_keys: &shortcut::Keys,
    keys: &mut Vec<(ui::Shortcut, AnyMessage)>,
) -> ui::Node<AnyMessage> {
    shortcut::resolve_panel(&mut node, user_keys);
    shortcut::collect(&node, keys);
    node
}

/// The rows' shortcuts press them; one that types gives way to a focused field (D-91).
fn rows_on_key<'a>(
    id: u64,
    keys: Vec<(ui::Shortcut, AnyMessage)>,
) -> Option<island::OnKey<'a, Message>> {
    (!keys.is_empty()).then(|| {
        Box::new(move |key: &keyboard::Key, modifiers, typing| {
            keys.iter()
                .find(|(keys, _)| {
                    !(typing && shortcut::types(keys)) && shortcut::matches(keys, key, modifiers)
                })
                .map(|(_, message)| Message::Widget(id, message.clone()))
        }) as island::OnKey<'a, Message>
    })
}

fn popover_view<'a>(bar: &'a Bar, popover: &Popover) -> island::Popover<'a, Message> {
    let id = popover.instance;
    // A widget can stop offering its popover while it closes: the island morphs back empty.
    let mut width = style::PANEL_WIDTH;
    let mut keys = Vec::new();
    let content: Element<'_, AnyMessage> = match bar
        .find(id)
        .and_then(|instance| Some((instance.widget.popover()?, &instance.entry.keys)))
    {
        Some((panel, user_keys)) => {
            if panel.is_wide() {
                width = style::PANEL_WIDE_WIDTH;
            }
            kit::panel(
                panel_keys(panel.into_node(), user_keys, &mut keys),
                &bar.icons,
                bar.palette(),
            )
        }
        None => Space::new().into(),
    };
    let on_key = rows_on_key(id, keys);
    let island = bar.find(id).map(Instance::island);
    island::Popover {
        key: id,
        content: content.map(move |message| match island {
            Some(island) => Message::Acted(island, id, message),
            None => Message::Widget(id, message),
        }),
        open: popover.open,
        on_placed: Box::new(move |rect| Message::PopoverPlaced(id, rect)),
        on_closed: Message::PopoverClosed(id),
        on_hover: None,
        width,
        placement: Placement::Below,
        on_key,
        drop: None,
        drop_color: None,
        on_neck: None,
        sag: None,
        home: None,
    }
}

fn notices_view<'a>(bar: &'a Bar, panel: &NoticePanel) -> island::Popover<'a, Message> {
    let now = SystemTime::now();
    let notice_island = bar.notice_island();
    let mut cards = bar.cards();
    let waiting = cards.split_off(cards.len().min(style::NOTICES_SHOWN));
    // While it folds back, it fades out the last ones it showed.
    let shown = cards.into_iter().map(|card| {
        let head = &bar.toasts[card[0]];
        let instance = head.instance;
        let key = head.notice.key;
        let countdown = head.notice.expires.as_ref().map(|(total, _)| Countdown {
            total: *total,
            left: head.left.unwrap_or_default(),
            ends: head.deadline,
        });
        // Closing the card closes every notice on it.
        let dismiss: Vec<_> = card
            .iter()
            .filter_map(|&index| {
                let toast = &bar.toasts[index];
                Some((toast.instance, toast.notice.on_dismiss.clone()?))
            })
            .collect();
        let dismiss = (!dismiss.is_empty()).then_some(Message::Many(dismiss));
        let earlier = card
            .iter()
            .skip(1)
            .take(style::NOTICE_STACK_LINES)
            .map(|&index| bar.toasts[index].notice.clone())
            .collect();
        // The question before, for as long as it may still be sliding away.
        let previous = head
            .previous
            .as_ref()
            .filter(|(_, since)| since.elapsed() < 2 * style::TAB_FADE)
            .map(|(notice, _)| notice.clone());
        let element = kit::notice(
            kit::Card {
                notice: head.notice.clone(),
                earlier,
                countdown,
                unfolded: head.unfolded,
                previous,
            },
            &bar.icons,
            bar.palette(),
            now,
            move |message| Message::Acted(notice_island, instance, message),
            move |focused| Message::ReplyFocused(instance, key, focused),
            dismiss,
            Message::Unfold(instance, key),
        );
        ((instance, key), element)
    });
    // The cards that wait: how many notices they hold, and whose popover lists them.
    let footer = waiting.first().map(|card| {
        let count = waiting.iter().map(Vec::len).sum();
        kit::more_notices(
            count,
            Message::MoreNotices(bar.toasts[card[0]].instance),
            bar.palette(),
        )
    });
    island::Popover {
        key: NOTICES,
        content: kit::notices(shown, footer, bar.palette()),
        open: panel.open,
        on_placed: Box::new(Message::NoticesPlaced),
        on_closed: Message::NoticesClosed,
        on_hover: Some(Box::new(Message::NoticesHovered)),
        width: style::PANEL_WIDTH,
        placement: Placement::Below,
        on_key: None,
        drop: None,
        drop_color: None,
        on_neck: None,
        sag: None,
        home: None,
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
    if bar.config.theme == config::Theme::System {
        subscriptions.push(Subscription::run(portal::color_scheme).map(Message::ColorScheme));
    }
    // Pointer, click and key tracking only while a popover is open, or the bar shows on hover:
    // otherwise every hover over the bar would wake us.
    if bar.is_open() || bar.typing() || bar.showing() == Showing::Hover {
        subscriptions.push(iced::event::listen_with(popover_event));
    }
    // Always: the dormer's keybind comes this way, and what may change whether a window covers
    // the screen. Every other event stops in the filter.
    subscriptions.push(Subscription::run(hyprland_events).filter_map(hyprland));
    // Notifications say how long ago they came: a minute at a time, while they show.
    let timed = bar
        .toasts
        .iter()
        .any(|toast| !toast.leaving && toast.notice.time.is_some());
    if timed && bar.notices.as_ref().is_some_and(|panel| panel.open) {
        subscriptions.push(iced::time::every(Duration::from_secs(60)).map(|_| Message::Minute));
    }
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
        Event::Mouse(mouse::Event::CursorMoved { position })
            if position.y < style::REVEAL_EDGE as f32 =>
        {
            Some(Message::PointerAtTop)
        }
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

/// Our own events: the clicks Hyprland reports while a popover is open, the dormer's keybind,
/// and `eave popover` and `eave action`.
/// And the ones that may change whether a window covers the screen: a check on the way in, and
/// whenever windows come, go or move.
fn hyprland(update: eave_hyprland::Update<()>) -> Option<Message> {
    use eave_hyprland::Event;
    let event = match update {
        eave_hyprland::Update::Snapshot(()) => return Some(Message::CheckFullscreen),
        eave_hyprland::Update::Event(event) => event,
    };
    match event {
        Event::Custom(data) => match data.as_str() {
            eave_hyprland::CLICK => Some(Message::Clicked),
            eave_hyprland::DORMER => Some(Message::Dormer(dormer::Event::Toggle)),
            data => {
                let mut words = data.split_whitespace();
                match (words.next()?, words.next(), words.next(), words.next()) {
                    (eave_hyprland::POPOVER, Some(widget), None, _) => {
                        Some(Message::TogglePopover(widget.into()))
                    }
                    (eave_hyprland::ACTION, Some(widget), Some(id), None) => {
                        Some(Message::RunAction(widget.into(), id.into()))
                    }
                    _ => None,
                }
            }
        },
        Event::Workspace { .. } | Event::FocusedMonitor { .. } => Some(Message::WorkspaceChanged),
        Event::Fullscreen(_)
        | Event::OpenWindow { .. }
        | Event::CloseWindow { .. }
        | Event::MoveWindow { .. } => Some(Message::CheckFullscreen),
        _ => None,
    }
}

/// Whether a window covers the focused screen, fullscreen: one on its workspace, and not just
/// maximized, which keeps clear of the bar. The bar opens on the focused screen (one surface for
/// now).
async fn covered() -> bool {
    use eave_hyprland::types::Client;
    let Some(monitor) = notch::focused().await else {
        return false;
    };
    let clients = match eave_hyprland::query::<Vec<Client>>("clients").await {
        Ok(clients) => clients,
        Err(error) => {
            eprintln!("eave: can't tell whether a window is fullscreen: {error}");
            return false;
        }
    };
    let workspace = monitor.active_workspace.id;
    clients
        .iter()
        .any(|client| client.workspace.id == workspace && client.covers())
}

/// Surfaces are transparent: the islands draw their own backgrounds, so they can have rounded
/// corners and the wallpaper shows around them.
pub fn style(_: &Bar, theme: &Theme) -> theme::Style {
    theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: Palette::of(theme).normal,
    }
}

/// The theme every surface draws with: light or dark, as `theme` in config.toml says (D-92).
/// Widgets that draw themselves find the palette through it.
pub fn theme(bar: &Bar, _: window::Id) -> Theme {
    bar.scheme().theme()
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use eave_api::ui::label;
    use eave_api::{Effect, Widget};

    use super::*;

    #[test]
    fn custom_events_name_a_popover_or_an_action() {
        let event = |data: &str| {
            hyprland(eave_hyprland::Update::Event(eave_hyprland::Event::Custom(
                data.into(),
            )))
        };
        assert!(matches!(
            event("eave-popover claude"),
            Some(Message::TogglePopover(name)) if name == "claude"
        ));
        assert!(matches!(
            event("eave-action volume mute"),
            Some(Message::RunAction(name, id)) if name == "volume" && id == "mute"
        ));
        assert!(event("eave-popover").is_none());
        assert!(event("eave-popover a b").is_none());
        assert!(event("eave-action volume").is_none());
        assert!(event("eave-action volume mute now").is_none());
        assert!(event("someone-else claude").is_none());
    }

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
    fn a_solid_bar_ends_where_the_islands_do() {
        let screen = notch::Screen::default();
        let bottom = band_bottom(&screen);
        assert_eq!(
            bottom,
            f32::from(style::EDGE_GAP) + style::ISLAND_HEIGHT as f32
        );
        // Above what the bar keeps windows clear of: they never go under it.
        assert!(bottom <= screen.bar_height() as f32);
        let grown = notch::Screen {
            grow: 4.0,
            ..screen
        };
        assert_eq!(band_bottom(&grown), bottom + 4.0);
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
    /// the one being answered. Keys from 100 are in one thread, from 200 critical; and each may
    /// be on a question of several.
    struct Notifying(Vec<u64>, Option<u64>, Vec<(u64, usize)>);

    #[derive(Debug, Clone, PartialEq)]
    enum Note {
        Add(u64),
        Drop(u64),
        Expired(u64),
        Answer(u64),
        Typed(String),
        Cancel,
        Step(u64, usize),
    }

    impl Widget for Notifying {
        type Config = toml::Table;
        type Message = Note;

        fn new(_: Self::Config) -> (Self, eave_api::Task<Note>) {
            (Self(Vec::new(), None, Vec::new()), eave_api::Task::none())
        }

        fn update(&mut self, message: Note) -> Effect<Note> {
            match message {
                Note::Add(key) => self.0.insert(0, key),
                Note::Drop(key) | Note::Expired(key) => self.0.retain(|shown| *shown != key),
                Note::Answer(key) => self.1 = Some(key),
                Note::Typed(_) => {}
                Note::Cancel => self.1 = None,
                Note::Step(key, step) => {
                    self.2.retain(|(at, _)| *at != key);
                    self.2.push((key, step));
                }
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
                    let mut notice = Notice::new(key, format!("notice {key}"))
                        .app("app")
                        .expires(Duration::from_secs(5), Note::Expired(key));
                    if (100..200).contains(&key) {
                        notice = notice.thread("chat");
                    }
                    if key >= 200 {
                        notice = notice.urgency(Urgency::Critical);
                    }
                    if let Some((_, step)) = self.2.iter().find(|(at, _)| *at == key) {
                        notice = notice.step(*step, 3, None);
                    }
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

    /// `notifying_bar`, with `extra` settings.
    fn notifying_bar_with(extra: &str) -> Bar {
        let mut registry = Registry::new();
        registry.widget::<Notifying>("n");
        registry.widget::<WithPopover>("p");
        let (mut bar, _) = Bar::new(
            registry,
            config(&format!(
                "{extra}\n[[right]]\nwidget = \"n\"\n[[right]]\nwidget = \"p\""
            )),
        );
        let _ = update(&mut bar, Message::WindowOpened(window::Id::unique()));
        bar
    }

    fn fullscreen(bar: &mut Bar, covered: bool) {
        let _ = update(bar, Message::Fullscreen(bar.checks, covered));
    }

    #[test]
    fn in_fullscreen_only_critical_notices_show_and_the_rest_wait_for_it_to_end() {
        let mut bar = notifying_bar_with("");
        fullscreen(&mut bar, true);
        tell(&mut bar, Note::Add(1));
        assert_eq!(panel(&bar), None, "nothing to show yet");
        tell(&mut bar, Note::Add(200));
        assert_eq!(panel(&bar), Some(true));
        assert_eq!(cards(&bar), [vec![200]]);

        // Placed and shown: only the critical one's time runs.
        let start = Instant::now();
        let _ = handle(&mut bar, Message::NoticesPlaced(placed()));
        let _ = bar.run_clocks(start);
        let deadlines: Vec<_> = bar.toasts.iter().map(|toast| toast.deadline).collect();
        assert_eq!(deadlines, [Some(start + Duration::from_secs(5)), None]);

        // The critical one goes: the panel folds back, and the other one still waits.
        tell(&mut bar, Note::Drop(200));
        let _ = update(&mut bar, Message::NoticesClosed);
        assert_eq!(panel(&bar), None);
        assert_eq!(keys(&bar), [(1, false)]);

        fullscreen(&mut bar, false);
        assert_eq!(panel(&bar), Some(true));
        assert_eq!(cards(&bar), [vec![1]]);
        assert_eq!(
            bar.toasts[0].left,
            Some(Duration::from_secs(5)),
            "all its time left"
        );
    }

    #[test]
    fn notices_can_all_show_or_all_wait_in_fullscreen() {
        let mut bar = notifying_bar_with("notifications = { fullscreen = \"show\" }");
        fullscreen(&mut bar, true);
        tell(&mut bar, Note::Add(1));
        assert_eq!(panel(&bar), Some(true));

        let mut bar = notifying_bar_with("notifications = { fullscreen = \"hold\" }");
        fullscreen(&mut bar, true);
        tell(&mut bar, Note::Add(200));
        assert_eq!(panel(&bar), None);
        // Going fullscreen with one on screen: it folds back, and comes back after.
        fullscreen(&mut bar, false);
        assert_eq!(panel(&bar), Some(true));
        fullscreen(&mut bar, true);
        assert_eq!(panel(&bar), Some(false));
    }

    #[test]
    fn a_bar_on_hover_comes_out_while_pointed_at_and_hides_a_moment_after() {
        let mut bar = notifying_bar_with("bar = { show = \"hover\" }");
        assert!(!bar.out());
        // Onto the surface, at its top strip.
        let _ = update(&mut bar, Message::PointerEntered);
        let _ = update(&mut bar, Message::PointerAtTop);
        assert!(bar.out());
        let _ = update(&mut bar, Message::PointerLeft);
        assert!(bar.out(), "not right away");
        // Back before the timer: that timer counts for nothing.
        let left = bar.leaves;
        let _ = update(&mut bar, Message::PointerEntered);
        let _ = update(&mut bar, Message::Unhover(left));
        assert!(bar.out());
        let _ = update(&mut bar, Message::PointerLeft);
        let left = bar.leaves;
        let _ = update(&mut bar, Message::Unhover(left));
        assert!(!bar.out());
        assert_eq!(
            bar.layer,
            Layer::Top,
            "on top of windows, not fullscreen ones"
        );
    }

    #[test]
    fn a_hidden_bar_peeks_on_a_workspace_switch_and_a_nudge_but_only_on_hover() {
        let mut bar = notifying_bar_with("bar = { show = \"hover\" }");
        let _ = update(&mut bar, Message::WorkspaceChanged);
        assert!(bar.out());
        let peek = bar.peek.unwrap();
        // Another one keeps it out longer: the first one's time is up for nothing.
        let _ = update(&mut bar, Message::WorkspaceChanged);
        let _ = update(&mut bar, Message::Unpeek(peek));
        assert!(bar.out());
        let peek = bar.peek.unwrap();
        let _ = update(&mut bar, Message::Unpeek(peek));
        assert!(!bar.out());

        let mut bar = notifying_bar_with("bar = { show = \"hover\", peek = false }");
        let _ = update(&mut bar, Message::WorkspaceChanged);
        assert!(!bar.out());

        // Hidden on purpose over a fullscreen window: no peeking.
        let mut bar = notifying_bar_with("");
        fullscreen(&mut bar, true);
        let _ = update(&mut bar, Message::WorkspaceChanged);
        assert!(!bar.out());
    }

    #[test]
    fn a_hidden_bar_leaves_the_notch_and_edge_out_and_only_the_top_strip_brings_it_out() {
        let mut bar = notifying_bar_with("bar = { show = \"hover\" }");
        let window = bar.windows[0];
        bar.screen.grow = 3.0;
        bar.screen.notch = Some(Notch {
            width: 200.0,
            height: 30.0,
            shoulder: 5.0,
            ..Notch::default()
        });
        assert!(!bar.out());
        // Not laid out yet: nowhere to say.
        assert!(bar.bezel_rects(window).is_empty());
        let _ = update(&mut bar, Message::Width(window, 1000.0));
        let rects = bar.bezel_rects(window);
        assert!(rects.contains(&Rectangle::new(Point::ORIGIN, Size::new(1000.0, 3.0))));
        assert!(rects.contains(&Rectangle::new(
            Point::new(395.0, 0.0),
            Size::new(210.0, 33.0)
        )));
        // Onto the notch: the bar stays hidden.
        let _ = update(&mut bar, Message::PointerEntered);
        assert!(!bar.out());
        // Up to the top strip: it comes out, and coming back onto it keeps it out.
        let _ = update(&mut bar, Message::PointerAtTop);
        assert!(bar.out());
        let _ = update(&mut bar, Message::PointerEntered);
        assert!(bar.out());
        // Over a fullscreen window, they go with the bar.
        fullscreen(&mut bar, true);
        assert!(bar.bezel_rects(window).is_empty());
    }

    #[test]
    fn over_a_fullscreen_window_the_bar_goes_on_the_overlay_only_while_it_shows() {
        let mut bar = notifying_bar_with("");
        assert!(bar.out());
        fullscreen(&mut bar, true);
        assert!(!bar.out());
        assert_eq!(bar.layer, Layer::Top);

        // A critical notice brings it out, over the window.
        tell(&mut bar, Note::Add(200));
        assert!(bar.out());
        assert_eq!(bar.layer, Layer::Overlay);

        // Gone and folded back: it slides away, then goes back under.
        tell(&mut bar, Note::Drop(200));
        let _ = update(&mut bar, Message::NoticesClosed);
        assert!(!bar.out());
        assert_eq!(bar.layer, Layer::Overlay, "still sliding away");
        let sink = bar.sinking.unwrap();
        let _ = update(&mut bar, Message::Sink(sink));
        assert_eq!(bar.layer, Layer::Top);

        // Shown on hover over it, it stays on the overlay to be hovered.
        let mut bar = notifying_bar_with("bar = { fullscreen = \"hover\" }");
        fullscreen(&mut bar, true);
        assert!(!bar.out());
        assert_eq!(bar.layer, Layer::Overlay);
        let mut bar = notifying_bar_with("bar = { fullscreen = \"show\" }");
        fullscreen(&mut bar, true);
        assert!(bar.out());
        assert_eq!(bar.layer, Layer::Overlay);
        fullscreen(&mut bar, false);
        assert_eq!(bar.layer, Layer::Top);
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

    /// The cards, as the keys of their notices.
    fn cards(bar: &Bar) -> Vec<Vec<u64>> {
        bar.cards()
            .into_iter()
            .map(|card| card.into_iter().map(|i| bar.toasts[i].notice.key).collect())
            .collect()
    }

    #[test]
    fn notices_of_a_thread_that_come_one_after_another_share_a_card() {
        let mut bar = notifying_bar();
        for key in [101, 102, 1, 103] {
            tell(&mut bar, Note::Add(key));
        }
        assert_eq!(cards(&bar), [vec![103], vec![1], vec![102, 101]]);
        tell(&mut bar, Note::Add(104));
        assert_eq!(cards(&bar), [vec![104, 103], vec![1], vec![102, 101]]);
    }

    #[test]
    fn critical_notices_come_first() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Add(201));
        tell(&mut bar, Note::Add(1));
        assert_eq!(cards(&bar), [vec![201], vec![1]]);
    }

    #[test]
    fn past_three_cards_the_rest_wait_with_their_clocks_stopped() {
        let mut bar = notifying_bar();
        for key in 1..=4 {
            tell(&mut bar, Note::Add(key));
        }
        let start = Instant::now();
        let _ = handle(&mut bar, Message::NoticesPlaced(placed()));
        let _ = bar.run_clocks(start);
        let running: Vec<_> = bar
            .toasts
            .iter()
            .map(|toast| (toast.notice.key, toast.deadline.is_some()))
            .collect();
        assert_eq!(running, [(4, true), (3, true), (2, true), (1, false)]);

        // One shown goes: the one waiting comes in, and its clock starts.
        tell(&mut bar, Note::Drop(4));
        let _ = bar.run_clocks(start);
        assert!(bar.toasts.iter().all(|toast| toast.deadline.is_some()));
    }

    #[test]
    fn a_card_of_several_expires_together_on_its_newest_s_time() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Add(101));
        tell(&mut bar, Note::Add(102));
        let start = Instant::now();
        let _ = handle(&mut bar, Message::NoticesPlaced(placed()));
        let _ = bar.run_clocks(start);
        assert!(bar.toasts[0].deadline.is_some());
        assert_eq!(bar.toasts[1].deadline, None, "the earlier one goes with it");
        let expired: Vec<Note> = bar
            .expire(start + Duration::from_secs(5))
            .into_iter()
            .map(|(_, message)| message.downcast().unwrap())
            .collect();
        assert_eq!(expired, [Note::Expired(102), Note::Expired(101)]);
    }

    #[test]
    fn moving_on_to_another_question_keeps_the_one_before_to_slide_away() {
        let mut bar = notifying_bar();
        tell(&mut bar, Note::Step(1, 0));
        tell(&mut bar, Note::Add(1));
        assert!(bar.toasts[0].previous.is_none());
        tell(&mut bar, Note::Step(1, 1));
        let (before, _) = bar.toasts[0].previous.as_ref().expect("kept");
        assert_eq!(before.step.as_ref().map(|step| step.index), Some(0));
        assert_eq!(
            bar.toasts[0].notice.step.as_ref().map(|step| step.index),
            Some(1)
        );
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
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
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
    fn the_notch_pinches_as_the_dormer_leaves_and_flattens_as_it_lands() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        assert_eq!(bar.notch_kick, None, "not before the drop leaves");
        dormer(&mut bar, dormer::Event::Neck(false));
        assert_eq!(bar.notch_kick, Some((1, bezel::Kick::Pinch)));
        let _ = update(&mut bar, Message::KeyPressed);
        dormer(&mut bar, dormer::Event::Neck(true));
        assert_eq!(bar.notch_kick, Some((2, bezel::Kick::Flatten)));
    }

    #[test]
    fn the_dormer_never_covers_the_bar() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(-50.0, 1000.0));
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
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        assert!(ASKED.get() > 0);
        assert!(rows(&bar).contains(&"Mute".to_string()));
    }

    fn tell_acting(bar: &mut Bar, message: Act) {
        let _ = update(bar, Message::Widget(0, AnyMessage::new(message)));
    }

    #[test]
    fn typing_narrows_the_list_and_arrows_pick_a_row() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
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
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        dormer(&mut bar, dormer::Event::Query("mute".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(!bar.is_open());

        // The widget got it: its action follows its state now, and comes first as a recent pick.
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        assert_eq!(rows(&bar)[0], "Unmute");
        // The list follows the widget while it's open, too.
        tell_acting(&mut bar, Act::Mute);
        assert!(rows(&bar).contains(&"Mute".to_string()));
    }

    #[test]
    fn an_action_or_a_row_can_open_a_popover() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        dormer(&mut bar, dormer::Event::Query("show".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(bar.popover_of(0).is_some());
        assert!(!bar.dormer_open());

        let _ = update(&mut bar, Message::BarPressed);
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        dormer(&mut bar, dormer::Event::Query("acting".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(bar.popover_of(0).is_some());
    }

    #[test]
    fn the_dormer_and_popovers_take_turns() {
        let (mut bar, window) = acting_bar();
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        assert!(bar.popover_of(0).is_none() && bar.dormer_open());
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        assert!(bar.popover_of(0).is_some() && !bar.dormer_open());
    }

    #[test]
    fn a_reload_takes_a_gone_widgets_rows_with_it() {
        let (mut bar, _) = acting_bar();
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
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
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        assert_eq!(rows(&bar), ["Mute", "Show it"]);
        dormer(&mut bar, dormer::Event::Run(1));
        assert!(!bar.is_open(), "no island to grow a popover out of");
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Talk {
        Open,
        Close,
        Quiet,
        Opened,
        Closed,
    }

    #[derive(Default)]
    struct Talking {
        quiet: bool,
    }

    impl Widget for Talking {
        type Config = toml::Table;
        type Message = Talk;

        fn new(_: Self::Config) -> (Self, eave_api::Task<Talk>) {
            (Self::default(), eave_api::Task::none())
        }

        fn update(&mut self, message: Talk) -> Effect<Talk> {
            match message {
                Talk::Open => return Effect::none().open_dialog(),
                Talk::Close => return Effect::none().close_dialog(),
                Talk::Quiet => self.quiet = true,
                Talk::Opened | Talk::Closed => {}
            }
            Effect::none()
        }

        fn item(&self) -> eave_api::ui::Item<Talk> {
            eave_api::ui::Item::new(label("talking"))
        }

        fn popover(&self) -> Option<eave_api::ui::Panel<Talk>> {
            Some(eave_api::ui::Panel::new(label("sessions")))
        }

        fn dialog(&self) -> Option<eave_api::ui::Panel<Talk>> {
            (!self.quiet).then(|| {
                eave_api::ui::Panel::new(label("chat"))
                    .on_open(Talk::Opened)
                    .on_close(Talk::Closed)
            })
        }

        fn actions(&self) -> Vec<Action<Talk>> {
            vec![ui::action("Talk", Talk::Open)]
        }
    }

    /// A bar with a `Talking` (0) on the left and a `Dummy` (1) on the right, on a surface.
    fn talking_bar() -> (Bar, window::Id) {
        let mut registry = Registry::new();
        registry.widget::<Talking>("talking");
        registry.widget::<Dummy>("d");
        let (mut bar, _) = Bar::new(
            registry,
            config("[[left]]\nwidget = \"talking\"\n[[right]]\nwidget = \"d\""),
        );
        let window = window::Id::unique();
        let _ = update(&mut bar, Message::WindowOpened(window));
        (bar, window)
    }

    fn talk(bar: &mut Bar, message: Talk) {
        let _ = update(bar, Message::Widget(0, AnyMessage::new(message)));
    }

    fn heard(hooks: Hooks) -> Vec<(u64, Talk)> {
        hooks
            .into_iter()
            .map(|(id, message)| (id, message.downcast().unwrap()))
            .collect()
    }

    const LEFT: IslandId = IslandId {
        zone: Zone::Left,
        index: 0,
    };

    #[test]
    fn a_dialog_asked_for_from_a_popover_comes_out_of_it_and_leaves_its_island_on_the_bar() {
        let (mut bar, window) = talking_bar();
        let _ = update(&mut bar, Message::Touched(window, 0));
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        assert!(bar.popover_of(0).is_some());
        let open = AnyMessage::new(Talk::Open);
        let _ = update(&mut bar, Message::Acted(LEFT, 0, open));
        assert_eq!(bar.asked, Some((0, Origin::Island(LEFT))));
        assert!(bar.dialog.is_none(), "not before its top is known");

        let _ = update(&mut bar, Message::Dialog(dialog::Event::Opened(1000.0)));
        let dialog = bar.dialog.as_ref().unwrap();
        assert!(dialog.open && dialog.instance == 0);
        assert_eq!(dialog.origin, Origin::Island(LEFT));
        assert!(
            bar.popovers.is_empty(),
            "the popover became the dialog, with nothing to fold back: the island is back"
        );
        // It comes out of the island's shape as drawn, and goes back into it.
        let home = dialog_popover(&bar, dialog).home.unwrap();
        assert!(Rc::ptr_eq(&home, &bar.shape(LEFT)));
        assert!(bar.is_open() && bar.dialog_of(0));
        assert_eq!(bar.keyboard, Some(window));
        assert_eq!(heard(bar.dialog_opened(0)), [(0, Talk::Opened)]);
        // No drop, so no neck: the notch stays put.
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Neck(false)));
        let _ = update(&mut bar, Message::KeyPressed);
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Closed));
        assert!(bar.dialog.is_none() && bar.popovers.is_empty());
    }

    #[test]
    fn a_dialog_asked_for_out_of_the_blue_or_long_after_a_touch_comes_out_of_the_top() {
        let (mut bar, window) = talking_bar();
        talk(&mut bar, Talk::Open);
        assert_eq!(bar.asked, Some((0, Origin::Top)));

        let (mut bar, _) = talking_bar();
        let _ = update(&mut bar, Message::Touched(window, 0));
        bar.touched = bar.touched.map(|touched| Touched {
            at: touched.at - 2 * dialog::ORIGIN_WINDOW,
            ..touched
        });
        talk(&mut bar, Talk::Open);
        assert_eq!(bar.asked, Some((0, Origin::Top)));
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Opened(1000.0)));
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Neck(false)));
        assert_eq!(bar.notch_kick, Some((1, bezel::Kick::Pinch)));
    }

    #[test]
    fn a_dialog_asked_for_from_a_notification_comes_out_of_its_island() {
        let (mut bar, _) = talking_bar();
        let island = bar.notice_island();
        let _ = update(&mut bar, Message::Acted(island, 1, AnyMessage::new(())));
        talk(&mut bar, Talk::Open);
        assert_eq!(bar.asked, Some((0, Origin::Island(island))));
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Opened(1000.0)));
        assert!(bar.dialog.is_some());
        assert!(
            !bar.toasts_held(),
            "the island is back, and so are its notifications"
        );
    }

    #[test]
    fn the_dormer_turns_into_a_dialog_picked_in_it() {
        let (mut bar, _) = talking_bar();
        dormer(&mut bar, dormer::Event::Opened(240.0, 1000.0));
        dormer(&mut bar, dormer::Event::Query("talk".into()));
        dormer(&mut bar, dormer::Event::Run(0));
        assert!(bar.dormer.is_none() && bar.asked.is_none());
        let dialog = bar.dialog.as_ref().unwrap();
        assert!(dialog.open);
        assert_eq!((dialog.room, dialog.origin), (1000.0, Origin::Top));
    }

    #[test]
    fn a_dialog_closes_on_a_key_a_click_or_when_asked_and_stays_until_it_is_back() {
        for close in [
            Message::KeyPressed,
            Message::BarPressed,
            Message::Widget(0, AnyMessage::new(Talk::Close)),
            Message::Widget(0, AnyMessage::new(Talk::Quiet)),
        ] {
            let (mut bar, _) = talking_bar();
            talk(&mut bar, Talk::Open);
            let _ = update(&mut bar, Message::Dialog(dialog::Event::Opened(1000.0)));
            let _ = update(&mut bar, Message::Dialog(dialog::Event::Placed(placed())));
            assert_eq!(bar.dialog.as_ref().unwrap().rect, Some(placed()));
            let _ = update(&mut bar, close);
            assert!(!bar.is_open() && bar.dialog.is_some());
            assert_eq!(bar.keyboard, None);
            let _ = update(&mut bar, Message::Dialog(dialog::Event::Closed));
            assert!(bar.dialog.is_none());
        }
    }

    #[test]
    fn a_dialog_takes_turns_with_popovers_and_the_dormer() {
        let (mut bar, window) = talking_bar();
        talk(&mut bar, Talk::Open);
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Opened(1000.0)));
        assert!(bar.dialog_open());
        let _ = update(&mut bar, Message::Pressed(window, 0, None));
        assert!(bar.popover_of(0).is_some() && !bar.dialog_open());

        talk(&mut bar, Talk::Open);
        assert!(bar.dialog_open(), "one going back turns around");
        dormer(&mut bar, dormer::Event::Opened(200.0, 1000.0));
        assert!(bar.dormer_open() && bar.dialog.is_none());
    }

    #[test]
    fn a_reload_that_drops_the_instance_drops_its_dialog() {
        let (mut bar, _) = talking_bar();
        talk(&mut bar, Talk::Open);
        let _ = update(&mut bar, Message::Dialog(dialog::Event::Opened(1000.0)));
        let _ = bar.apply(config("[[right]]\nwidget = \"d\""));
        assert!(bar.dialog.is_none());
    }
}
