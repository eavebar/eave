//! What's playing, from any MPRIS player: the track's cover art, or the player's icon when it has
//! none, and the title and artist beside it, dimmed while paused. Right click plays or pauses,
//! scrolling skips tracks, and the island is nudged the way the track changed. The popover has the
//! cover, the track, a seek bar, the controls, and the players to choose from when there's more
//! than one. Hidden when nothing is playing or paused.

use std::time::Instant;

use eave_api::time::{Duration, every, sleep};
use eave_api::ui::{self, Icon, Item, Panel, Tone};
use eave_api::{Effect, Nudge, Registry, Subscription, Task, Widget};
use eave_mpris::{Player, Status};
use serde::Deserialize;

/// How long the last track stays on screen once its player has nothing to show. Players go
/// stopped with no track for a moment between songs, and that gap shouldn't hide the item.
const GAP: Duration = Duration::from_millis(1000);
/// How long after a skip we asked for its track can still come. A skip back that only restarts
/// the track brings none.
const SKIP_LANDS: Duration = Duration::from_secs(3);
/// How many tracks back a player's history goes.
const TRACKS_KEPT: usize = 32;

pub fn register(registry: &mut Registry) {
    registry.widget::<Media>("media");
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// What the bar item shows beside the cover.
    show: Show,
    /// Longer titles are cut, with an ellipsis.
    max_chars: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show: Show::Title,
            max_chars: 40,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Show {
    /// The cover, then the title and artist.
    Title,
    /// The cover, and the title and artist while hovered.
    Hover,
    /// Only the cover.
    Icon,
}

pub struct Media {
    config: Config,
    /// Every player, by bus name.
    players: Vec<Player>,
    /// The bus name of the player the bar follows.
    current: Option<String>,
    /// The player picked in the popover. It holds until another one starts playing.
    chosen: Option<String>,
    open: bool,
    /// The time the seek bar shows the position at, moved on every second while open.
    now: Instant,
    /// Where the seek bar is being dragged to. Sent when it's let go.
    dragging: Option<Duration>,
    /// Asked to skip on this player, which way, and when: a new track it reports soon after went
    /// that way, if the tracks it played don't already say.
    skipping: Option<(String, Nudge, Instant)>,
    /// The tracks the followed player went through, to tell going back from going on.
    tracks: Tracks,
    /// The covers the followed player showed last, so a new track doesn't flash its icon while
    /// its cover comes.
    art: Art,
    /// The player as it last showed, kept on screen for a `GAP` after it stopped showing.
    held: Option<Player>,
    /// Counts the holds, so only the last one's timer lets go.
    holds: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Players(Vec<Player>),
    PlayPause,
    Next,
    Previous,
    /// The seek bar moved, in seconds.
    Drag(f32),
    /// The seek bar was let go.
    Seek,
    /// Follow this player, by bus name.
    Choose(String),
    Tick(Instant),
    Opened,
    Closed,
    /// The hold with this number is up.
    Release(u64),
    /// The cover that went missing at this time is let go, if it's still missing.
    ArtGone(Instant),
}

impl Widget for Media {
    type Config = Config;
    type Message = Message;

    fn new(config: Config) -> (Self, Task<Message>) {
        let media = Self {
            config: Config {
                max_chars: config.max_chars.max(1),
                ..config
            },
            players: Vec::new(),
            current: None,
            chosen: None,
            open: false,
            now: Instant::now(),
            dragging: None,
            skipping: None,
            tracks: Tracks::default(),
            art: Art::default(),
            held: None,
            holds: 0,
        };
        (media, Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Players(players) => self.read(players),
            Message::PlayPause => {
                let now = Instant::now();
                let Some(player) = self.current_mut() else {
                    return Effect::none();
                };
                // Shown at once; the player's own reading follows and confirms it.
                player.position = player.position_at(now);
                player.read_at = now;
                player.status = match player.status {
                    Status::Playing => Status::Paused,
                    _ => Status::Playing,
                };
                let bus_name = player.bus_name.clone();
                run(async move { eave_mpris::play_pause(&bus_name).await }).into()
            }
            Message::Next => self.skip(Nudge::Right),
            Message::Previous => self.skip(Nudge::Left),
            Message::Drag(seconds) => {
                self.dragging = Some(Duration::from_secs_f32(seconds.max(0.0)));
                Effect::none()
            }
            Message::Seek => {
                let Some(position) = self.dragging.take() else {
                    return Effect::none();
                };
                let now = Instant::now();
                let Some(player) = self.current_mut() else {
                    return Effect::none();
                };
                let Some(track_id) = player.track_id.clone() else {
                    return Effect::none();
                };
                player.position = position;
                player.read_at = now;
                let bus_name = player.bus_name.clone();
                run(async move { eave_mpris::set_position(&bus_name, &track_id, position).await })
                    .into()
            }
            Message::Choose(bus_name) => {
                if self.players.iter().any(|p| p.bus_name == bus_name) {
                    self.current = Some(bus_name.clone());
                    self.chosen = Some(bus_name);
                }
                Effect::none()
            }
            Message::Tick(now) => {
                self.now = now;
                Effect::none()
            }
            Message::Release(hold) => {
                if hold == self.holds {
                    self.held = None;
                }
                Effect::none()
            }
            Message::ArtGone(lost) => {
                if self.art.lost == Some(lost) {
                    self.art = Art::default();
                }
                Effect::none()
            }
            Message::Opened => {
                self.open = true;
                self.now = Instant::now();
                Effect::none()
            }
            Message::Closed => {
                self.open = false;
                self.dragging = None;
                Effect::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let players = Subscription::run(eave_mpris::watch).map(Message::Players);
        // The seek bar moves on its own only while it can be seen.
        if !(self.open && self.current().is_some_and(Player::playing)) {
            return players;
        }
        let tick = every(Duration::from_secs(1)).map(Message::Tick);
        Subscription::batch([players, tick])
    }

    fn item(&self) -> Item<Message> {
        let Some(player) = self.showing() else {
            return Item::hidden();
        };
        let icon = || self.picture(player, false);
        let text = || ui::label(shorten(&track(player), self.config.max_chars));
        let content = match self.config.show {
            Show::Title => ui::row([icon(), text()]),
            Show::Hover => ui::hover(icon(), ui::row([icon(), text()])),
            Show::Icon => icon(),
        };
        let content = if player.playing() {
            content
        } else {
            content.tone(Tone::Dim)
        };
        Item::new(content)
            .on_secondary(Message::PlayPause)
            .on_scroll(Message::Previous, Message::Next)
    }

    fn popover(&self) -> Option<Panel<Message>> {
        let player = self.showing()?;
        let mut lines = vec![ui::header(player.identity.clone())];
        // The track, centered under its cover. The header already names the player, so a track
        // with no title has no heading.
        if !self.art.url(player).is_empty() {
            lines.push(ui::center(self.picture(player, true)));
        }
        if !player.title.is_empty() {
            lines.push(ui::center(ui::heading(player.title.clone())));
        }
        let from: Vec<_> = [&player.artist, &player.album]
            .into_iter()
            .filter(|line| !line.is_empty())
            .map(String::as_str)
            .collect();
        if !from.is_empty() {
            lines.push(ui::center(ui::label(from.join(" · ")).tone(Tone::Dim)));
        }
        if let Some(seek) = self.seek_bar(player) {
            lines.push(seek);
        }
        let play_pause = if player.playing() {
            Icon::Pause
        } else {
            Icon::Play
        };
        lines.push(ui::center(ui::row([
            ui::button(ui::icon(Icon::Previous), Message::Previous),
            ui::button(ui::icon(play_pause), Message::PlayPause).prominent(),
            ui::button(ui::icon(Icon::Next), Message::Next),
        ])));
        if self.players.len() > 1 {
            lines.push(ui::section("Players"));
            let rows = self.players.iter().map(|other| {
                let current = other.bus_name == player.bus_name;
                let mut row = ui::list_item(other.identity.clone())
                    .leading(ui::app_icon(app_id(other)))
                    .selected(current)
                    .on_press(Message::Choose(other.bus_name.clone()));
                if !other.title.is_empty() {
                    row = row.subtitle(track(other));
                }
                if current {
                    row = row.trailing(ui::icon(Icon::Check));
                }
                row
            });
            lines.push(ui::list(rows));
        }
        let panel = Panel::new(ui::column(lines))
            .on_open(Message::Opened)
            .on_close(Message::Closed);
        Some(panel)
    }
}

impl Media {
    fn current(&self) -> Option<&Player> {
        let current = self.current.as_ref()?;
        self.players.iter().find(|p| &p.bus_name == current)
    }

    /// The player on screen: the one followed while it has something to show, or for a `GAP`
    /// after, as it last was.
    fn showing(&self) -> Option<&Player> {
        self.current().filter(|p| shown(p)).or(self.held.as_ref())
    }

    fn current_mut(&mut self) -> Option<&mut Player> {
        let current = self.current.as_ref()?;
        self.players.iter_mut().find(|p| &p.bus_name == current)
    }

    /// Takes in the players' reading, picks the one to follow, and nudges if its track changed.
    /// has landed.
    fn read(&mut self, players: Vec<Player>) -> Effect<Message> {
        // A player that starts playing takes over from one picked by hand.
        let started = players.iter().any(|player| {
            player.playing()
                && Some(&player.bus_name) != self.chosen.as_ref()
                && !self
                    .players
                    .iter()
                    .any(|before| before.bus_name == player.bus_name && before.playing())
        });
        if started {
            self.chosen = None;
        }
        let was = self.current().filter(|p| shown(p)).cloned();
        self.current = pick(&players, self.chosen.as_deref(), self.current.as_deref());
        if self.chosen.as_ref() != self.current.as_ref() {
            self.chosen = None;
        }
        self.players = players;
        self.now = Instant::now();

        // Gone with nothing to show: keep the last of it on screen for a moment, in case it's
        // only between tracks.
        let mut tasks = Vec::new();
        let current = self.current().cloned();
        if let Some(lost) = self.art.read(current.as_ref()) {
            tasks.push(Task::future(sleep(GAP)).map(move |()| Message::ArtGone(lost)));
        }
        if self.current().is_some_and(shown) {
            self.held = None;
        } else if let Some(was) = was {
            self.held = Some(was);
            self.holds += 1;
            let hold = self.holds;
            tasks.push(Task::future(sleep(GAP)).map(move |()| Message::Release(hold)));
        }
        let effect = Effect::task(Task::batch(tasks));

        // Another track on the same player nudges the way it went: back to the one before, or
        // on. MPRIS doesn't say which, so the tracks it played do, or failing that the skip we
        // asked for. A gap between tracks has no title, and doesn't count.
        let Some((bus_name, key)) = self
            .current()
            .filter(|p| !p.title.is_empty())
            .map(|p| (p.bus_name.clone(), (p.title.clone(), p.artist.clone())))
        else {
            return effect;
        };
        let asked = match self.skipping.take() {
            Some((skipped, nudge, at)) if skipped == bus_name && at.elapsed() < SKIP_LANDS => {
                Some(nudge)
            }
            _ => None,
        };
        match self.tracks.went(&bus_name, key, asked) {
            Some(nudge) => effect.nudge(nudge),
            None => {
                // Not a new track yet: the skip is still on its way.
                if let Some(nudge) = asked {
                    self.skipping = Some((bus_name, nudge, Instant::now()));
                }
                effect
            }
        }
    }

    /// Asks the current player for the next or previous track, `nudge` saying which.
    fn skip(&mut self, nudge: Nudge) -> Effect<Message> {
        let Some(player) = self.current() else {
            return Effect::none();
        };
        let bus_name = player.bus_name.clone();
        self.skipping = Some((bus_name.clone(), nudge, Instant::now()));
        let task = match nudge {
            Nudge::Left => run(async move { eave_mpris::previous(&bus_name).await }),
            _ => run(async move { eave_mpris::next(&bus_name).await }),
        };
        task.into()
    }

    /// Elapsed time, a slider through the track, and its length. Only for a track that has a
    /// length and can be moved through.
    fn seek_bar(&self, player: &Player) -> Option<ui::Node<Message>> {
        let length = player.length?;
        let position = self
            .dragging
            .unwrap_or_else(|| player.position_at(self.now))
            .min(length);
        if !(player.can_seek && player.track_id.is_some()) {
            return Some(ui::row([
                ui::label(clock(position)).tone(Tone::Dim),
                ui::meter(position.as_secs_f32() / length.as_secs_f32()),
                ui::label(clock(length)).tone(Tone::Dim),
            ]));
        }
        Some(ui::row([
            ui::label(clock(position)).tone(Tone::Dim),
            ui::slider(
                position.as_secs_f32(),
                0.0..=length.as_secs_f32(),
                Message::Drag,
            )
            .on_release(Message::Seek),
            ui::label(clock(length)).tone(Tone::Dim),
        ]))
    }
}

/// The player to follow: the one picked by hand while it's there; else the one followed so far
/// if it's still playing; else the first one playing; else the one followed so far; else the
/// first one.
fn pick(players: &[Player], chosen: Option<&str>, current: Option<&str>) -> Option<String> {
    let find = |name: Option<&str>| {
        let name = name?;
        players.iter().find(|p| p.bus_name == name)
    };
    find(chosen)
        .or_else(|| find(current).filter(|p| p.playing()))
        .or_else(|| players.iter().find(|p| p.playing()))
        .or_else(|| find(current))
        .or_else(|| players.first())
        .map(|p| p.bus_name.clone())
}

/// The covers the followed player showed last. A new track's cover takes a moment to come, and
/// some players (Chrome) send the track first, with no cover or one still to fetch: meanwhile the
/// last cover stays instead of the player's icon.
#[derive(Default)]
struct Art {
    bus_name: String,
    /// The last cover it had.
    url: String,
    /// The one before that, shown while `url` is fetched.
    before: String,
    /// When it went without a cover, if it has none now. `url` is kept for a `GAP` after.
    lost: Option<Instant>,
}

impl Art {
    /// Takes in the followed player's reading. Returns when its cover went missing, if it just
    /// did, to let it go a `GAP` later.
    fn read(&mut self, player: Option<&Player>) -> Option<Instant> {
        let player = player?;
        if self.bus_name != player.bus_name {
            *self = Self {
                bus_name: player.bus_name.clone(),
                ..Self::default()
            };
        }
        if !player.art_url.is_empty() {
            if self.url != player.art_url {
                self.before = std::mem::replace(&mut self.url, player.art_url.clone());
            }
            self.lost = None;
            return None;
        }
        if self.url.is_empty() || self.lost.is_some() {
            return None;
        }
        let now = Instant::now();
        self.lost = Some(now);
        Some(now)
    }

    /// The cover to show for `player`: its own, or the last one while it's between covers.
    fn url<'a>(&'a self, player: &'a Player) -> &'a str {
        if !player.art_url.is_empty() || self.bus_name != player.bus_name {
            &player.art_url
        } else {
            &self.url
        }
    }

    /// What shows until the cover at `url` is there: the one before it, if that's another.
    fn before(&self, url: &str) -> Option<&str> {
        (url == self.url && !self.before.is_empty()).then_some(self.before.as_str())
    }
}

impl Media {
    /// The track's cover, or until it's there and when there's none, the player's icon; the media
    /// bars for a player that doesn't say which app it is. Between tracks, the last cover stays
    /// until the next one is there. `large` for the popover, where nothing shows without one.
    fn picture(&self, player: &Player, large: bool) -> ui::Node<Message> {
        let placeholder = if large {
            ui::empty()
        } else if player.desktop_entry.is_empty() {
            ui::icon(Icon::Media {
                playing: player.playing(),
            })
        } else {
            ui::app_icon(player.desktop_entry.clone())
        };
        let cover = |url: &str, fallback| {
            let cover = ui::cover(url.to_owned(), fallback);
            if large { cover.large() } else { cover }
        };
        let url = self.art.url(player);
        if url.is_empty() {
            return placeholder;
        }
        let fallback = match self.art.before(url) {
            Some(before) => cover(before, placeholder),
            None => placeholder,
        };
        cover(url, fallback)
    }
}

/// A player is shown while it has something to show: anything but stopped with no track.
fn shown(player: &Player) -> bool {
    player.status != Status::Stopped || !player.title.is_empty()
}

/// The tracks one player went through, in order, and which of them it's on: going to the one
/// before is going back, to the one after or a new one is going on.
#[derive(Default)]
struct Tracks {
    bus_name: String,
    /// Each track's title and artist. A player may give a track a new id each time it plays it.
    list: Vec<(String, String)>,
    at: usize,
}

impl Tracks {
    /// The player is on `track` now: which way that went from the one before, if it's another
    /// one on the same player. `asked` is the way we skipped, for a track it hasn't seen.
    fn went(
        &mut self,
        bus_name: &str,
        track: (String, String),
        asked: Option<Nudge>,
    ) -> Option<Nudge> {
        if self.bus_name != bus_name {
            *self = Self {
                bus_name: bus_name.to_owned(),
                list: vec![track],
                at: 0,
            };
            return None;
        }
        if self.list.get(self.at) == Some(&track) {
            return None;
        }
        if self.at > 0 && self.list[self.at - 1] == track {
            self.at -= 1;
            return Some(Nudge::Left);
        }
        if self.list.get(self.at + 1) == Some(&track) {
            self.at += 1;
            return Some(Nudge::Right);
        }
        if asked == Some(Nudge::Left) {
            self.list.insert(self.at, track);
            return Some(Nudge::Left);
        }
        self.list.truncate(self.at + 1);
        self.list.push(track);
        if self.list.len() > TRACKS_KEPT {
            self.list.remove(0);
        }
        self.at = self.list.len() - 1;
        Some(Nudge::Right)
    }
}

/// "Title · Artist", the title alone, or the player's name when there's no title.
fn track(player: &Player) -> String {
    match (player.title.as_str(), player.artist.as_str()) {
        ("", _) => player.identity.clone(),
        (title, "") => title.to_owned(),
        (title, artist) => format!("{title} · {artist}"),
    }
}

/// The icon to look up for a player: its desktop entry, or failing that its name.
fn app_id(player: &Player) -> String {
    if player.desktop_entry.is_empty() {
        player.identity.to_lowercase()
    } else {
        player.desktop_entry.clone()
    }
}

/// `m:ss`, or `h:mm:ss` from an hour.
fn clock(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn shorten(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        None => text.to_string(),
        Some(_) => {
            let cut = text
                .char_indices()
                .nth(max_chars - 1)
                .map_or(text.len(), |(at, _)| at);
            format!("{}…", text[..cut].trim_end())
        }
    }
}

fn run(work: impl Future<Output = eave_mpris::zbus::Result<()>> + Send + 'static) -> Task<Message> {
    Task::future(async move {
        if let Err(error) = work.await {
            eprintln!("eave-media: {error}");
        }
    })
    .discard()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, status: Status, title: &str) -> Player {
        Player {
            bus_name: format!("org.mpris.MediaPlayer2.{name}"),
            identity: name.to_owned(),
            desktop_entry: name.to_owned(),
            status,
            title: title.to_owned(),
            artist: "Artist".to_owned(),
            album: String::new(),
            art_url: String::new(),
            length: Some(Duration::from_secs(200)),
            track_id: None,
            position: Duration::ZERO,
            read_at: Instant::now(),
            rate: 1.0,
            can_go_next: true,
            can_go_previous: true,
            can_play: true,
            can_pause: true,
            can_seek: true,
        }
    }

    fn media(config: Config) -> Media {
        Media::new(config).0
    }

    fn current(media: &Media) -> &str {
        media.current.as_deref().unwrap()
    }

    fn content(media: &Media) -> ui::Kind<Message> {
        media.item().into_parts().content.unwrap().into_kind()
    }

    #[test]
    fn hidden_with_no_players_or_nothing_to_show() {
        let mut media = media(Config::default());
        assert!(media.item().into_parts().content.is_none());
        media.update(Message::Players(vec![player("mpv", Status::Stopped, "")]));
        assert!(media.item().into_parts().content.is_none());
        assert!(media.popover().is_none());
    }

    #[test]
    fn a_gap_between_tracks_keeps_the_last_one_on_screen() {
        let mut media = media(Config::default());
        media.update(Message::Players(vec![player("a", Status::Playing, "One")]));
        media.update(Message::Players(vec![player("a", Status::Stopped, "")]));
        assert!(media.item().into_parts().content.is_some(), "held");
        assert_eq!(media.showing().unwrap().title, "One");
        // The next track comes before the hold is up: no gap, and the old timer does nothing.
        media.update(Message::Players(vec![player("a", Status::Playing, "Two")]));
        media.update(Message::Release(1));
        assert_eq!(media.showing().unwrap().title, "Two");
        // Really stopped: hidden once the hold is up.
        media.update(Message::Players(vec![player("a", Status::Stopped, "")]));
        media.update(Message::Release(1));
        assert!(media.item().into_parts().content.is_some(), "a stale timer");
        media.update(Message::Release(2));
        assert!(media.item().into_parts().content.is_none());
        // The player leaving altogether is held the same way.
        media.update(Message::Players(vec![player(
            "a",
            Status::Playing,
            "Three",
        )]));
        media.update(Message::Players(Vec::new()));
        assert!(media.item().into_parts().content.is_some());
    }

    #[test]
    fn follows_the_player_that_plays() {
        let mut media = media(Config::default());
        media.update(Message::Players(vec![
            player("a", Status::Paused, "One"),
            player("b", Status::Playing, "Two"),
        ]));
        assert_eq!(current(&media), "org.mpris.MediaPlayer2.b");
        // Paused, it's still the one followed.
        media.update(Message::Players(vec![
            player("a", Status::Paused, "One"),
            player("b", Status::Paused, "Two"),
        ]));
        assert_eq!(current(&media), "org.mpris.MediaPlayer2.b");
        // Until another one plays.
        media.update(Message::Players(vec![
            player("a", Status::Playing, "One"),
            player("b", Status::Paused, "Two"),
        ]));
        assert_eq!(current(&media), "org.mpris.MediaPlayer2.a");
    }

    #[test]
    fn a_player_picked_by_hand_holds_until_another_starts() {
        let mut media = media(Config::default());
        media.update(Message::Players(vec![
            player("a", Status::Paused, "One"),
            player("b", Status::Playing, "Two"),
        ]));
        media.update(Message::Choose("org.mpris.MediaPlayer2.a".into()));
        assert_eq!(current(&media), "org.mpris.MediaPlayer2.a");
        // Still playing elsewhere isn't starting.
        media.update(Message::Players(vec![
            player("a", Status::Paused, "One"),
            player("b", Status::Playing, "Two"),
        ]));
        assert_eq!(current(&media), "org.mpris.MediaPlayer2.a");
        // Starting is.
        media.update(Message::Players(vec![
            player("a", Status::Paused, "One"),
            player("b", Status::Paused, "Two"),
        ]));
        media.update(Message::Players(vec![
            player("a", Status::Paused, "One"),
            player("b", Status::Playing, "Two"),
        ]));
        assert_eq!(current(&media), "org.mpris.MediaPlayer2.b");
    }

    #[test]
    fn shows_the_cover_then_the_track() {
        let mut media = media(Config::default());
        let mut playing = player("a", Status::Playing, "Song");
        playing.art_url = "file:///tmp/cover.jpg".into();
        media.update(Message::Players(vec![playing]));
        let ui::Kind::Row(children) = content(&media) else {
            panic!("a row");
        };
        let mut children = children.into_iter().map(ui::Node::into_kind);
        let Some(ui::Kind::Cover { url, fallback, .. }) = children.next() else {
            panic!("the cover first");
        };
        assert_eq!(url, "file:///tmp/cover.jpg");
        assert!(
            matches!(fallback.into_kind(), ui::Kind::AppIcon { app_id, .. } if app_id == "a"),
            "the player's icon until it's there"
        );
        assert!(matches!(
            children.next(),
            Some(ui::Kind::Label { text, .. }) if text == "Song · Artist"
        ));
    }

    #[test]
    fn the_last_cover_stays_until_the_next_one_is_there() {
        let mut media = media(Config {
            show: Show::Icon,
            ..Config::default()
        });
        let with = |title: &str, art: &str| {
            let mut playing = player("a", Status::Playing, title);
            playing.art_url = art.into();
            Message::Players(vec![playing])
        };
        let cover = |media: &Media| match content(media) {
            ui::Kind::Cover { url, fallback, .. } => (url, Some(fallback.into_kind())),
            other => (
                format!("{:?}", matches!(other, ui::Kind::AppIcon { .. })),
                None,
            ),
        };
        media.update(with("One", "file:///one.jpg"));
        // The next track, its cover not sent yet: the last one stays.
        media.update(with("Two", ""));
        assert_eq!(cover(&media).0, "file:///one.jpg");
        // Its cover is sent: shown once fetched, the last one until then.
        media.update(with("Two", "file:///two.jpg"));
        let (url, fallback) = cover(&media);
        assert_eq!(url, "file:///two.jpg");
        assert!(matches!(fallback, Some(ui::Kind::Cover { url, .. }) if url == "file:///one.jpg"));
        // A track that really has none shows the player's icon, a moment later.
        media.update(with("Three", ""));
        let lost = media.art.lost.unwrap();
        assert_eq!(cover(&media).0, "file:///two.jpg");
        media.update(Message::ArtGone(lost));
        let (icon, fallback) = cover(&media);
        assert_eq!(icon, "true", "the player's icon");
        assert!(fallback.is_none());
    }

    #[test]
    fn shows_only_what_it_is_told_to() {
        let players = vec![player("a", Status::Paused, "Song")];
        let mut hover = media(Config {
            show: Show::Hover,
            ..Config::default()
        });
        hover.update(Message::Players(players.clone()));
        assert!(matches!(content(&hover), ui::Kind::Alternate { .. }));

        let mut icon = media(Config {
            show: Show::Icon,
            ..Config::default()
        });
        icon.update(Message::Players(players));
        assert!(matches!(
            content(&icon),
            ui::Kind::AppIcon {
                tone: Some(Tone::Dim),
                ..
            }
        ));
    }

    #[test]
    fn a_player_that_says_nothing_of_itself_shows_the_bars() {
        let mut media = media(Config {
            show: Show::Icon,
            ..Config::default()
        });
        let mut anonymous = player("a", Status::Playing, "Song");
        anonymous.desktop_entry.clear();
        media.update(Message::Players(vec![anonymous]));
        assert!(matches!(
            content(&media),
            ui::Kind::Icon {
                icon: Icon::Media { playing: true },
                ..
            }
        ));
    }

    #[test]
    fn reads_the_show_option() {
        let config: Config = toml::from_str("show = \"hover\"\nmax_chars = 20").unwrap();
        assert_eq!(config.show, Show::Hover);
        assert_eq!(config.max_chars, 20);
        assert!(toml::from_str::<Config>("show = \"art\"").is_err());
    }

    #[test]
    fn long_tracks_are_cut() {
        assert_eq!(shorten("Midnight City", 20), "Midnight City");
        assert_eq!(shorten("Midnight City", 9), "Midnight…");
    }

    #[test]
    fn a_new_track_nudges_the_way_it_went() {
        let nudge = |effect: Effect<Message>| effect.into_parts().2.map(|request| request.nudge);
        let mut media = media(Config::default());
        let play = |media: &mut Media, title: &str| {
            nudge(media.update(Message::Players(vec![player("a", Status::Playing, title)])))
        };
        assert_eq!(play(&mut media, "One"), None, "the first track");
        assert_eq!(play(&mut media, "One"), None, "the same track");
        assert_eq!(play(&mut media, "Two"), Some(Nudge::Right));
        assert_eq!(play(&mut media, "Three"), Some(Nudge::Right));
        // Back and forth, from a media key or the player's own buttons: the tracks say which way.
        assert_eq!(play(&mut media, "Two"), Some(Nudge::Left));
        assert_eq!(play(&mut media, "One"), Some(Nudge::Left));
        assert_eq!(play(&mut media, "Two"), Some(Nudge::Right));
        // Through a gap between tracks: one nudge, when the next one comes.
        assert_eq!(
            nudge(media.update(Message::Players(vec![player("a", Status::Stopped, "")]))),
            None
        );
        assert_eq!(play(&mut media, "Three"), Some(Nudge::Right));
        // Another player taking over isn't a track change.
        assert_eq!(
            nudge(media.update(Message::Players(vec![
                player("a", Status::Paused, "Three"),
                player("b", Status::Playing, "Other"),
            ]))),
            None
        );
    }

    #[test]
    fn a_skip_back_to_a_track_never_seen_goes_left() {
        let nudge = |effect: Effect<Message>| effect.into_parts().2.map(|request| request.nudge);
        let mut media = media(Config::default());
        media.update(Message::Players(vec![player("a", Status::Playing, "Five")]));
        assert_eq!(nudge(media.update(Message::Previous)), None, "not yet");
        assert_eq!(
            nudge(media.update(Message::Players(vec![player("a", Status::Playing, "Five")]))),
            None,
            "the same track"
        );
        assert_eq!(
            nudge(media.update(Message::Players(vec![player("a", Status::Playing, "Four")]))),
            Some(Nudge::Left)
        );
        // And on from there is the track it came from.
        assert_eq!(
            nudge(media.update(Message::Players(vec![player("a", Status::Playing, "Five")]))),
            Some(Nudge::Right)
        );
    }

    #[test]
    fn play_pause_shows_at_once() {
        let mut media = media(Config::default());
        media.update(Message::Players(vec![player("a", Status::Playing, "One")]));
        media.update(Message::PlayPause);
        assert_eq!(media.current().unwrap().status, Status::Paused);
    }

    #[test]
    fn dragging_the_seek_bar_holds_it_until_let_go() {
        let mut media = media(Config::default());
        let mut playing = player("a", Status::Paused, "One");
        playing.track_id = Some("/track/1".try_into().unwrap());
        media.update(Message::Players(vec![playing]));
        media.update(Message::Opened);
        media.update(Message::Drag(42.0));
        assert_eq!(media.dragging, Some(Duration::from_secs(42)));
        media.update(Message::Seek);
        assert_eq!(media.dragging, None);
        assert_eq!(media.current().unwrap().position, Duration::from_secs(42));
    }

    #[test]
    fn the_popover_lists_players_only_when_there_are_several() {
        let sections = |media: &Media| {
            let ui::Kind::Column(lines) = media.popover().unwrap().into_node().into_kind() else {
                panic!("a column");
            };
            lines
                .into_iter()
                .filter(|line| matches!(line.kind(), ui::Kind::Section { .. }))
                .count()
        };
        let mut media = media(Config::default());
        media.update(Message::Players(vec![player("a", Status::Playing, "One")]));
        assert_eq!(sections(&media), 0);
        media.update(Message::Players(vec![
            player("a", Status::Playing, "One"),
            player("b", Status::Paused, "Two"),
        ]));
        assert_eq!(sections(&media), 1);
    }

    #[test]
    fn clock_reads_like_a_player() {
        assert_eq!(clock(Duration::from_secs(72)), "1:12");
        assert_eq!(clock(Duration::from_secs(3)), "0:03");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
    }
}
