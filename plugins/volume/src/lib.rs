//! The default output's volume, as a speaker with up to three sound waves, struck through when
//! muted. Scroll to change it, right click to mute. The popover has the level, fine control and the
//! list of outputs to switch between. Whenever the level goes up or down, from here or anywhere
//! else, the island gets a nudge that way, and the item shows the percent for a moment instead of
//! the icon. It shows the percent while hovered, too.

use eave_api::time::{Duration, sleep};
use eave_api::ui::{self, Icon, Item, Panel, Tone};
use eave_api::{Effect, Nudge, Registry, Subscription, Task, Widget};
use eave_pulse::{Outputs, Sink};
use serde::Deserialize;

/// How long the percent shows after the last change.
const LEVEL_SHOWN: Duration = Duration::from_millis(1500);

pub fn register(registry: &mut Registry) {
    registry.widget::<Volume>("volume");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Percent per scroll step or button press.
    step: u32,
    /// Highest percent eave will set. Above 100 amplifies, and can distort.
    max: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self { step: 5, max: 100 }
    }
}

pub struct Volume {
    config: Config,
    outputs: Outputs,
    /// The default sink and its level as the server last reported them, which our own changes
    /// only show ahead of. A change in the level is what nudges.
    heard: Option<(u32, u32)>,
    /// Whether the item shows the percent instead of the icon, after a change.
    showing_level: bool,
    /// Counts changes, so only the latest one's timer puts the icon back.
    changes: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Outputs),
    Up,
    Down,
    ToggleMute,
    Choose(usize),
    /// The percent has shown long enough after change number this.
    HideLevel(u64),
}

impl Widget for Volume {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let volume = Self {
            config: Config {
                step: config.step.max(1),
                max: config.max.max(1),
            },
            outputs: Outputs::default(),
            heard: None,
            showing_level: false,
            changes: 0,
        };
        (volume, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Read(outputs) => self.read(outputs),
            Message::Up => self.change(self.config.step as i64),
            Message::Down => self.change(-(self.config.step as i64)),
            Message::ToggleMute => {
                let Some(sink) = self.sink_mut() else {
                    return Effect::none();
                };
                sink.muted = !sink.muted;
                let (sink, mute) = (sink.clone(), sink.muted);
                run(async move { eave_pulse::set_mute(&sink, mute).await }).into()
            }
            Message::Choose(index) => {
                let Some(sink) = self.outputs.sinks.get(index).cloned() else {
                    return Effect::none();
                };
                self.outputs.default = Some(index);
                run(async move { eave_pulse::set_default(&sink).await }).into()
            }
            Message::HideLevel(change) => {
                if change == self.changes {
                    self.showing_level = false;
                }
                Effect::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(eave_pulse::watch).map(Message::Read)
    }

    fn item(&self) -> Item<Message> {
        let Some(sink) = self.outputs.default_sink() else {
            return Item::hidden();
        };
        let icon = ui::icon(Icon::Volume {
            level: sink.volume as f32 / 100.0,
            muted: sink.muted,
        });
        let max = self.config.max.max(sink.volume);
        let level = ui::counter(sink.volume.into(), 0..=max.into(), "%");
        let content = ui::alternate([icon, level], self.showing_level.into()).on_hover(1);
        let content = if sink.muted {
            content.tone(Tone::Dim)
        } else {
            content
        };
        Item::new(content)
            .on_scroll(Message::Up, Message::Down)
            .on_secondary(Message::ToggleMute)
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let sink = self.outputs.default_sink()?;
        let level = if sink.muted {
            "Muted".to_string()
        } else {
            format!("{}%", sink.volume)
        };
        let mut lines = vec![
            ui::heading(format!("Volume {level}")),
            ui::row([
                ui::button(ui::label("−"), Message::Down),
                ui::button(
                    ui::label(if sink.muted { "Unmute" } else { "Mute" }),
                    Message::ToggleMute,
                ),
                ui::button(ui::label("+"), Message::Up),
            ]),
        ];
        if self.outputs.sinks.len() > 1 {
            lines.push(ui::separator());
            lines.push(ui::label("Output").tone(Tone::Dim));
            lines.extend(self.outputs.sinks.iter().enumerate().map(|(index, sink)| {
                let label = ui::label(sink.description.clone());
                let label = if Some(index) == self.outputs.default {
                    label.tone(Tone::Accent)
                } else {
                    label
                };
                ui::button(label, Message::Choose(index))
            }));
        }
        Some(Panel::new(ui::column(lines)))
    }
}

impl Volume {
    fn sink_mut(&mut self) -> Option<&mut Sink> {
        let index = self.outputs.default?;
        self.outputs.sinks.get_mut(index)
    }

    /// Takes in the server's reading. A level that went up or down on the same sink nudges that
    /// way. Our own changes come back here too, one reading per step, so each nudges once.
    fn read(&mut self, outputs: Outputs) -> Effect<Message> {
        let heard = outputs.default_sink().map(|sink| (sink.index, sink.volume));
        let before = std::mem::replace(&mut self.heard, heard);
        self.outputs = outputs;
        let nudge = match (before, heard) {
            (Some((sink, from)), Some((same, to))) if sink == same && to > from => Nudge::Up,
            (Some((sink, from)), Some((same, to))) if sink == same && to < from => Nudge::Down,
            _ => return Effect::none(),
        };
        Effect::task(self.show_level()).nudge(nudge)
    }

    /// Shows the percent instead of the icon, until a moment after the last change.
    fn show_level(&mut self) -> Task<Message> {
        self.showing_level = true;
        self.changes += 1;
        let change = self.changes;
        Task::future(sleep(LEVEL_SHOWN)).map(move |()| Message::HideLevel(change))
    }

    /// Changes the default sink's volume by `delta` percent. The new level shows at once; the
    /// server's own reading follows and confirms it. Steps land on multiples of the step size.
    fn change(&mut self, delta: i64) -> Effect<Message> {
        let (step, max) = (i64::from(self.config.step), i64::from(self.config.max));
        let Some(sink) = self.sink_mut() else {
            return Effect::none();
        };
        let current = i64::from(sink.volume);
        let snapped = if delta > 0 {
            (current / step + 1) * step
        } else {
            (current + step - 1) / step * step - step
        };
        // Already above the cap (set elsewhere): scrolling up doesn't pull it down.
        let target = snapped.clamp(0, max.max(current)) as u32;
        if target == sink.volume {
            return Effect::none();
        }
        sink.volume = target;
        let sink = sink.clone();
        let set = run(async move { eave_pulse::set_volume(&sink, target).await });
        Effect::task(Task::batch([set, self.show_level()]))
    }
}

fn run(work: impl Future<Output = std::io::Result<()>> + Send + 'static) -> Task<Message> {
    Task::future(async move {
        if let Err(error) = work.await {
            eprintln!("eave-volume: {error}");
        }
    })
    .discard()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume_at(percent: u32) -> Volume {
        let (mut volume, _) = Volume::new(Config::default());
        volume.outputs = test_outputs(percent);
        volume
    }

    fn test_outputs(percent: u32) -> Outputs {
        let mut outputs = Outputs::default();
        outputs.sinks.push(eave_pulse::test_sink(percent));
        outputs.default = Some(0);
        outputs
    }

    fn level(volume: &Volume) -> u32 {
        volume.outputs.default_sink().unwrap().volume
    }

    #[test]
    fn steps_snap_to_the_grid() {
        let mut volume = volume_at(43);
        volume.update(Message::Up);
        assert_eq!(level(&volume), 45);
        volume.update(Message::Up);
        assert_eq!(level(&volume), 50);

        let mut volume = volume_at(43);
        volume.update(Message::Down);
        assert_eq!(level(&volume), 40);
        let mut volume = volume_at(45);
        volume.update(Message::Down);
        assert_eq!(level(&volume), 40);
    }

    #[test]
    fn respects_the_cap_and_zero() {
        let mut volume = volume_at(100);
        volume.update(Message::Up);
        assert_eq!(level(&volume), 100);
        let mut volume = volume_at(130);
        volume.update(Message::Up);
        assert_eq!(level(&volume), 130);
        volume.update(Message::Down);
        assert_eq!(level(&volume), 125);
        let mut volume = volume_at(0);
        volume.update(Message::Down);
        assert_eq!(level(&volume), 0);
    }

    #[test]
    fn shows_an_icon() {
        let content = volume_at(50).item().into_parts().content.unwrap();
        let ui::Kind::Alternate {
            mut options,
            current: 0,
            hovered: Some(1),
            grows: false,
        } = content.into_kind()
        else {
            panic!("the icon first");
        };
        assert!(matches!(
            options.remove(0).into_kind(),
            ui::Kind::Icon {
                icon: Icon::Volume { muted: false, .. },
                ..
            }
        ));
    }

    fn nudge(effect: Effect<Message>) -> Option<Nudge> {
        effect.into_parts().2
    }

    #[test]
    fn readings_that_change_the_level_nudge() {
        let (mut volume, _) = Volume::new(Config::default());
        // The first reading is where it starts, not a change.
        assert_eq!(nudge(volume.update(Message::Read(test_outputs(40)))), None);
        // Scrolling shows the new level at once, and nudges when the server confirms it.
        assert_eq!(nudge(volume.update(Message::Up)), None);
        assert_eq!(
            nudge(volume.update(Message::Read(test_outputs(45)))),
            Some(Nudge::Up)
        );
        assert_eq!(nudge(volume.update(Message::Read(test_outputs(45)))), None);
        assert_eq!(
            nudge(volume.update(Message::Read(test_outputs(30)))),
            Some(Nudge::Down)
        );
    }

    fn shows_label(volume: &Volume) -> bool {
        let content = volume.item().into_parts().content.unwrap();
        matches!(content.into_kind(), ui::Kind::Alternate { current: 1, .. })
    }

    #[test]
    fn a_change_shows_the_percent_for_a_moment() {
        let (mut volume, _) = Volume::new(Config::default());
        volume.update(Message::Read(test_outputs(40)));
        assert!(!shows_label(&volume), "the icon, at first");
        volume.update(Message::Read(test_outputs(45)));
        assert!(shows_label(&volume));
        // Another change before the first one's time is up keeps it showing.
        volume.update(Message::Up);
        volume.update(Message::HideLevel(1));
        assert!(shows_label(&volume));
        volume.update(Message::HideLevel(volume.changes));
        assert!(!shows_label(&volume));
    }

    #[test]
    fn mute_shows_at_once() {
        let mut volume = volume_at(50);
        volume.update(Message::ToggleMute);
        assert!(volume.outputs.default_sink().unwrap().muted);
    }
}
