# Architecture

## The two halves of a bar

A bar does two separate jobs:

1. **Being a bar-shaped surface.** On Wayland a normal window gets tiled by the compositor. A bar instead uses the **`wlr-layer-shell`** protocol to anchor to a screen edge, sit on a layer (`background`, `bottom`, `top` or `overlay`), and reserve an **exclusive zone** so tiled windows stay out of its space. `iced_layershell` handles this for us.
2. **Knowing what to show.** Every module reads a different system interface. This is where most of the work is.

## Data flow

```
 event sources                     iced runtime                         compositor
┌────────────────────┐   Message  ┌──────────────┐  view()  ┌────────┐ frame cb ┌──────────┐
│ Hyprland socket2   │──────────▶ │ update(state)│────────▶ │ layout │ ───────▶ │ Hyprland │
│ D-Bus signals      │──────────▶ │              │          │ render │ ◀─────── │          │
│ PipeWire listener  │──────────▶ │   App state  │          └────────┘          └──────────┘
│ timers (/proc,clock)│─────────▶ └──────────────┘
└────────────────────┘
```

- Each source is an **iced `Subscription`**: an async stream that turns external events into `Message`s.
- `update` is the **only** place state changes. It's synchronous and fast, and it never does I/O. If something must happen in response (for example a command sent to Hyprland), `update` returns a `Task`.
- `view` is a pure function of state. iced decides when to redraw. With no messages and no running animation, nothing is drawn.
- Animations are state too: a start value, a target and a start time. While one is running, we request frames, so pacing follows the compositor's frame callbacks. Once it settles, the requests stop.

## Data sources by module

| Module | Source | Mechanism |
|---|---|---|
| Workspaces, active window, monitors | Hyprland | `.socket2.sock` events + `.socket.sock` queries (our own client) |
| Clock | system time | 1 s timer |
| Battery | UPower | D-Bus (`zbus`), `PropertiesChanged` signals |
| Volume | PipeWire / WirePlumber | `pipewire` crate listener |
| Media | MPRIS | D-Bus (`zbus`) |
| Network | NetworkManager or iwd | D-Bus (`zbus`) |
| Bluetooth | BlueZ | D-Bus (`zbus`) |
| Tray | StatusNotifierItem | D-Bus. We must act as the watcher/host. The hardest module. |
| CPU / RAM / temps | `/proc`, `/sys` | timer (the only legitimate polling) |

So far only the clock exists. Hyprland is next. See [roadmap.md](roadmap.md).

## Code layout

```
src/
  main.rs   entry point: layer-shell settings (anchor, layer, size, exclusive zone), runs the app
  app.rs    App state, Message enum, update / view / subscription / style
```

The planned layout as modules arrive:

```
src/
  main.rs
  app.rs            top-level state + routing of messages to modules
  hyprland/         IPC client (socket2 stream, socket queries), event types
  modules/          one file per bar module: its state, message, view fragment, subscription
  services/         shared D-Bus connection, PipeWire, etc. (when >1 module needs them)
  theme.rs          colors, spacing, fonts in one place
```

The rule: **split a file when it gets a second responsibility, not before.**

## Things to keep in mind

- `#[to_layer_message]` on `Message` adds variants that `iced_layershell` uses to control the surface at runtime (resize, re-anchor, margins). `update` must handle them with a catch-all arm.
- `KeyboardInteractivity::None`: the bar must never steal keyboard focus. Popups that need typing (a launcher, say) will be separate surfaces with their own settings.
- The bar opens on the active output for now (`StartMode` default). For one bar per monitor we'll switch to `iced_layershell::daemon` with one surface per output.
