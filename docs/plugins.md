# Plugins

Everything in the bar is a plugin, including the clock. A plugin is an ordinary Rust crate that implements `eave_api::Widget`. It lives in a git repository and gets compiled into the bar by `eave-build`. Nothing is loaded at runtime ([D-12](decisions.md)).

## Using plugins

`~/.config/eave/plugins.toml` lists what to compile in:

```toml
# Where eave itself comes from. Defaults to the main repository.
eave = { path = "~/code/fran/eave" }          # or { git = "…", rev = "…" }

[[plugin]]                                     # the built-in clock, from the eave checkout
path = "~/code/fran/eave"
dir = "plugins/clock"

[[plugin]]
git = "https://github.com/someone/eave-weather"
rev = "v0.2.0"                                 # commit, tag or branch; default branch if omitted
```

Then:

```fish
cargo run --release -p eave-build -- --install   # builds and copies to ~/.local/bin/eave
cargo run --release -p eave-build -- --restart   # same, then restarts the running bar on it
```

A rebuild is needed only when `plugins.toml` changes. A systemd path unit can do it for you on every save ([running.md](running.md#rebuild-automatically-when-pluginstoml-changes)).

Sources are cloned to `~/.cache/eave/src`, and the generated crate builds in `~/.cache/eave/build`, so rebuilds are incremental. The plain `cargo run --release` in this repo builds the default binary, which has only the built-in plugins.

`~/.config/eave/config.toml` decides what appears where:

```toml
[[center]]
widget = "clock"

[[right]]
widget = "clock"          # the same widget twice, with different options
format = "%a %d %b"
```

The zones are `left`, `center` and `right`, in order. Every key except `widget` and `width_group` goes to the widget's `Config`.

`width_group = "name"` lines items up: items in the same island with the same group all get a slot as wide as the widest of them, each centered in its slot. For example, put `volume` and `network` in one group, so the Wi-Fi icon gets the same room as the volume item, which needs space for its percent. Unknown widgets and invalid options are reported on stderr and skipped. They never crash the bar.

Changes apply as soon as you save, with no restart. Widgets whose entry didn't change keep their state. A file that doesn't parse leaves the bar as it was ([D-21](decisions.md)).

## Built-in plugins

They ship in the default binary, and with `eave-build` you list them like the clock above (`dir = "plugins/<name>"`).

| Widget | Shows | Options |
|---|---|---|
| `clock` | time, calendar in the popover | `format` (strftime, default `%H:%M`) |
| `workspaces` | Hyprland workspaces that have windows, plus the active one (marked); click to go, scroll to move | `persistent` (workspaces 1..N always shown, even empty, default 0) |
| `window` | focused window title, hidden when nothing has focus | `max_chars` (default 60) |
| `battery` | battery icon filled to the charge (bolt while charging) and the percent, warning/critical tones; time left and power draw in the popover; hidden without a battery | `warning` (default 20), `critical` (default 10) |
| `network` | Wi-Fi icon with as many bars as the signal, wired icon, struck-through Wi-Fi when offline; name, signal, band, address, gateway in the popover | `show_name` (default false): the Wi-Fi name next to the icon |
| `volume` | speaker icon with 0–3 waves for the volume, struck through when muted; the percent for 1.5 s after a change, with a nudge; scroll to change, right click to mute; −/+, mute, output list in the popover | `step` (default 5), `max` (default 100) |
| `keyboard` | active layout, click for the next one; hidden with a single layout | `aliases` (`{ "English (US)" = "us" }`) |

```toml
[[left]]
widget = "workspaces"
persistent = 5

[[left]]
widget = "window"

[[right]]
widget = "keyboard"
aliases = { "English (US)" = "us", "Italian" = "it" }
```

## Writing a plugin

```toml
# Cargo.toml
[dependencies]
eave-api = "0.1"
serde = { version = "1", features = ["derive"] }
```

`eave-api` isn't published on crates.io yet. `eave-build` patches it to the eave checkout it builds against. To work on a plugin on its own, add this to the plugin's own `Cargo.toml` (cargo ignores it when the plugin is used as a dependency):

```toml
[patch.crates-io]
eave-api = { path = "../eave/crates/eave-api" }
```

```toml
# eave-plugin.toml, at the repository root
[plugin]
name = "weather"
description = "Current weather, forecast in the popover"
crate = "eave-weather"   # package name
path = "."               # crate directory, relative to this file
api = "0.1"              # eave-api version you target (cargo caret rules)
```

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

For D-Bus, depend on `eave-dbus`: `eave_dbus::watch(service, snapshot)` yields a snapshot of the service's state, then a new one after each `PropertiesChanged` on the objects the snapshot depends on. Unchanged snapshots are skipped. `properties(...)` reads every property of an interface in one call.

For the network, depend on `eave-net`: `watch()` yields the primary connection (interface, Wi-Fi SSID and signal, address, gateway) after every change. It reads the kernel directly, so it works whichever network manager is installed.

For audio, depend on `eave-pulse`: `watch()` yields the outputs (sinks, default, volume, mute) after every change, and `set_volume`, `set_mute` and `set_default` send commands. Run them in a `Task`.

### The rules

- **Views are pure and cheap.** `item()` runs on every redraw, and `popover()` runs after every message to your widget. Precompute in `update`.
- **`update` never blocks.** I/O goes in a `Task` (`Task::perform`) or a `Subscription`.
- **Events, not polling.** Use `eave_api::time::every` only when there's no event source (see [development.md](development.md#code-rules)). To show something for a moment, return `Task::future(eave_api::time::sleep(d))` mapped to a message, numbered so that only the latest one counts.
- **You describe, the host draws.** The vocabulary is `label`, `heading`, `icon`, `counter`, `row`, `column`, `grid`, `button`, `switcher`, `alternate`, `separator`, `empty`, plus `.tone(Tone::…)`. `icon(Icon::…)` is a status symbol (volume, Wi-Fi, wired, battery) that says how full it is, and the host draws it in the text's tone. `switcher(options, current)` is a row of choices with one marked current, and the host slides the mark when it changes. `counter(value, range, unit)` is a number the host counts up or down through every value in between when it changes. It keeps room for the widest number in `range`, so it holds its width. `alternate(options, current)` shows one option at a time, as big as the biggest one, and fades between them, e.g. an icon that turns into a `counter` for a moment. If you need something it can't express, propose a new primitive in eave rather than working around it ([D-13](decisions.md)).

### Clicks and popovers

| Item has `on_press` | `popover()` returns `Some` | Primary click |
|---|---|---|
| no | no | nothing (no pointer cursor) |
| no | yes | toggles the popover |
| yes | either | sends your message (you can still return `Effect::none().open_popover()`) |

A right click sends `on_secondary`, if set. `Item::on_scroll(up, down)` sends one message per scroll step. The host turns touchpad scrolling into steps. Buttons inside the item take their own clicks first. Return `Item::hidden()` to take no space at all (a battery widget on a desktop).

### Nudges

Return `Effect::none().nudge(Nudge::…)` from `update` when something your item shows just went one way, and the host gives your island a nudge that way. You say what happened, and the host picks the motion ([D-30](decisions.md)):

| Nudge | Use it when | The island |
|---|---|---|
| `Up`, `Down` | a level rose or fell (volume, brightness) | its top edge bumps up above your item, or its bottom edge dips below it, then springs flat |
| `Left`, `Right` | you moved to the previous or next thing (workspace, track) | the whole island is pushed that way and springs back |

Nudge on the change itself, wherever it came from, and once per change. `volume` nudges on the server's readings, so a scroll step and a media key look the same. A nudge is dropped while the island is held or has a popover open.
