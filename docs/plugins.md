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

The zones are `left`, `center` and `right`, in order. Every key except `widget` goes to the widget's `Config`. Unknown widgets and invalid options are reported on stderr and skipped. They never crash the bar.

Changes apply as soon as you save, with no restart. Widgets whose entry didn't change keep their state. A file that doesn't parse leaves the bar as it was ([D-17](decisions.md)).

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

### The rules

- **Views are pure and cheap.** `item()` runs on every redraw, and `popover()` runs after every message to your widget. Precompute in `update`.
- **`update` never blocks.** I/O goes in a `Task` (`Task::perform`) or a `Subscription`.
- **Events, not polling.** Use `eave_api::time::every` only when there's no event source (see [development.md](development.md#code-rules)).
- **You describe, the host draws.** The vocabulary is `label`, `heading`, `row`, `column`, `grid`, `button`, `separator`, `empty`, plus `.tone(Tone::…)`. If you need something it can't express, propose a new primitive in eave rather than working around it ([D-13](decisions.md)).

### Clicks and popovers

| Item has `on_press` | `popover()` returns `Some` | Primary click |
|---|---|---|
| no | no | nothing (no pointer cursor) |
| no | yes | toggles the popover |
| yes | either | sends your message (you can still return `Effect::none().open_popover()`) |

A right click sends `on_secondary`, if set. Buttons inside the item take their own clicks first. Return `Item::hidden()` to take no space at all (a battery widget on a desktop).
