//! The default output's volume, as a speaker with up to three sound waves, struck through when
//! muted. Scroll to change it, right click to mute. The popover has a slider for the output and
//! the input, the devices to switch between, and a slider for each app playing sound. Whenever the level goes up or down, from here or anywhere
//! else, the island gets a nudge that way, and the item shows the percent for a moment instead of
//! the icon. It shows the percent while hovered, too.

use eave_api::time::{Duration, sleep};
use eave_api::ui::{self, Action, Icon, Item, Panel, Tone};
use eave_api::{Effect, Nudge, Registry, Subscription, Task, Widget};
use eave_pulse::{Audio, Device, Direction, Form, Playback};
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
    audio: Audio,
    /// Apps playing sound, watched only while the popover is open.
    streams: Vec<Playback>,
    open: bool,
    /// Levels being sent: one request at a time per slider, and the latest level dragged to
    /// while it was out, so a drag never queues up requests.
    sending: Vec<(Target, Option<u32>)>,
    /// The default sink and its level as the server last reported them, which our own changes
    /// only show ahead of. A change in the level is what nudges.
    heard: Option<(u32, u32)>,
    /// Whether the item shows the percent instead of the icon, after a change.
    showing_level: bool,
    /// Counts changes, so only the latest one's timer puts the icon back.
    changes: u64,
}

/// What a slider sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Output,
    Input,
    /// An app's stream, by index.
    Stream(u32),
}

#[derive(Debug, Clone)]
pub enum Message {
    Read(Audio),
    Streams(Vec<Playback>),
    Up,
    Down,
    ToggleMute,
    ToggleMic,
    MuteStream(u32),
    /// A slider moved.
    Set(Target, u32),
    /// The level last sent for this target got there.
    Sent(Target),
    Choose(Direction, usize),
    Opened,
    Closed,
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
            audio: Audio::default(),
            streams: Vec::new(),
            open: false,
            sending: Vec::new(),
            heard: None,
            showing_level: false,
            changes: 0,
        };
        (volume, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Read(audio) => self.read(audio),
            Message::Streams(mut streams) => {
                for stream in &mut streams {
                    let dragged = self
                        .sending
                        .iter()
                        .any(|(t, _)| *t == Target::Stream(stream.index));
                    let ours = self.streams.iter().find(|s| s.index == stream.index);
                    if let (true, Some(ours)) = (dragged, ours) {
                        stream.volume = ours.volume;
                    }
                }
                self.streams = streams;
                Effect::none()
            }
            Message::Up => self.change(self.config.step as i64),
            Message::Down => self.change(-(self.config.step as i64)),
            Message::ToggleMute => self.toggle(Direction::Output),
            Message::ToggleMic => self.toggle(Direction::Input),
            Message::MuteStream(index) => {
                let Some(stream) = self.streams.iter_mut().find(|s| s.index == index) else {
                    return Effect::none();
                };
                stream.muted = !stream.muted;
                let (stream, mute) = (stream.clone(), stream.muted);
                run(async move { eave_pulse::set_stream_mute(&stream, mute).await }).into()
            }
            Message::Set(target, level) => {
                match target {
                    Target::Output | Target::Input => {
                        let direction = direction(target);
                        let Some(device) = self.device_mut(direction) else {
                            return Effect::none();
                        };
                        device.volume = level;
                    }
                    Target::Stream(index) => {
                        let Some(stream) = self.streams.iter_mut().find(|s| s.index == index)
                        else {
                            return Effect::none();
                        };
                        stream.volume = level;
                    }
                }
                self.send(target, level).into()
            }
            Message::Sent(target) => {
                let Some(at) = self.sending.iter().position(|(t, _)| *t == target) else {
                    return Effect::none();
                };
                match self.sending[at].1.take() {
                    Some(level) => self.set(target, level).into(),
                    None => {
                        self.sending.remove(at);
                        Effect::none()
                    }
                }
            }
            Message::Choose(direction, index) => {
                let (devices, default) = match direction {
                    Direction::Output => (&self.audio.sinks, &mut self.audio.default_sink),
                    Direction::Input => (&self.audio.sources, &mut self.audio.default_source),
                };
                let Some(device) = devices.get(index).cloned() else {
                    return Effect::none();
                };
                *default = Some(index);
                run(async move { eave_pulse::set_default(&device).await }).into()
            }
            Message::Opened => {
                self.open = true;
                Effect::none()
            }
            Message::Closed => {
                self.open = false;
                self.streams.clear();
                Effect::none()
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
        let devices = Subscription::run(eave_pulse::watch).map(Message::Read);
        if !self.open {
            return devices;
        }
        let streams = Subscription::run(eave_pulse::watch_streams).map(Message::Streams);
        Subscription::batch([devices, streams])
    }

    fn item(&self) -> Item<Message> {
        let Some(sink) = self.audio.default_sink() else {
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
        let sink = self.audio.default_sink()?;
        let mut pages = vec![("Output", self.group(Direction::Output, sink))];
        if let Some(source) = self.audio.default_source() {
            pages.push(("Input", self.group(Direction::Input, source)));
        }
        let apps = if self.streams.is_empty() {
            ui::label("Nothing is playing").tone(Tone::Dim)
        } else {
            ui::column(self.streams.iter().map(|stream| self.app(stream)))
        };
        pages.push(("Apps", apps));
        let panel = Panel::new(ui::column([ui::header("Sound"), ui::tabs(pages)]))
            .on_open(Message::Opened)
            .on_close(Message::Closed);
        Some(panel)
    }

    fn actions(&self) -> Vec<Action<Message>> {
        let Some(sink) = self.audio.default_sink() else {
            return Vec::new();
        };
        let sound = ["sound", "volume", "audio"];
        let speaker = |level: u32, muted| {
            ui::icon(Icon::Volume {
                level: level as f32 / 100.0,
                muted,
            })
        };
        let mute = if sink.muted { "Unmute" } else { "Mute" };
        let mut actions = vec![
            ui::action(mute, Message::ToggleMute)
                .id("mute")
                .subtitle(sink.description.clone())
                .keywords(sound.into_iter().chain(["silence", "quiet"]))
                .leading(speaker(sink.volume, !sink.muted)),
            ui::action("Volume up", Message::Up)
                .id("up")
                .subtitle(format!("{}%", sink.volume))
                .keywords(sound.into_iter().chain(["louder", "raise"]))
                .leading(speaker(100, false)),
            ui::action("Volume down", Message::Down)
                .id("down")
                .subtitle(format!("{}%", sink.volume))
                .keywords(sound.into_iter().chain(["quieter", "lower"]))
                .leading(speaker(30, false)),
        ];
        if let Some(source) = self.audio.default_source() {
            let mute = if source.muted {
                "Unmute microphone"
            } else {
                "Mute microphone"
            };
            actions.push(
                ui::action(mute, Message::ToggleMic)
                    .id("mic-mute")
                    .subtitle(source.description.clone())
                    .keywords(["mic", "input"])
                    .leading(ui::icon(Icon::Mic {
                        muted: !source.muted,
                    })),
            );
        }
        // The devices to switch to: the ones not in use.
        for (direction, devices, default, kind) in [
            (
                Direction::Output,
                &self.audio.sinks,
                self.audio.default_sink,
                "Sound output",
            ),
            (
                Direction::Input,
                &self.audio.sources,
                self.audio.default_source,
                "Sound input",
            ),
        ] {
            for (index, device) in devices.iter().enumerate() {
                if Some(index) == default {
                    continue;
                }
                actions.push(
                    ui::action(
                        format!("Switch to {}", device.description),
                        Message::Choose(direction, index),
                    )
                    .subtitle(kind)
                    .keywords(sound.into_iter().chain(device.connection))
                    .leading(ui::icon(form_icon(device.form))),
                );
            }
        }
        actions
    }
}

/// Which device a slider target sets.
fn direction(target: Target) -> Direction {
    match target {
        Target::Input => Direction::Input,
        _ => Direction::Output,
    }
}

fn form_icon(form: Form) -> Icon {
    match form {
        Form::Speaker => Icon::Speaker,
        Form::Headphones | Form::Headset => Icon::Headphones,
        Form::Display => Icon::Display,
        Form::Microphone => Icon::Mic { muted: false },
    }
}

impl Volume {
    /// The output's or input's page: its mute button, slider and level, the `output` or `input`
    /// bay about the device in use (D-94), then the devices to choose from when there's more than
    /// one, or the only one's name.
    fn group(&self, direction: Direction, device: &Device) -> ui::Node<Message> {
        let (devices, default, target, toggle) = match direction {
            Direction::Output => (
                &self.audio.sinks,
                self.audio.default_sink,
                Target::Output,
                Message::ToggleMute,
            ),
            Direction::Input => (
                &self.audio.sources,
                self.audio.default_source,
                Target::Input,
                Message::ToggleMic,
            ),
        };
        let icon = match direction {
            Direction::Output => Icon::Volume {
                level: device.volume as f32 / 100.0,
                muted: device.muted,
            },
            Direction::Input => Icon::Mic {
                muted: device.muted,
            },
        };
        let bay = match direction {
            Direction::Output => "output",
            Direction::Input => "input",
        };
        let mut lines = vec![
            self.control(icon, toggle, target, device.volume, device.muted),
            ui::bay(bay, device.name.clone()),
        ];
        if devices.len() == 1 {
            lines.push(ui::label(device.description.clone()).tone(Tone::Dim));
        } else {
            let rows = devices.iter().enumerate().map(|(index, device)| {
                let current = Some(index) == default;
                let mut row = ui::list_item(device.description.clone())
                    .leading(ui::icon(form_icon(device.form)))
                    .selected(current)
                    .on_press(Message::Choose(direction, index));
                if let Some(connection) = device.connection {
                    row = row.subtitle(connection);
                }
                if current {
                    row = row.trailing(ui::icon(Icon::Check));
                }
                row
            });
            lines.push(ui::list(rows));
        }
        ui::column(lines)
    }

    /// A mute button, a slider and the level.
    fn control(
        &self,
        icon: Icon,
        toggle: Message,
        target: Target,
        volume: u32,
        muted: bool,
    ) -> ui::Node<Message> {
        let max = self.config.max.max(volume);
        let level = ui::counter(volume.into(), 0..=max.into(), "%");
        let level = if muted { level.tone(Tone::Dim) } else { level };
        ui::row([
            ui::button(ui::icon(icon), toggle),
            ui::slider(volume as f32, 0.0..=max as f32, move |level| {
                Message::Set(target, level.round() as u32)
            }),
            level,
        ])
    }

    /// An app playing sound: its icon, its name over a slider, and a mute button.
    fn app(&self, stream: &Playback) -> ui::Node<Message> {
        let max = self.config.max.max(stream.volume);
        let target = Target::Stream(stream.index);
        let name = match &stream.title {
            Some(title) => format!("{} · {title}", stream.app),
            None => stream.app.clone(),
        };
        let id = stream
            .app_id
            .clone()
            .unwrap_or_else(|| stream.app.to_lowercase());
        ui::row([
            ui::app_icon(id),
            ui::column([
                ui::label(name),
                ui::slider(stream.volume as f32, 0.0..=max as f32, move |level| {
                    Message::Set(target, level.round() as u32)
                }),
            ]),
            ui::button(
                ui::icon(Icon::Volume {
                    level: stream.volume as f32 / 100.0,
                    muted: stream.muted,
                }),
                Message::MuteStream(stream.index),
            ),
        ])
    }

    fn device_mut(&mut self, direction: Direction) -> Option<&mut Device> {
        match direction {
            Direction::Output => {
                let index = self.audio.default_sink?;
                self.audio.sinks.get_mut(index)
            }
            Direction::Input => {
                let index = self.audio.default_source?;
                self.audio.sources.get_mut(index)
            }
        }
    }

    fn toggle(&mut self, direction: Direction) -> Effect<Message> {
        let Some(device) = self.device_mut(direction) else {
            return Effect::none();
        };
        device.muted = !device.muted;
        let (device, mute) = (device.clone(), device.muted);
        run(async move { eave_pulse::set_mute(&device, mute).await }).into()
    }

    /// Sends `level` for `target`, unless a level is already on its way: then it goes once that
    /// one is there, replacing any other still waiting.
    fn send(&mut self, target: Target, level: u32) -> Task<Message> {
        if let Some((_, pending)) = self.sending.iter_mut().find(|(t, _)| *t == target) {
            *pending = Some(level);
            return Task::none();
        }
        self.sending.push((target, None));
        self.set(target, level)
    }

    fn set(&self, target: Target, level: u32) -> Task<Message> {
        let done = move |result: std::io::Result<()>| {
            if let Err(error) = result {
                eprintln!("eave-volume: {error}");
            }
            Message::Sent(target)
        };
        match target {
            Target::Output | Target::Input => {
                let device = match direction(target) {
                    Direction::Output => self.audio.default_sink(),
                    Direction::Input => self.audio.default_source(),
                };
                let Some(device) = device.cloned() else {
                    return Task::done(Message::Sent(target));
                };
                Task::future(async move { eave_pulse::set_volume(&device, level).await }).map(done)
            }
            Target::Stream(index) => {
                let Some(stream) = self.streams.iter().find(|s| s.index == index).cloned() else {
                    return Task::done(Message::Sent(target));
                };
                Task::future(async move { eave_pulse::set_stream_volume(&stream, level).await })
                    .map(done)
            }
        }
    }

    /// Takes in the server's reading. A level that went up or down on the same sink nudges that
    /// way. Our own changes come back here too, one reading per step, so each nudges once.
    fn read(&mut self, mut audio: Audio) -> Effect<Message> {
        // A slider still being dragged keeps the level it was dragged to: readings lag behind it.
        for direction in [Direction::Output, Direction::Input] {
            let target = match direction {
                Direction::Output => Target::Output,
                Direction::Input => Target::Input,
            };
            if !self.sending.iter().any(|(t, _)| *t == target) {
                continue;
            }
            let ours = self
                .device_mut(direction)
                .map(|device| (device.index, device.volume));
            let theirs = match direction {
                Direction::Output => audio.default_sink.and_then(|i| audio.sinks.get_mut(i)),
                Direction::Input => audio.default_source.and_then(|i| audio.sources.get_mut(i)),
            };
            if let (Some((index, volume)), Some(device)) = (ours, theirs)
                && device.index == index
            {
                device.volume = volume;
            }
        }
        let heard = audio.default_sink().map(|sink| (sink.index, sink.volume));
        let before = std::mem::replace(&mut self.heard, heard);
        self.audio = audio;
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
        let Some(sink) = self.device_mut(Direction::Output) else {
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
        volume.audio = test_outputs(percent);
        volume
    }

    fn test_outputs(percent: u32) -> Audio {
        let mut audio = Audio::default();
        audio.sinks.push(eave_pulse::test_device(percent));
        audio.default_sink = Some(0);
        audio
    }

    fn level(volume: &Volume) -> u32 {
        volume.audio.default_sink().unwrap().volume
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
        effect.into_parts().2.map(|request| request.nudge)
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
    fn its_actions_follow_the_sound() {
        let titles = |volume: &Volume| {
            volume
                .actions()
                .into_iter()
                .map(|action| action.title().to_owned())
                .collect::<Vec<_>>()
        };
        let mut volume = volume_at(50);
        let mut headphones = eave_pulse::test_device(20);
        headphones.description = "Headphones".into();
        volume.audio.sinks.push(headphones);
        assert_eq!(
            titles(&volume),
            ["Mute", "Volume up", "Volume down", "Switch to Headphones"]
        );
        volume.update(Message::ToggleMute);
        assert_eq!(titles(&volume)[0], "Unmute");
        assert!(titles(&Volume::new(Config::default()).0).is_empty());
    }

    #[test]
    fn mute_shows_at_once() {
        let mut volume = volume_at(50);
        volume.update(Message::ToggleMute);
        assert!(volume.audio.default_sink().unwrap().muted);
    }

    #[test]
    fn a_drag_sends_one_level_at_a_time() {
        let mut volume = volume_at(50);
        volume.update(Message::Set(Target::Output, 60));
        assert_eq!(level(&volume), 60, "shown at once");
        assert_eq!(volume.sending, [(Target::Output, None)]);
        volume.update(Message::Set(Target::Output, 70));
        volume.update(Message::Set(Target::Output, 80));
        assert_eq!(
            volume.sending,
            [(Target::Output, Some(80))],
            "only the latest waits"
        );
        volume.update(Message::Sent(Target::Output));
        assert_eq!(volume.sending, [(Target::Output, None)], "and goes next");
        volume.update(Message::Sent(Target::Output));
        assert!(volume.sending.is_empty());
    }

    #[test]
    fn apps_are_watched_only_while_open() {
        let mut volume = volume_at(50);
        volume.update(Message::Opened);
        assert!(volume.open);
        volume.update(Message::Closed);
        assert!(!volume.open);
        assert!(volume.streams.is_empty());
    }

    #[test]
    fn output_and_apps_are_tabs_under_the_header() {
        let content = volume_at(50).popover().unwrap().into_node();
        let ui::Kind::Column(children) = content.into_kind() else {
            panic!("a column");
        };
        let mut children = children.into_iter().map(ui::Node::into_kind);
        assert!(matches!(children.next(), Some(ui::Kind::Header { .. })));
        let Some(ui::Kind::Tabs(pages)) = children.next() else {
            panic!("tabs under the header");
        };
        let names: Vec<_> = pages.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["Output", "Apps"], "no input without a source");
        let (_, apps) = pages.into_iter().nth(1).unwrap();
        assert!(matches!(
            apps.into_kind(),
            ui::Kind::Label { text, .. } if text == "Nothing is playing"
        ));
    }
}
