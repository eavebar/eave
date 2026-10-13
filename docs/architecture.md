# Architecture

## The two halves of a bar

A bar does two separate jobs:

1. **Being a bar-shaped surface.** On Wayland a normal window gets tiled by the compositor. A bar instead uses the **`wlr-layer-shell`** protocol to anchor to a screen edge, sit on a layer (`background`, `bottom`, `top` or `overlay`), and reserve an **exclusive zone** so tiled windows stay out of its space. `iced_layershell` handles this for us.
2. **Knowing what to show.** Every widget reads a different system interface. This is where most of the work is.

eave keeps these apart. The **host** does the first job and owns everything visual. **Widgets** (plugins) do the second and only describe content. With no widgets configured, the bar draws nothing (it still reserves its space).

## Three layers

```
┌──────────────────────────────── host (crates/eave) ────────────────────────────────┐
│ surfaces: bars (popovers grow out of islands) · zones · theme · kit (nodes → pixels)│
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
- **Services** are shared event sources, as ordinary public crates that plugins depend on. `crates/eave-hyprland` is the first: every widget that watches Hyprland shares one socket2 connection ([D-17](decisions.md)). `crates/eave-net` holds one netlink listener for links, addresses, routes and Wi-Fi ([D-23](decisions.md)). `crates/eave-dbus` shares one system bus connection ([D-18](decisions.md)). `crates/eave-pulse` holds the one audio event subscription ([D-19](decisions.md)). `crates/eave-iwd` controls Wi-Fi through iwd, watched only while the network popover is open ([D-42](decisions.md)).

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
- A widget's `update` returns an `Effect`: a `Task` for async work, plus optional requests to open or close its popover, or to nudge its island. `update` never does I/O.
- `view(window)` is pure. For the bar it lays out the left, center and right zones from `config.toml`, each as a squircle island only as wide as its items. A zone with nothing to show has no island ([D-24](decisions.md)). When items come or go, the island morphs: its edges spring to the new size, items slide to their new places and new ones fade in ([D-27](decisions.md)). Pressing an island squeezes it; dragging stretches it toward the pointer with a rubber band, most where it was grabbed, and it springs back on release ([D-26](decisions.md), [D-45](decisions.md)). An open popover stretches the same way when a press on it isn't a control's. A widget's nudge pushes the island a little that way and pokes its outline, a jelly of springs that smooths the poke out by itself: the edge bulges at the item, and the island swells on the side it's going toward ([D-32](decisions.md)). Each item is wrapped in a `Slot`, which turns unhandled clicks and scroll steps into messages. An open popover's `Panel` is drawn by the island it grew out of.
- Nothing is drawn when no message arrives. Pointer tracking is only subscribed while a popover is open, so hovering over the bar sends the app no messages. The pointer entering or leaving an item does redraw it: its `Slot` tells the alternates inside, which fade to their hover option and back ([D-35](decisions.md)). Moving within an item, or resting on it, draws nothing.

## Popovers

A popover isn't a surface of its own: the island holding the item turns into it ([D-28](decisions.md)). A click on an item that has a popover and no `on_press`:

1. The host records which bar was clicked and marks the popover open. `view` hands the panel to that zone's island.
2. The island lays the panel out at its natural size in an iced overlay and places it: top edge on the island, growing from the island's left edge, right edge or center (by zone), never narrower than the island, `EDGE_GAP` from the screen sides. It reports the place (`PopoverPlaced`).
3. The bar surface is already a little taller than its exclusive zone, for drags ([D-45](decisions.md)). The host grows it to fit (`SizeChange`, exclusive zone unchanged, so windows don't move) and sets an input region of the bar strip plus the panel, so the pointer still reaches windows beside and below it.
4. Once the surface is tall enough, the island morphs into the panel: its edges spring to the panel's, its items fade out, and the panel's content fades in once the shape is halfway there. It squishes as it sets off, squeezed in from the sides, and eases back as it grows ([D-44](decisions.md)).
5. It closes on a second click on the item, when the widget returns `close_popover()` or stops offering a popover, on a click anywhere else, or on any key press: the bar takes the keyboard only while a popover is open ([D-36](decisions.md)). While a popover is open, a non-consuming Hyprland bind reports every left click on socket2, and the host ignores the ones that land on the bar. Clicks on the bar are handled by the bar itself, and the island captures presses on its panel ([D-14](decisions.md)). Closing runs the morph backwards, and the island splats against the top as it lands ([D-44](decisions.md)). When the island is back, it reports `PopoverClosed` and the host shrinks the surface again.

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
  src/app.rs               state, Message, update/view/subscription, popover lifecycle, surface fit, reload
  src/config.rs            config.toml → zone entries
  src/watch.rs             inotify watch on the config directory
  src/kit.rs               ui nodes → iced elements, styled
  src/icon.rs              status icons (volume, Wi-Fi, wired, battery) as vector paths
  src/apps.rs              app icons: desktop entry → icon theme → 32 px bitmap, off the UI thread
  src/theme.rs             every color, size and font
  src/island.rs            island: item row, morph springs, press squeeze and drag, popover overlay
  src/squish.rs            how squished the items being drawn are, set by their island
  src/label.rs             a label that can squish (outlines while squished)
  src/squircle.rs          the squircle path
  src/jelly.rs             an island's outline as a ring of coupled springs, which nudges poke
  src/slot.rs              item wrapper: clicks, scroll steps, and the pointer for alternates
  src/alternate.rs         alternate: one option at a time with fades; hover, and growing (expand)
  src/switcher.rs          switcher: row of options with a sliding, frame-paced mark
  src/tabs.rs              tabs: a pill strip of names over the current page, which the host owns
  src/toggle.rs            toggle: a pill whose knob slides over
  src/meter.rs             meter: a thin bar eased to its value, with an optional limit tick
  src/field.rs             text field box, its focus on appearing, and the focus query popovers use
  src/clip.rs              a line of text cut at its room, fading at the end
  src/ease.rs              a number eased to each new value on the frame clock
crates/eave-build/         builds the widgets config.toml uses, installs and restarts; `update`, `init`
crates/eave-dbus/          service: shared system bus, snapshot-on-PropertiesChanged watcher
crates/eave-iwd/           service: Wi-Fi networks and controls through iwd's D-Bus API, and a passphrase agent
crates/eave-net/           service: network state from the kernel over netlink (one event thread)
crates/eave-pulse/         service: audio over the PulseAudio protocol (one event thread)
crates/eave-hyprland/      service: shared socket2 events, socket1 queries and dispatches
eave-collection.toml       lists plugins/*: the built-ins, as one collection (D-46)
plugins/clock/             the clock plugin (crate eave-clock) + its eave-plugin.toml
plugins/workspaces/        Hyprland workspaces (eave-workspaces)
plugins/window/            focused window title (eave-window)
plugins/keyboard/          keyboard layout (eave-keyboard)
plugins/battery/           battery from sysfs (eave-battery)
plugins/network/           primary connection, via eave-net; Wi-Fi to join via eave-iwd (eave-network)
plugins/volume/            output volume and switching (eave-volume)
vendor/softbuffer/         softbuffer 0.4.8 with an alpha-carrying Wayland buffer (D-25)
dist/systemd/              user units: the bar, and the rebuild-on-config.toml pair
dist/config.toml           the starter config `eave-build init` writes
install.sh                 the curl | sh installer
```

The rule: **split a file when it gets a second responsibility, not before.** Each service is its own crate (`eave-hyprland`, later a D-Bus one), so a plugin only compiles in the sources it uses.

## Things to keep in mind

- `#[to_layer_message(multi)]` on `Message` adds the variants `iced_layershell` uses to create and control surfaces (`SizeChange`, `SetInputRegion`, `NewLayerShell`, `RemoveWindow`, …). `update` must handle them with a catch-all arm.
- Margin tuples are `(top, right, bottom, left)`, despite the macro's doc comment.
- Surfaces are transparent. The islands (popovers included) draw their own backgrounds, which is what gives them rounded corners and lets the wallpaper show around the islands. With the CPU renderer that needs our patched softbuffer ([D-25](decisions.md)).
- `KeyboardInteractivity::None`, except while a popover is open: then its bar takes the keyboard (`Exclusive`), any key closes the popover, and keys stay with a focused text field in it ([D-36](decisions.md), [D-41](decisions.md)).
- The bar opens on the active output (`StartMode` default). In daemon mode, every window is drawn as a bar, and popovers remember which bar they grew on, so one bar per monitor (`StartMode::AllScreens`) mostly needs per-output state.
