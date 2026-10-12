//! The host: holds the widget instances, routes their messages, and owns every surface (the bar
//! and the popover).

use std::time::Duration;

use eave_api::Registry;
use eave_api::erased::{AnyMessage, ErasedWidget, PopoverRequest};
use iced::alignment::Horizontal;
use iced::widget::{container, row, text};
use iced::{
    Color, Element, Event, Length, Rectangle, Size, Subscription, Task, Theme, mouse, theme, window,
};
use iced_layershell::reexport::{
    Anchor as Edge, KeyboardInteractivity, Layer, NewLayerShellSettings,
};
use iced_layershell::to_layer_message;

use crate::config::{Entry, Zone};
use crate::measure::Measure;
use crate::slot::Slot;
use crate::{kit, theme as style};

/// How long the pointer may be outside an open popover before it closes.
const LEAVE_GRACE: Duration = Duration::from_millis(400);

pub struct Bar {
    instances: Vec<Instance>,
    popover: Option<Popover>,
    /// Pointer-leave timers carry this, so re-entering cancels a pending close.
    leave_generation: u64,
}

struct Instance {
    zone: Zone,
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
    instance: usize,
    window: window::Id,
    /// Known once the panel has been measured. Until then the surface is 1×1 and nothing is drawn.
    size: Option<Size>,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    Widget(usize, AnyMessage),
    /// A primary click on an item: its bounds, the bar width, and the item's own `on_press`.
    Pressed(usize, Rectangle, f32, Option<AnyMessage>),
    PopoverMeasured(Size),
    PointerEntered(window::Id),
    PointerLeft(window::Id),
    LeaveTimeout(u64),
    WindowClosed(window::Id),
}

impl Bar {
    pub fn new(registry: &Registry, entries: &[Entry]) -> (Self, Task<Message>) {
        let mut instances = Vec::new();
        let mut tasks = Vec::new();

        for entry in entries {
            let Some(construct) = registry.get(&entry.widget) else {
                eprintln!(
                    "eave: no plugin provides widget `{}`, skipping it",
                    entry.widget
                );
                continue;
            };
            match construct(entry.options.clone()) {
                Ok((widget, task)) => {
                    let index = instances.len();
                    tasks.push(task.map(move |message| Message::Widget(index, message)));
                    instances.push(Instance {
                        zone: entry.zone,
                        has_popover: widget.popover().is_some(),
                        widget,
                        anchor: None,
                    });
                }
                Err(error) => eprintln!("eave: widget `{}`: {error}, skipping it", entry.widget),
            }
        }

        let bar = Self {
            instances,
            popover: None,
            leave_generation: 0,
        };
        (bar, Task::batch(tasks))
    }

    fn open_popover(&mut self, index: usize) -> Task<Message> {
        let instance = &self.instances[index];
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
            instance: index,
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
                margin: Some((style::PANEL_GAP, 0, 0, anchor.bounds.x as i32)),
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

    fn popover_of(&self, index: usize) -> Option<&Popover> {
        self.popover
            .as_ref()
            .filter(|popover| popover.instance == index)
    }
}

pub fn update(bar: &mut Bar, message: Message) -> Task<Message> {
    match message {
        Message::Widget(index, message) => {
            let Some(instance) = bar.instances.get_mut(index) else {
                return Task::none();
            };
            let (task, request) = instance.widget.update(message).into_parts();
            instance.has_popover = instance.widget.popover().is_some();
            let task = task.map(move |message| Message::Widget(index, message));

            let popover = match request {
                Some(PopoverRequest::Open) => bar.open_popover(index),
                Some(PopoverRequest::Close) if bar.popover_of(index).is_some() => {
                    bar.close_popover()
                }
                // A widget that stops offering a popover closes it.
                _ if bar.popover_of(index).is_some() && !bar.instances[index].has_popover => {
                    bar.close_popover()
                }
                _ => Task::none(),
            };
            Task::batch([task, popover])
        }
        Message::Pressed(index, bounds, bar_width, on_press) => {
            let Some(instance) = bar.instances.get_mut(index) else {
                return Task::none();
            };
            instance.anchor = Some(Anchor { bounds, bar_width });
            match on_press {
                Some(message) => update(bar, Message::Widget(index, message)),
                None if bar.popover_of(index).is_some() => bar.close_popover(),
                None => bar.open_popover(index),
            }
        }
        Message::PopoverMeasured(size) => {
            let Some(popover) = &mut bar.popover else {
                return Task::none();
            };
            let Some(anchor) = bar.instances[popover.instance].anchor else {
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
                    margin: (style::PANEL_GAP, 0, 0, x),
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
        // Stale timers, and the layer-shell variants added by `to_layer_message`.
        _ => Task::none(),
    }
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
    let zone = |zone: Zone, align: Horizontal| {
        let items = bar
            .instances
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.zone == zone)
            .filter_map(|(index, instance)| item_view(index, instance));
        container(row(items).spacing(style::ITEM_SPACING).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(align)
    };

    container(
        row![
            zone(Zone::Left, Horizontal::Left),
            zone(Zone::Center, Horizontal::Center),
            zone(Zone::Right, Horizontal::Right),
        ]
        .padding([0, style::BAR_PADDING]),
    )
    .style(kit::bar_style)
    .into()
}

fn item_view(index: usize, instance: &Instance) -> Option<Element<'_, Message>> {
    let (content, on_press, on_secondary) = instance.widget.item().into_parts();
    let content: Element<'_, AnyMessage> = container(kit::node(content?))
        .padding([0, style::ITEM_PADDING])
        .center_y(Length::Fill)
        .into();

    let mut slot = Slot::new(content.map(move |message| Message::Widget(index, message)))
        .on_secondary(on_secondary.map(|message| Message::Widget(index, message)));
    if on_press.is_some() || instance.has_popover {
        slot = slot.on_press(move |bounds, width| {
            Message::Pressed(index, bounds, width, on_press.clone())
        });
    }
    Some(slot.into())
}

fn popover_view<'a>(bar: &'a Bar, popover: &Popover) -> Element<'a, Message> {
    let index = popover.instance;
    let content = match bar.instances[index].widget.popover() {
        Some(panel) => kit::node(panel.into_node()),
        None => text("").into(),
    };
    let panel: Element<'_, AnyMessage> = container(content)
        .padding(style::PANEL_PADDING)
        .style(kit::panel_style)
        .into();

    Measure::new(
        panel.map(move |message| Message::Widget(index, message)),
        Message::PopoverMeasured,
        popover.size.is_some(),
    )
    .into()
}

pub fn subscription(bar: &Bar) -> Subscription<Message> {
    let widgets = bar.instances.iter().enumerate().map(|(index, instance)| {
        instance
            .widget
            .subscription()
            .with(index)
            .map(|(index, message)| Message::Widget(index, message))
    });
    let mut subscriptions: Vec<_> = widgets.collect();
    subscriptions.push(window::close_events().map(Message::WindowClosed));
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
    use super::*;

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
