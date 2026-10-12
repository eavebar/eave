//! The host: holds the widget instances, routes their messages, and owns the bar surfaces. A
//! popover isn't a surface of its own: the island holding its item morphs into it, and the bar
//! surface grows to make room while it's there.

use eave_api::erased::{AnyMessage, ErasedWidget, PopoverRequest};
use eave_api::{Nudge, Registry};
use iced::alignment::Horizontal;
use iced::widget::{Space, container, row};
use iced::{
    Color, Element, Event, Length, Padding, Rectangle, Subscription, Task, Theme, keyboard, mouse,
    theme, window,
};
use iced_layershell::actions::ActionCallback;
use iced_layershell::reexport::KeyboardInteractivity;
use iced_layershell::to_layer_message;

use crate::apps::{self, AppIcon, Icons};
use crate::config::{self, Config, Entry, Zone};
use crate::island::{self, Island};
use crate::slot::Slot;
use crate::{kit, theme as style, watch};

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
    /// App icons loaded so far, by app id, for every widget to share.
    icons: Icons,
    /// App ids whose icons are being loaded.
    loading: Vec<String>,
}

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
    nudge: Option<(u64, Nudge)>,
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
    /// Hyprland saw a primary click somewhere ([`eave_hyprland::report_clicks`]).
    Clicked,
    PointerEntered,
    PointerLeft,
    /// A key was pressed while a popover had the keyboard.
    KeyPressed,
    /// An island placed the popover it's turning into.
    PopoverPlaced(u64, Rectangle),
    /// An island finished turning back from a popover.
    PopoverClosed(u64),
    /// A bar surface opened: it takes the pointer only over its strip.
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    /// `config.toml` changed on disk: the new contents, or why they couldn't be loaded.
    ConfigChanged(Result<Config, String>),
    /// An app's icon has loaded, or turned out not to exist.
    AppIcon(String, Option<AppIcon>),
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
            icons: Icons::new(),
            loading: Vec::new(),
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
        // The icon theme may have changed since: look every icon up again.
        self.icons.clear();
        self.loading.clear();
        let mut old = std::mem::take(&mut self.instances);
        let mut tasks = Vec::new();

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
                    });
                }
                Err(error) => eprintln!("eave: widget `{}`: {error}, skipping it", entry.widget),
            }
        }

        // A popover whose instance is gone has no island left to morph back into.
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.popovers)
            .into_iter()
            .partition(|popover| self.find(popover.instance).is_some());
        self.popovers = kept;
        tasks.extend(gone.iter().map(|popover| self.fit(popover.window)));
        self.config = config;
        Task::batch(tasks)
    }

    /// Starts loading the app icons that items and the open popover show and nobody has asked
    /// for yet.
    fn load_icons(&mut self) -> Vec<Task<Message>> {
        let mut wanted = Vec::new();
        for instance in &self.instances {
            if let Some(content) = instance.widget.item().into_parts().content {
                kit::app_ids(content, &mut wanted);
            }
            if self.popover_of(instance.id).is_some()
                && let Some(panel) = instance.widget.popover()
            {
                kit::app_ids(panel.into_node(), &mut wanted);
            }
        }
        let mut tasks = Vec::new();
        for app_id in wanted {
            if self.icons.contains_key(&app_id) || self.loading.contains(&app_id) {
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
        if !instance.has_popover {
            return Hooks::new();
        }
        let Some(window) = instance.window else {
            eprintln!(
                "eave: a widget asked for its popover before its item was ever clicked; not opening it"
            );
            return Hooks::new();
        };
        let zone = instance.entry.zone;

        let mut hooks = self.close_popover();
        // Popovers open from a click on the bar, so that's where the pointer is.
        self.pointer_inside = true;
        // An island turns into one popover at a time: one still closing in it gives way.
        let instances = &self.instances;
        self.popovers.retain(|popover| {
            let same_island = popover.window == window
                && instances
                    .iter()
                    .any(|other| other.id == popover.instance && other.entry.zone == zone);
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
        self.popovers.iter().any(|popover| popover.open)
    }

    fn popover_of(&self, id: u64) -> Option<&Popover> {
        self.popovers
            .iter()
            .find(|popover| popover.instance == id && popover.open)
    }

    /// Sizes a bar surface to fit its popovers, and lets the pointer through everywhere else: the
    /// bar itself takes input across its whole strip, a popover only over its shape.
    fn fit(&self, window: window::Id) -> Task<Message> {
        let rects: Vec<Rectangle> = self
            .popovers
            .iter()
            .filter(|popover| popover.window == window)
            .filter_map(|popover| popover.rect)
            .collect();
        let height = rects
            .iter()
            .map(|rect| (rect.y + rect.height).ceil() as u32 + u32::from(style::EDGE_GAP))
            .fold(style::SURFACE_HEIGHT, u32::max);
        let callback = ActionCallback::new(move |region| {
            // iced_layershell reuses one region and only clears the surface's current size, which
            // may be the old one: clear all of it, so no earlier popover's rect is left behind.
            region.subtract(0, 0, i32::MAX, i32::MAX);
            region.add(0, 0, i32::MAX, style::BAR_HEIGHT as i32);
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
}

pub fn update(bar: &mut Bar, message: Message) -> Task<Message> {
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
    tasks.extend(hand_keyboard(bar));
    tasks.extend(bar.load_icons());
    Task::batch(tasks)
}

/// Gives the keyboard to the bar surface with the open popover, and takes it back from the one
/// that had it. Exclusive, not on demand: with `follow_mouse`, hovering a window would take the
/// focus away ([D-36](../../../docs/decisions.md)).
fn hand_keyboard(bar: &mut Bar) -> Vec<Task<Message>> {
    let wanted = bar
        .popovers
        .iter()
        .find(|popover| popover.open)
        .map(|popover| popover.window);
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
            instance.has_popover = instance.widget.popover().is_some();
            let has_popover = instance.has_popover;
            if let Some(nudge) = nudge {
                bar.nudges += 1;
                let generation = bar.nudges;
                if let Some(instance) = bar.find_mut(id) {
                    instance.nudge = Some((generation, nudge));
                }
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
            bar.icons.insert(app_id, icon);
            Task::none()
        }
        Message::InnerPressed(id) if bar.popover_of(id).is_none() => send(bar.close_popover()),
        // Clicks on our own surfaces are handled where they land.
        Message::BarPressed => send(bar.close_popover()),
        Message::Clicked if !bar.pointer_inside => send(bar.close_popover()),
        Message::PointerEntered => {
            bar.pointer_inside = true;
            Task::none()
        }
        Message::PointerLeft => {
            bar.pointer_inside = false;
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
        Message::WindowOpened(window) => bar.fit(window),
        Message::WindowClosed(window) => {
            bar.popovers.retain(|popover| popover.window != window);
            if bar.keyboard == Some(window) {
                bar.keyboard = None;
            }
            Task::none()
        }
        Message::ConfigChanged(config) => match config {
            // Editors often fire several events per save; only a real change rebuilds anything.
            Ok(config) if config == bar.config => Task::none(),
            Ok(config) => bar.apply(config),
            Err(error) => {
                eprintln!("eave: config: {error}; keeping the current bar");
                Task::none()
            }
        },
        // Clicks that aren't outside, and the layer-shell variants added by `to_layer_message`.
        _ => Task::none(),
    }
}

/// Every surface is a bar.
pub fn view(bar: &Bar, window: window::Id) -> Element<'_, Message> {
    // Each zone is an island as wide as its items. A zone with nothing to show draws nothing, but
    // still takes its share of the width, so the center island stays centered on screen.
    let zone = |zone: Zone, align: Horizontal| {
        let instances = || {
            bar.instances
                .iter()
                .filter(move |instance| instance.entry.zone == zone)
        };
        let (items, groups): (Vec<_>, Vec<_>) = instances()
            .filter_map(|instance| {
                let item = (instance.id, item_view(instance, window, &bar.icons)?);
                Some((item, instance.entry.width_group.as_deref()))
            })
            .unzip();
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

        let island: Element<'_, Message> = if items.is_empty() && popover.is_none() {
            Space::new().into()
        } else {
            Island::new(items)
                .align(align)
                .popover(popover.map(|popover| popover_view(bar, popover)))
                .nudges(nudges)
                .width_groups(groups)
                .into()
        };
        container(island).width(Length::Fill).align_x(align)
    };

    // No background of its own: the surface is transparent around and between the islands, and
    // below them, where a dragged island has room to move and popovers grow.
    container(row![
        zone(Zone::Left, Horizontal::Left),
        zone(Zone::Center, Horizontal::Center),
        zone(Zone::Right, Horizontal::Right),
    ])
    .padding(Padding {
        top: style::EDGE_GAP.into(),
        right: style::EDGE_GAP.into(),
        bottom: style::DRAG_ROOM.into(),
        left: style::EDGE_GAP.into(),
    })
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
    let content: Element<'_, AnyMessage> =
        match bar.find(id).and_then(|instance| instance.widget.popover()) {
            Some(panel) => kit::panel(panel.into_node(), &bar.icons),
            None => Space::new().into(),
        };
    island::Popover {
        content: content.map(move |message| Message::Widget(id, message)),
        open: popover.open,
        on_placed: Box::new(move |rect| Message::PopoverPlaced(id, rect)),
        on_closed: Message::PopoverClosed(id),
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
    if bar.is_open() {
        subscriptions.push(iced::event::listen_with(popover_event));
        subscriptions.push(Subscription::run(hyprland_events).filter_map(clicked));
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

fn clicked(update: eave_hyprland::Update<()>) -> Option<Message> {
    match update {
        eave_hyprland::Update::Event(eave_hyprland::Event::Custom(data))
            if data == eave_hyprland::CLICK =>
        {
            Some(Message::Clicked)
        }
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
}
