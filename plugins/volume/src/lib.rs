//! The default output's volume. Scroll to change it, right click to mute. The popover has fine
//! control and the list of outputs to switch between.

use eave_api::ui::{self, Item, Panel, Tone};
use eave_api::{Effect, Registry, Subscription, Task, Widget};
use eave_pulse::{Outputs, Sink};
use serde::Deserialize;

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
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Outputs),
    Up,
    Down,
    ToggleMute,
    Choose(usize),
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
        };
        (volume, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Read(outputs) => self.outputs = outputs,
            Message::Up => return self.change(self.config.step as i64),
            Message::Down => return self.change(-(self.config.step as i64)),
            Message::ToggleMute => {
                let Some(sink) = self.sink_mut() else {
                    return Effect::none();
                };
                sink.muted = !sink.muted;
                let (sink, mute) = (sink.clone(), sink.muted);
                return run(async move { eave_pulse::set_mute(&sink, mute).await });
            }
            Message::Choose(index) => {
                let Some(sink) = self.outputs.sinks.get(index).cloned() else {
                    return Effect::none();
                };
                self.outputs.default = Some(index);
                return run(async move { eave_pulse::set_default(&sink).await });
            }
        }
        Effect::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(eave_pulse::watch).map(Message::Read)
    }

    fn item(&self) -> Item<Message> {
        let Some(sink) = self.outputs.default_sink() else {
            return Item::hidden();
        };
        let content = if sink.muted {
            ui::label("Muted").tone(Tone::Dim)
        } else {
            ui::row([
                ui::label("Vol").tone(Tone::Dim),
                ui::label(format!("{}%", sink.volume)),
            ])
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
        run(async move { eave_pulse::set_volume(&sink, target).await })
    }
}

fn run(work: impl Future<Output = std::io::Result<()>> + Send + 'static) -> Effect<Message> {
    Task::future(async move {
        if let Err(error) = work.await {
            eprintln!("eave-volume: {error}");
        }
    })
    .discard()
    .into()
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
    fn mute_shows_at_once() {
        let mut volume = volume_at(50);
        volume.update(Message::ToggleMute);
        assert!(volume.outputs.default_sink().unwrap().muted);
    }
}
