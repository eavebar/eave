//! The host: holds the widget instances, routes their messages, and owns every surface (the bar
//! and the popover).

use std::time::Duration;

use eave_api::Registry;
use eave_api::erased::{AnyMessage, ErasedWidget, PopoverRequest};
use iced::alignment::Horizontal;
use iced::widget::{Space, container, row, text};
use iced::{
    Color, Element, Event, Length, Padding, Rectangle, Size, Subscription, Task, Theme, mouse,
    theme, window,
};
use iced_layershell::reexport::{
    Anchor as Edge, KeyboardInteractivity, Layer, NewLayerShellSettings,
};
use iced_layershell::to_layer_message;

use crate::config::{Config, Entry, Zone};
use crate::island::Island;
use crate::measure::Measure;
use crate::slot::Slot;
use crate::{kit, theme as style, watch};

/// How long the pointer may be outside an open popover before it closes.
const LEAVE_GRACE: Duration = Duration::from_millis(400);

pub struct Bar {
    registry: Registry,
    /// The config the instances were built from, to skip reloads that change nothing.
    config: Config,
    instances: Vec<Instance>,
    /// Instances are addressed by id, not position, so a reload can't route a stale message to
    /// whichever widget now sits at the old index.
    next_id: u64,
    popover: Option<Popover>,
    /// Pointer-leave timers carry this, so re-entering cancels a pending close.
    leave_generation: u64,
}

struct Instance {
    id: u64,
    /// What the instance was built from. An unchanged entry keeps its instance across reloads.
    entry: Entry,
    widget: Box<dyn ErasedWidget>,
    /// Cached after every update, so drawing the bar never builds a popover just to ask.
    has_popover: bool,
    /// Where the item was when last clicked, to place its popover.
    anchor: Option<Anchor>,
}

#[derive(Debug, Clone, Copy)]
struct Anchor {
    bounds: Rectangle,
    bar_width: f32,
}

struct Popover {
    /// The id of the instance it belongs to.
    instance: u64,
    window: window::Id,
    /// Known once the panel has been measured. Until then the surface is 1×1 and nothing is drawn.
    size: Option<Size>,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    Widget(u64, AnyMessage),
    /// A primary click on an item: its bounds, the bar width, and the item's own `on_press`.
    Pressed(u64, Rectangle, f32, Option<AnyMessage>),
    PopoverMeasured(Size),
    PointerEntered(window::Id),
    PointerLeft(window::Id),
    LeaveTimeout(u64),
    WindowClosed(window::Id),
    /// `config.toml` changed on disk: the new contents, or why they couldn't be loaded.
    ConfigChanged(Result<Config, String>),
}

impl Bar {
    pub fn new(registry: Registry, config: Config) -> (Self, Task<Message>) {
        let mut bar = Self {
            registry,
            config: Config::default(),
            instances: Vec::new(),
            next_id: 0,
            popover: None,
            leave_generation: 0,
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
        let mut old = std::mem::take(&mut self.instances);
        let mut tasks = Vec::new();

        for entry in &config.entries {
            if let Some(position) = old.iter().position(|instance| instance.entry == *entry) {
                self.instances.push(old.remove(position));
                continue;
            }
            let Some(construct) = self.registry.get(&entry.widget) else {
                eprintln!(
                    "eave: no plugin provides widget `{}`, skipping it",
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
                        anchor: None,
                    });
                }
                Err(error) => eprintln!("eave: widget `{}`: {error}, skipping it", entry.widget),
            }
        }

        if let Some(popover) = &self.popover
            && self.find(popover.instance).is_none()
        {
            tasks.push(self.close_popover());
        }
        self.config = config;
        Task::batch(tasks)
    }

    fn find(&self, id: u64) -> Option<&Instance> {
        self.instances.iter().find(|instance| instance.id == id)
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Instance> {
        self.instances.iter_mut().find(|instance| instance.id == id)
    }

    fn open_popover(&mut self, id: u64) -> Task<Message> {
        let Some(instance) = self.find(id) else {
            return Task::none();
        };
        if !instance.has_popover {
            return Task::none();
        }
        let Some(anchor) = instance.anchor else {
            eprintln!(
                "eave: a widget asked for its popover before its item was ever clicked; not opening it"
            );
            return Task::none();
        };

        let close = self.close_popover();
        let window = window::Id::unique();
        self.popover = Some(Popover {
            instance: id,
            window,
            size: None,
        });
        // Start at 1×1 and invisible; `PopoverMeasured` gives the real size and position.
        let open = Task::done(Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some((1, 1)),
                layer: Layer::Overlay,
                anchor: Edge::Top | Edge::Left,
                // Zero exclusive zone: the compositor places us below the bar, not over it.
                exclusive_zone: None,
                // Top, right, bottom, left (wlr-layer-shell order, despite the macro's doc comment).
                margin: Some((popover_top(), 0, 0, anchor.bounds.x as i32)),
                keyboard_interactivity: KeyboardInteractivity::None,
                namespace: Some("eave-popover".into()),
                ..NewLayerShellSettings::default()
            },
            id: window,
        });
        Task::batch([close, open])
    }

    fn close_popover(&mut self) -> Task<Message> {
        match self.popover.take() {
            Some(popover) => Task::done(Message::RemoveWindow(popover.window)),
            None => Task::none(),
        }
    }

    fn popover_of(&self, id: u64) -> Option<&Popover> {
        self.popover
            .as_ref()
            .filter(|popover| popover.instance == id)
    }
}

pub fn update(bar: &mut Bar, message: Message) -> Task<Message> {
    match message {
        Message::Widget(id, message) => {
            // Gone after a reload: its leftover messages have nowhere to go.
            let Some(instance) = bar.find_mut(id) else {
                return Task::none();
            };
            let (task, request) = instance.widget.update(message).into_parts();
            instance.has_popover = instance.widget.popover().is_some();
            let has_popover = instance.has_popover;
            let task = task.map(move |message| Message::Widget(id, message));

            let popover = match request {
                Some(PopoverRequest::Open) => bar.open_popover(id),
                Some(PopoverRequest::Close) if bar.popover_of(id).is_some() => bar.close_popover(),
                // A widget that stops offering a popover closes it.
                _ if bar.popover_of(id).is_some() && !has_popover => bar.close_popover(),
                _ => Task::none(),
            };
            Task::batch([task, popover])
        }
        Message::Pressed(id, bounds, bar_width, on_press) => {
            let Some(instance) = bar.find_mut(id) else {
                return Task::none();
            };
            instance.anchor = Some(Anchor { bounds, bar_width });
            match on_press {
                Some(message) => update(bar, Message::Widget(id, message)),
                None if bar.popover_of(id).is_some() => bar.close_popover(),
                None => bar.open_popover(id),
            }
        }
        Message::PopoverMeasured(size) => {
            let Some(anchor) = bar
                .popover
                .as_ref()
                .and_then(|popover| bar.find(popover.instance)?.anchor)
            else {
                return Task::none();
            };
            let Some(popover) = &mut bar.popover else {
                return Task::none();
            };
            popover.size = Some(size);
            let id = popover.window;
            let x = popover_x(anchor, size.width);
            let size = (size.width.ceil() as u32, size.height.ceil() as u32);
            Task::batch([
                Task::done(Message::SizeChange { id, size }),
                Task::done(Message::MarginChange {
                    id,
                    margin: (popover_top(), 0, 0, x),
                }),
            ])
        }
        Message::PointerEntered(window) => {
            if bar
                .popover
                .as_ref()
                .is_some_and(|popover| popover.window == window)
            {
                bar.leave_generation += 1;
            }
            Task::none()
        }
        Message::PointerLeft(window) => {
            if !bar
                .popover
                .as_ref()
                .is_some_and(|popover| popover.window == window)
            {
                return Task::none();
            }
            bar.leave_generation += 1;
            let generation = bar.leave_generation;
            Task::perform(tokio::time::sleep(LEAVE_GRACE), move |_| {
                Message::LeaveTimeout(generation)
            })
        }
        Message::LeaveTimeout(generation) if generation == bar.leave_generation => {
            bar.close_popover()
        }
        Message::WindowClosed(window) => {
            if bar
                .popover
                .as_ref()
                .is_some_and(|popover| popover.window == window)
            {
                bar.popover = None;
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
        // Stale timers, and the layer-shell variants added by `to_layer_message`.
        _ => Task::none(),
    }
}

/// The popover's top margin. The compositor already puts it below the bar's exclusive zone, which
/// ends `DRAG_ROOM` below the islands, so this keeps the visible gap at `PANEL_GAP`.
fn popover_top() -> i32 {
    (style::PANEL_GAP - i32::from(style::DRAG_ROOM)).max(0)
}

/// Centered under the item, kept on screen.
fn popover_x(anchor: Anchor, width: f32) -> i32 {
    let gap = style::PANEL_GAP as f32;
    let centered = anchor.bounds.center_x() - width / 2.0;
    let max = (anchor.bar_width - width - gap).max(gap);
    centered.clamp(gap, max).round() as i32
}

pub fn view(bar: &Bar, window: window::Id) -> Element<'_, Message> {
    match &bar.popover {
        Some(popover) if popover.window == window => popover_view(bar, popover),
        // Every other surface is a bar.
        _ => bar_view(bar),
    }
}

fn bar_view(bar: &Bar) -> Element<'_, Message> {
    // Each zone is an island as wide as its items. A zone with nothing to show draws nothing, but
    // still takes its share of the width, so the center island stays centered on screen.
    let zone = |zone: Zone, align: Horizontal| {
        let items: Vec<_> = bar
            .instances
            .iter()
            .filter(|instance| instance.entry.zone == zone)
            .filter_map(item_view)
            .collect();
        let island: Element<'_, Message> = if items.is_empty() {
            Space::new().into()
        } else {
            Island::new(
                container(row(items).spacing(style::ITEM_SPACING).height(Length::Fill))
                    .height(Length::Fixed(style::ISLAND_HEIGHT as f32))
                    .padding([0, style::ISLAND_PADDING]),
            )
            .into()
        };
        container(island).width(Length::Fill).align_x(align)
    };

    // No background of its own: the surface is transparent around and between the islands, and
    // below them, where a dragged island has room to move.
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

fn item_view(instance: &Instance) -> Option<Element<'_, Message>> {
    let id = instance.id;
    let parts = instance.widget.item().into_parts();
    let on_press = parts.on_press;
    let content: Element<'_, AnyMessage> = container(kit::node(parts.content?))
        .padding([0, style::ITEM_PADDING])
        .center_y(Length::Fill)
        .into();

    let wrap = move |message| Message::Widget(id, message);
    let mut slot = Slot::new(content.map(wrap))
        .on_secondary(parts.on_secondary.map(wrap))
        .on_scroll(parts.on_scroll.map(|(up, down)| (wrap(up), wrap(down))));
    if on_press.is_some() || instance.has_popover {
        slot = slot
            .on_press(move |bounds, width| Message::Pressed(id, bounds, width, on_press.clone()));
    }
    Some(slot.into())
}

fn popover_view<'a>(bar: &'a Bar, popover: &Popover) -> Element<'a, Message> {
    let id = popover.instance;
    let content = match bar.find(id).and_then(|instance| instance.widget.popover()) {
        Some(panel) => kit::node(panel.into_node()),
        None => text("").into(),
    };
    let panel: Element<'_, AnyMessage> = container(content)
        .padding(style::PANEL_PADDING)
        .style(kit::panel_style)
        .into();

    Measure::new(
        panel.map(move |message| Message::Widget(id, message)),
        Message::PopoverMeasured,
        popover.size.is_some(),
    )
    .into()
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
    subscriptions.push(window::close_events().map(Message::WindowClosed));
    subscriptions.push(watch::config_changes().map(Message::ConfigChanged));
    // Pointer tracking only while a popover is open: otherwise every hover over the bar would wake us.
    if bar.popover.is_some() {
        subscriptions.push(iced::event::listen_with(pointer_event));
    }
    Subscription::batch(subscriptions)
}

fn pointer_event(event: Event, _: iced::event::Status, window: window::Id) -> Option<Message> {
    match event {
        Event::Mouse(mouse::Event::CursorEntered) => Some(Message::PointerEntered(window)),
        Event::Mouse(mouse::Event::CursorLeft) => Some(Message::PointerLeft(window)),
        _ => None,
    }
}

/// Surfaces are transparent: the bar and the popover panel draw their own backgrounds, so a
/// popover can have rounded corners.
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

    fn anchor(x: f32, width: f32) -> Anchor {
        Anchor {
            bounds: Rectangle::new(iced::Point::new(x, 0.0), Size::new(width, 32.0)),
            bar_width: 1000.0,
        }
    }

    #[test]
    fn popover_centers_under_item() {
        assert_eq!(popover_x(anchor(450.0, 100.0), 200.0), 400);
    }

    #[test]
    fn popover_stays_on_screen() {
        let gap = style::PANEL_GAP;
        assert_eq!(popover_x(anchor(0.0, 40.0), 200.0), gap);
        assert_eq!(popover_x(anchor(960.0, 40.0), 200.0), 1000 - 200 - gap);
        // Wider than the bar: pinned to the left edge.
        assert_eq!(popover_x(anchor(500.0, 40.0), 2000.0), gap);
    }
}
