# Plugins

Everything in the bar is a plugin, including the clock. A plugin is an ordinary Rust crate that provides one widget, an `eave_api::Widget`. It lives in a git repository, alone or with others, and gets compiled into the bar by `eave-build`. Nothing is loaded at runtime ([D-12](decisions.md)).

## Using plugins

`~/.config/eave/config.toml` is the only file you edit. It says what appears where, and the same `widget = "…"` says where each widget comes from ([D-30](decisions.md)):

```toml
[[left]]
widget = "workspaces"                 # built in

[[center]]
widget = "clock"
format = "%a %d %b  %H:%M"            # every key except `widget` goes to the widget

[[right]]
widget = "someone/weather"            # a plugin: github.com/someone/weather
units = "metric"
```

The zones are `left`, `center` and `right`, in order. Every key except `widget`, `width_group` and `collapsible` goes to the widget's `Config`.

`width_group = "name"` lines items up: items in the same island with the same group all get a slot as wide as the widest of them, each centered in its slot ([D-34](decisions.md)). For example, put `volume` and `network` in one group, so the Wi-Fi icon gets the same room as the volume item, which needs space for its percent.

A zone is one island, or several side by side. Entries without a widget arrange them ([D-55](decisions.md)):

```toml
[[right]]
widget = "volume"
collapsible = true     # hidden until you open the island

[[right]]
divider = true         # a thin line before the next item

[[right]]
widget = "network"

[[right]]
island = true          # what follows is another island, beside this one

[[right]]
widget = "battery"
```

- `island = true` starts another island in the same zone. Add `live = true` in the same entry to make it a live island, snug around its items and rounder, still a squircle ([D-60](decisions.md)). A marker before the zone's first item sets the first island.
- `divider = true` puts a line before the next item. It shows only between two items that show.
- `collapsible = true` hides the item while its island is collapsed, which it is at first. Hover over the island and a chevron appears on the side facing the middle of the screen. Press it to show the hidden items in their places, and again to hide them. A collapsed item whose widget nudges (the volume going up, say) comes out by itself for two seconds after the last nudge, then tucks back.

Notifications open from the right zone's last island, the one at the screen's edge.

Unknown widgets and invalid options are reported on stderr and skipped. They never crash the bar. Changes apply as soon as you save, with no restart. Widgets whose entry didn't change keep their state, and a file that doesn't parse leaves the bar as it was ([D-21](decisions.md)).

### Widget references

```
<source>[@rev][#widget]
```

| You write | Comes from | Widget |
|---|---|---|
| `clock` (no `/`) | built in: the `clock` plugin of eave's own collection | `clock` |
| `someone/weather` | `https://github.com/someone/weather` | `weather` |
| `someone/eave-weather@v0.2` | the same, at tag `v0.2` (or any commit or branch) | `weather`: a leading `eave-` is dropped |
| `codeberg.org/x/eave-mail`, `https://…`, `git@host:x/y.git` | any git repository | `mail` |
| `~/code/eave-weather`, `./weather`, `/abs/path` | a local directory, for working on a plugin | `weather` |
| `someone/tools#cpu` | `github.com/someone/tools` | `cpu`: picks one plugin of a repository that has several |

Built-in widgets come with eave, so they take no `@rev`. The same plugin can appear many times, but always at one revision. An optional top-level `eave = "<source>[@rev]"` says where eave itself comes from. It defaults to `eaveshell/eave`, and `eave = "~/code/eave"` builds from a local checkout.

### Compiling plugins in

The binary in `~/.local/bin/eave` holds exactly the widgets your config.toml uses. When the config starts using another one, `eave-build` compiles it in:

```fish
eave-build           # build what config.toml uses; install and restart the bar if it changed
eave-build update    # fetch newer eave and plugins first, then the same
```

The installer sets up `eave-rebuild.path`, which runs `eave-build` on every save ([running.md](running.md#rebuild-automatically-when-configtoml-changes)). A save that only moves widgets or changes options builds nothing new and doesn't restart the bar. Until a new plugin is compiled in, the bar reports it as not compiled in and skips it.

**Nothing updates by itself** ([D-31](decisions.md)). Git sources are cloned once, to `~/.local/share/eave/src`, and stay at that commit. `eave-build update` fetches every source, moves unpinned ones to their default branch and pinned ones to the newest commit of their `@rev`, and prints what moved. Changing a pin in config.toml checks out that revision on the next build, fetching it if needed. Removing a pin leaves the checkout where it is until the next update.

The generated crate builds in `~/.cache/eave/build`, so rebuilds are incremental. The plain `cargo run --release` in this repo builds the default binary, which has every built-in plugin and ignores the config's sources.

## Built-in plugins

Use them by name. The default binary (`cargo run --release`) has all of them, and an installed bar only those config.toml uses.

| Widget | Shows | Options |
|---|---|---|
| `clock` | time, the date while hovered, calendar in the popover | `format` (strftime, default `%H:%M`), `hover_format` (strftime, shown on hover, default `%b %-d`: `Oct 21`) |
| `workspaces` | Hyprland workspaces that have windows, plus the active one. Each shows its name and its apps' icons (two at most, then `+n` for the other windows), and one icon per window while the pointer rests on it. Workspaces you aren't on are dimmed. Click to go, right click to list its windows (click one to focus it), scroll to move | `persistent` (workspaces 1..N always shown, even empty, default 0) |
| `window` | focused window title, hidden when nothing has focus | `max_chars` (default 60) |
| `battery` | battery icon filled to the charge (green, its charge rippling and gulping while charging, and the hovered percent gulping too), warning/critical tones, the percent while hovered; time left and power draw in the popover; hidden without a battery | `warning` (default 20), `critical` (default 10) |
| `network` | Wi-Fi icon with as many bars as the signal, wired icon, struck-through Wi-Fi when offline; name, signal, band, address, gateway in the popover | `show_name` (default false): the Wi-Fi name next to the icon |
| `volume` | speaker icon with 0–3 waves for the volume, struck through when muted; the percent while hovered and for 1.5 s after a change, with a nudge; scroll to change, right click to mute; −/+, mute, output list in the popover | `step` (default 5), `max` (default 100) |
| `keyboard` | active layout, click for the next one; hidden with a single layout | `aliases` (`{ "English (US)" = "us" }`) |
| `media` | what an MPRIS player is playing: the track's cover (the player's icon without one), then title · artist, dimmed while paused; right click to play or pause, scroll to skip tracks, with a nudge; cover, track, seek bar, controls and a player list in the popover; hidden when nothing plays or is paused ([D-56](decisions.md)) | `show` (`"title"` (default), `"hover"`: the text only while hovered, `"icon"`: the cover alone), `max_chars` (default 40) |

```toml
[[left]]
widget = "workspaces"
persistent = 5

[[left]]
widget = "window"

[[center]]
widget = "media"
show = "hover"

[[right]]
widget = "keyboard"
aliases = { "English (US)" = "us", "Italian" = "it" }
```

Notifications come from a plugin of their own, eave's notification server, in [eaveshell/eave-notifications](https://github.com/eaveshell/eave-notifications): `widget = "eaveshell/eave-notifications"` ([D-50](decisions.md)).

## Writing a plugin

```toml
# Cargo.toml
[dependencies]
eave-api = "0.1"
serde = { version = "1", features = ["derive"] }
```

`eave-api` and the service crates are on crates.io, so a plugin builds and tests on its own ([D-50](decisions.md)). When it's compiled into a bar, `eave-build` patches them to the eave checkout it builds against, so the host and every plugin share one copy.

```toml
# eave-plugin.toml, beside the crate's Cargo.toml
[plugin]
name = "weather"         # the one widget it registers
description = "Current weather, forecast in the popover"
api = "0.1"              # eave-api version you target (cargo caret rules)
```

A plugin is one crate with one widget, registered under the manifest's `name` ([D-46](decisions.md)). `eave-build` takes the crate's name from its `Cargo.toml` and calls its `register`.

**One plugin per repository:** put `eave-plugin.toml` and `Cargo.toml` at the root. Users write `widget = "you/<repo>"`, which asks for the plugin named like the repository, without a leading `eave-`. So a repository `eave-weather` should set `name = "weather"`. When the names differ, `eave-build` says to write `you/<repo>#<name>`.

**Several plugins in one repository:** put an `eave-collection.toml` at the root that lists their directories, each holding its own `eave-plugin.toml` and `Cargo.toml`. Users pick one with `#name`. eave's own built-ins are such a collection.

```toml
# eave-collection.toml, at the repository root
[collection]
description = "System monitors"
plugins = ["cpu", "mem"]   # directories, relative to this file
```

```toml
[[right]]
widget = "you/eave-monitors#cpu"
[[right]]
widget = "you/eave-monitors#mem"
```

A repository has one of the two files at its root, never both. Plugins in a collection can share code through a library crate of their own, and only the ones a config uses are compiled in. A name that isn't there fails the build with a list of the ones that are.

```rust
use eave_api::ui::{self, Item, Panel, Tone};
use eave_api::{Effect, NoConfig, Registry, Task, Widget};

pub fn register(registry: &mut Registry) {
    registry.widget::<Counter>("counter");
}

pub struct Counter(u32);

#[derive(Debug, Clone)]
pub enum Message {
    Reset,
}

impl Widget for Counter {
    type Config = NoConfig;      // or your own #[derive(Deserialize)] struct
    type Message = Message;

    fn new(_: NoConfig) -> (Self, Task<Message>) {
        (Self(0), Task::none())
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Reset => self.0 = 0,
        }
        Effect::none()
    }

    fn item(&self) -> Item<Message> {
        Item::new(ui::label(self.0.to_string()))
    }

    fn popover(&self) -> Option<Panel<Message>> {
        Some(Panel::new(ui::column([
            ui::heading("Counter"),
            ui::button(ui::label("Reset"), Message::Reset),
        ])))
    }
}
```

### Services

Widgets that read the same source share it through a service crate, instead of each opening its own connection. For Hyprland, depend on `eave-hyprland` (patched by `eave-build` like `eave-api`):

```rust
fn subscription(&self) -> Subscription<Message> {
    // A snapshot first, then every event after it. Re-snapshots by itself if events were missed.
    Subscription::run(|| eave_hyprland::watch(snapshot)).map(Message::Hyprland)
}

async fn snapshot() -> std::io::Result<eave_hyprland::types::Window> {
    eave_hyprland::query("activewindow").await
}
```

`eave_hyprland::dispatch("hl.dsp.focus({ workspace = 3 })", "workspace 3")` runs a dispatcher. Give it in both the Lua syntax of Hyprland 0.56+ and the older one, and it uses whichever this Hyprland speaks. `request("…")` sends any other command.

For D-Bus, depend on `eave-dbus`: `system()` and `session()` are the shared bus connections. `eave_dbus::watch(service, snapshot)` yields a snapshot of the service's state, then a new one after each `PropertiesChanged` on the objects the snapshot depends on. Unchanged snapshots are skipped. `properties(...)` reads every property of an interface in one call.

For the network, depend on `eave-net`: `watch()` yields the primary connection (interface, Wi-Fi SSID and signal, address, gateway) after every change. It reads the kernel directly, so it works whichever network manager is installed.

For media players, depend on `eave-mpris`: `watch()` yields every MPRIS player (status, track, cover URL, position, what it can do) whenever one comes, goes or changes, and `play_pause`, `next`, `previous` and `set_position` send commands. `Player::position_at(now)` is where a playing track is now, since players don't signal it as it moves.

For audio, depend on `eave-pulse`: `watch()` yields the outputs (sinks, default, volume, mute) after every change, and `set_volume`, `set_mute` and `set_default` send commands. Run them in a `Task`.

To send a notification, call `Notify` on `org.freedesktop.Notifications` over the session bus (`zbus::Connection::session()`; `eave-dbus` re-exports zbus), like any app ([D-52](decisions.md)). Whatever daemon the user runs shows it: eave's own, `eave-notify` in [eaveshell/eave-notifications](https://github.com/eaveshell/eave-notifications) ([D-50](decisions.md)), or mako, dunst or swaync. Put anything the spec lacks in `x-eave-<name>` hints, which other daemons ignore.

### The rules

- **Views are pure and cheap.** `item()` runs on every redraw, and `popover()` runs after every message to your widget. Precompute in `update`.
- **`update` never blocks.** I/O goes in a `Task` (`Task::perform`) or a `Subscription`.
- **Events, not polling.** Use `eave_api::time::every` only when there's no event source (see [development.md](development.md#code-rules)). To show something for a moment, return `Task::future(eave_api::time::sleep(d))` mapped to a message, numbered so that only the latest one counts.
- **You describe, the host draws.** The vocabulary is `label`, `heading`, `icon`, `app_icon`, `image`, `symbol`, `cover`, `counter`, `row`, `column`, `grid`, `button`, `switcher`, `alternate`, `hover`, `expand`, `gulp`, `separator`, `empty`, plus `.tone(Tone::…)` (`Normal`, `Dim`, `Accent`, `Warning`, `Critical`, `Good`). `icon(Icon::…)` is a status symbol (volume, Wi-Fi, wired, battery, mic, bell) that says how full it is or whether it's off, or a plain symbol (lock, info, back, forward, check, refresh, close, reveal, bolt, plug, speaker, headphones, display, media, play, pause, previous, next), and the host draws it in the text's tone. `switcher(options, current)` is a row of choices with one marked current, and the host slides the mark when it changes. `counter(value, range, unit)` is a number the host counts up or down through every value in between when it changes. It keeps room for the widest number in `range`, so it holds its width. `alternate(options, current)` shows one option at a time, as big as the biggest one, and fades between them, e.g. an icon that turns into a `counter` for a moment. `.on_hover(index)` on an alternate shows that option while the pointer is over the item, and `hover(normal, hovered)` is the short form for two: an icon that shows its percent, a time that shows the date. `app_icon(app_id)` is an application's icon, named by window class or app id. The host finds it in the user's icon theme and draws it in its own colors, dimmed by `Tone::Dim`. `image(include_bytes!("../assets/logo.svg"))` is an SVG or PNG file you ship, drawn the same way, and `symbol(..)` draws only its shape, in the text's tone like an `icon` ([D-53](decisions.md)). Use them for what no `Icon` covers, such as a product's logo. `cover(url, fallback)` is a picture a program hands over at runtime by URL, like a track's cover art: the host loads `file://` and `https://` PNGs and JPEGs off the UI thread, crops them square, rounds them, and shows `fallback` until the picture is there or if it can't be had. `.large()` makes it a popover's main picture ([D-57](decisions.md)). `expand(collapsed, expanded)` shows `expanded` once the pointer rests on it, fading like `hover`. Unlike `hover`, it grows to fit, with the island springing along, and only the pointer over it counts, not the whole item. `button(...).on_secondary(message)` sends a message on a right click on that button. `gulp(content)` keeps swelling its content gently, one gulp after another, like a bottle being poured, for something filling up. Popovers have their own pieces ([D-40](decisions.md)), laid out at one width the host picks: `header(title)` (with `.on_back(message)` for a sub-page, `.trailing(node)` at its end), `section(title)` (a small caption, with `.trailing`), `list_item(title)` (with `.subtitle`, `.leading`, `.trailing`, `.selected(bool)`, `.on_press`), `list(rows)` (scrolls past a few rows), `detail(name, value)`, `fill()` (pushes what follows to a row's end), `center(node)` (centers it across the panel; text in it keeps to one line and is cut if it's too long), `slider(value, range, on_change)` (with `.step`, `.on_release`), `toggle(on, message)`, `meter(fraction)` (with `.limit(at)`), `text_input(placeholder, value, on_input)` (with `.on_submit`, `.secret(bool)`, `.trailing`; it takes the focus, and keys stay in the popover while it has it, [D-41](decisions.md)), `display(node)` (large text), `mark(node)` (ringed: today in a calendar), `badge(text)`, `tabs([(name, page), …])` (pages behind a strip of names; the host shows the first when the popover opens and switches when one is clicked, so you keep no state for it, [D-43](decisions.md)), and `button(...).prominent()` for a page's main action. A `button` holding only an `icon`, `image` or `symbol` is drawn as a square icon button, or a filled disc when prominent. A `heading` in a popover keeps to one line, cut at the edge if it's too long. `Panel::new(..).on_open(message).on_close(message)` tells you when the popover opens and starts closing: subscribe to chatty services only while it's open, and forget anything typed when it closes. If you need something it can't express, propose a new primitive in eave rather than working around it ([D-13](decisions.md)).
- **Notifications go over the bus, never through `notices()`.** Send them with `Notify`, as above. `notices()` is only for a notification engine: the plugin that serves `org.freedesktop.Notifications` lists there what it received, for the host to draw ([D-52](decisions.md)). It lists the notifications to show on screen now, newest first, each a `Notice::new(key, summary)` with `.app(name)`, `.app_icon(id)`, `.body(text)`, `.urgency(Urgency::…)`, `.action(label, message)`, `.on_press(message)`, `.on_dismiss(message)` (gives it a close button), `.expires(after, message)` and `.reply(Reply::new(placeholder, value, on_input).on_submit(..).on_cancel(..))`: a field to type an answer into. While a notice shows one, the bar has the keyboard and nothing expires, so show it only once asked, after a click on a Reply action ([D-49](decisions.md)). While there are any, the host turns the right island into a panel of them, like a popover, keyed by `key`: a new key comes in, a missing one leaves. It counts `expires` only while the panel is on screen and not pointed at, then sends the message. Drop the notice from your list when it's dismissed or expires ([D-47](decisions.md)).

### Clicks and popovers

| Item has `on_press` | `popover()` returns `Some` | Primary click |
|---|---|---|
| no | no | nothing (no pointer cursor) |
| no | yes | toggles the popover |
| yes | either | sends your message (you can still return `Effect::none().open_popover()`) |

A right click sends `on_secondary`, if set. `Item::on_scroll(up, down)` sends one message per scroll step. The host turns touchpad scrolling into steps. Buttons inside the item take their own clicks first. Return `Item::hidden()` to take no space at all (a battery widget on a desktop).

### Nudges

Return `Effect::none().nudge(Nudge::…)` from `update` when something your item shows just went one way, and the host gives your island a nudge that way. You say what happened, and the host picks the motion ([D-32](decisions.md)):

| Nudge | Use it when | The island |
|---|---|---|
| `Up`, `Down` | a level rose or fell (volume, brightness) | its top edge bumps up above your item, or its bottom edge dips below it, then springs flat |
| `Left`, `Right` | you moved to the previous or next thing (workspace, track) | the whole island is pushed that way and springs back, your item a little further |

If the change you're following plays out on screen over a known time, like the compositor sliding a workspace in, return `Effect::none().nudge_lasting(nudge, duration)` instead: the host plays the same motion, just as far, stretched to settle over about that long. `workspaces` does this with Hyprland's `workspacesIn` animation.

Nudge on the change itself, wherever it came from, and once per change. `volume` nudges on the server's readings, so a scroll step and a media key look the same. A nudge is dropped while the island is held or has a popover open. A nudge also brings a collapsed item out for a moment, so the change is seen ([D-58](decisions.md)).
