# Architecture

## The two halves of a bar

A bar does two separate jobs:

1. **Being a bar-shaped surface.** On Wayland a normal window gets tiled by the compositor. A bar instead uses the **`wlr-layer-shell`** protocol to anchor to a screen edge, sit on a layer (`background`, `bottom`, `top` or `overlay`), and reserve an **exclusive zone** so tiled windows stay out of its space. `iced_layershell` handles this for us.
2. **Knowing what to show.** Every widget reads a different system interface. This is where most of the work is.

eave keeps these apart. The **host** does the first job and owns everything visual. **Widgets** (plugins) do the second and only describe content. With no widgets configured, the bar is an empty strip.

## Three layers

```
┌──────────────────────────────── host (crates/eave) ────────────────────────────────┐
│ surfaces: bar + popovers · zones from config.toml · theme · kit (nodes → pixels)   │
│ click routing · popover lifecycle · animation                                      │
└───────────────▲───────────────────────────────────────────────▲────────────────────┘
                │ Item / Panel (closed vocabulary)               │ Effect (task, open/close popover)
┌───────────────┴───────────── widgets (plugins) ────────────────┴────────────────────┐
│ clock · battery · workspaces · …   each: Config, state, Message, update, views      │
└───────────────▲────────────────────────────────────────────────────────────────────┘
                │ Subscription
┌───────────────┴───────────── services ─────────────────────────────────────────────┐
│ Hyprland IPC · kernel (netlink, sysfs) · D-Bus (MPRIS, SNI) · PipeWire · timers    │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- **Host** ([D-11](decisions.md)): owns every surface, the layout, the look and the motion. It is not pluggable.
- **Widgets** implement `eave_api::Widget`. They can't pick colors or sizes, can't build raw iced elements, and can't move other items or the bar ([D-13](decisions.md)).
- **Services** are shared event sources, as ordinary public crates that plugins depend on. `crates/eave-hyprland` is the first: every widget that watches Hyprland shares one socket2 connection ([D-17](decisions.md)). `crates/eave-net` holds one netlink listener for links, addresses, routes and Wi-Fi ([D-23](decisions.md)). `crates/eave-dbus` shares one system bus connection ([D-18](decisions.md)). `crates/eave-pulse` holds the one audio event subscription ([D-19](decisions.md)).

Core plugins depend only on Hyprland and the kernel, plus an audio server for volume. They don't need UPower, NetworkManager or any other daemon or binary to be installed ([D-23](decisions.md)).

## Data flow

```
 event sources                       host (iced daemon)                                 compositor
┌───────────────────┐  AnyMessage   ┌─────────────────────────┐ view(window) ┌────────┐ frame cb ┌──────────┐
│ widget            │─────────────▶ │ update: route to widget │────────────▶ │ layout │ ───────▶ │ Hyprland │
│ subscriptions     │  (id, msg)    │ instance, apply Effect  │              │ render │ ◀─────── │          │
│ pointer, windows, │─────────────▶ │                         │              └────────┘          └──────────┘
│ config.toml       │               │                         │
└───────────────────┘               └─────────────────────────┘
```

- Each widget instance's `subscription()` is tagged with the instance's stable id and mapped to `Message::Widget(id, AnyMessage)` ([D-15](decisions.md)). `update` hands the message to that instance only. Messages for an id that no longer exists (removed by a reload) are dropped.
- `config.toml` is watched with inotify. When it changes, the host re-reads it and rebuilds only the instances whose entry changed ([D-21](decisions.md)).
- A widget's `update` returns an `Effect`: a `Task` for async work, plus optional requests to open or close its popover. `update` never does I/O.
- `view(window)` is pure. For the bar it lays out the left, center and right zones from `config.toml`. Each item is wrapped in a `Slot`, which turns unhandled clicks and scroll steps into messages and reports the item's bounds. For a popover window it renders the widget's `Panel`.
- Nothing is drawn when no message arrives. Pointer tracking is only subscribed while a popover is open, so hovering over the bar wakes nothing.

## Popovers

A click on an item that has a popover and no `on_press`:

1. The host records the item's bounds and the bar width, then asks for a new layer surface: `overlay` layer, anchored top-left, zero exclusive zone (so it sits under the bar), 1×1, namespace `eave-popover`.
2. `Measure` lays the panel out at its natural size, draws nothing yet, and reports the size.
3. The host resizes the surface and sets its margin so it's centered under the item, clamped to the screen. Then the panel becomes visible.
4. It closes on a second click on the item, when the widget returns `close_popover()` or stops offering a popover, or 400 ms after the pointer leaves it. Closing on an outside click is an open issue ([D-14](decisions.md)).

## Data sources by widget

| Widget | Source | Mechanism |
|---|---|---|
| Workspaces, active window, monitors | Hyprland | `.socket2.sock` events + `.socket.sock` queries (our own client) |
| Clock | system time | 1 s timer (`eave_api::time::every`) |
| Battery | kernel (`/sys/class/power_supply`) | `power_supply` uevents over netlink, plus a 30 s re-read while a battery exists ([D-23](decisions.md)) |
| Volume | PipeWire (through pipewire-pulse) | PulseAudio native protocol, pure Rust, subscribed to sink/server events ([D-19](decisions.md)) |
| Media | MPRIS | D-Bus (`zbus`) |
| Network | kernel (rtnetlink, nl80211) | link/address/route multicast events, plus a 10 s signal re-read while on Wi-Fi ([D-23](decisions.md)) |
| Bluetooth | BlueZ | D-Bus (`zbus`) |
| Tray | StatusNotifierItem | D-Bus. We must act as the watcher/host. The hardest widget. |
| CPU / RAM / temps | `/proc`, `/sys` | timer (the only legitimate polling) |

So far: clock, workspaces, active window, keyboard layout, battery, network and volume. Media is next. See [roadmap.md](roadmap.md).

## Code layout

```
Cargo.toml                 workspace, release profile
crates/eave-api/           the plugin SDK: what plugin authors depend on
  src/widget.rs            the Widget trait
  src/ui.rs                the closed vocabulary: Node, Item, Panel, Tone
  src/effect.rs            Effect: a Task plus popover requests
  src/registry.rs          Registry: widget name → constructor
  src/erased.rs            type erasure for the host (doc-hidden)
crates/eave/               the host, as a library (`eave::run`) plus the default binary
  src/main.rs              default binary: host + built-in plugins
  src/lib.rs               run(): registry, layer-shell settings, daemon
  src/app.rs               state, Message, update/view/subscription, popover lifecycle, reload
  src/config.rs            config.toml → zone entries
  src/watch.rs             inotify watch on the config directory
  src/kit.rs               ui nodes → iced elements, styled
  src/theme.rs             every color, size and font
  src/slot.rs              item wrapper: clicks, scroll steps and bounds
  src/switcher.rs          switcher: row of options with a sliding, frame-paced mark
  src/measure.rs           popover sizing
crates/eave-build/         builds a binary with the plugins from plugins.toml, installs, restarts
crates/eave-dbus/          service: shared system bus, snapshot-on-PropertiesChanged watcher
crates/eave-net/           service: network state from the kernel over netlink (one event thread)
crates/eave-pulse/         service: audio over the PulseAudio protocol (one event thread)
crates/eave-hyprland/      service: shared socket2 events, socket1 queries and dispatches
plugins/clock/             the clock plugin (crate eave-clock) + eave-plugin.toml
plugins/workspaces/        Hyprland workspaces (eave-workspaces)
plugins/window/            focused window title (eave-window)
plugins/keyboard/          keyboard layout (eave-keyboard)
plugins/battery/           battery from sysfs (eave-battery)
plugins/network/           primary connection, via eave-net (eave-network)
plugins/volume/            output volume and switching (eave-volume)
dist/systemd/              user units: the bar, and the optional rebuild-on-plugins.toml pair
```

The rule: **split a file when it gets a second responsibility, not before.** Each service is its own crate (`eave-hyprland`, later a D-Bus one), so a plugin only compiles in the sources it uses.

## Things to keep in mind

- `#[to_layer_message(multi)]` on `Message` adds the variants `iced_layershell` uses to create and control surfaces (`NewLayerShell`, `SizeChange`, `MarginChange`, `RemoveWindow`, …). `update` must handle them with a catch-all arm.
- Margin tuples are `(top, right, bottom, left)`, despite the macro's doc comment.
- Surfaces are transparent. The bar and the popover panel draw their own backgrounds, which is what gives popovers rounded corners.
- `KeyboardInteractivity::None` everywhere: the bar and its popovers never steal keyboard focus. Something that needs typing (a launcher) will need its own settings.
- The bar opens on the active output (`StartMode` default). In daemon mode, every window that isn't a popover is drawn as a bar, so one bar per monitor (`StartMode::AllScreens`) mostly needs per-output state.
