# Vision

## The name

An **eave** is the edge of a roof, the strip along the top of a house. The bar is the strip along the top of the screen. Birds perch under eaves, which pairs it with [perch](https://github.com/francescomengarelli/perch), the dotfiles manager.

## What we are building

A status bar for Hyprland: a thin surface pinned to a screen edge that reserves its own space and shows live system state. That means workspaces, the focused window, a clock, battery, audio, network, media, and a system tray. Later it may grow into more shell components (OSD, notifications, launcher), built on the same foundations.

It is a personal tool first. It is built for one person's daily use on a MacBook running Asahi Linux, and must also work on a CachyOS desktop.

## Why build one at all

Waybar works. The reasons to replace it:

- **Feel.** Existing bars are mostly static. We want motion: workspace indicators that glide, values that ease in, all locked to the display's frame rate.
- **Ownership.** Configuring someone else's bar caps what you can do. Writing our own means any idea is possible, and we understand why it behaves the way it does.
- **Learning.** Wayland protocols, D-Bus, PipeWire, rendering and async Rust are all worth understanding properly. A bar touches all of them in a small, contained project.

## Philosophy

### Smooth beats feature-rich
One animation that runs perfectly is worth more than five modules that stutter. Frames are paced by the compositor (Wayland frame callbacks), never by our own timers. If a feature can't be smooth, it waits until it can be.

### Event-driven, always
The bar is idle almost all the time. It wakes up only when the world changes: Hyprland emits an event, a D-Bus property changes, PipeWire reports a new volume. Polling is a last resort, used only for things that have no event source (CPU and memory counters in `/proc`). When nothing changes and nothing animates, the bar does no work and draws no frames.

### Lightweight is measured, not claimed
"No bloat" means numbers: memory (RSS), idle CPU and binary size, recorded in [development.md](development.md#measuring). Every dependency costs compile time, binary size, attack surface and upgrade pain, so each one needs a reason. Feature flags default to off. When two approaches look equal, we measure them. That is how the CPU renderer became the default (see [decisions.md](decisions.md)).

### From scratch, but not reinventing everything
"From scratch" means we own the architecture and the data layer. It does not mean writing a font rasterizer. We use solid building blocks (iced for UI and layout, zbus for D-Bus, the layer-shell binding) and write ourselves the parts that are small and central to the bar. A Hyprland IPC client is about 100 lines, so we write it instead of pulling in a crate.

### Empty by default, everything is a plugin
With no plugins configured, the bar draws nothing. The clock, the battery and the workspaces are all plugins written against the same public API a third party would use. That keeps the API honest: if a built-in widget needs something, every plugin gets it too.

### Simple code over clever code
The Elm-style architecture (`Message → update → view`) keeps all state in one place and every change explicit. Modules should read like the code around them. No abstraction until there's a second user of it.

### Write down the why
Decisions record their reasons and the alternatives we rejected, so a future change can tell whether those reasons still hold.

## Non-goals

- **Supporting every compositor.** Hyprland is the target. Other wlroots compositors may work through layer-shell, but we won't add code for them.
- **X11.** Not supported, and it's disabled at build time.
- **A config language.** Configuration is a TOML file that says which widgets go where, plus each widget's own options. No scripting, no CSS.
- **Runtime-loaded plugins.** Plugins exist, and anyone can publish one as a git repository, but they are compiled into the binary by `eave-build`. No dynamic libraries, no WASM, no scripting runtime ([D-12](decisions.md)). *(This item used to read "a config language or plugin system for other people". The plugin half was reversed on 2026-10-12: see D-12.)*
- **Plugin-controlled styling or layout.** Plugins describe *what* to show with a closed vocabulary. The host decides how it looks and where it goes ([D-11](decisions.md), [D-13](decisions.md)).
- **Matching Waybar feature for feature.** Modules get added when they're actually wanted.
- **Distro packaging beyond Arch.** We may write a `PKGBUILD`. Nothing else is planned.
