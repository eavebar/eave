# AGENTS.md

**eave** is a status bar for Hyprland, written from scratch in Rust. It plays the same role as Waybar: a strip along the screen edge with workspaces, clock, battery, volume, media and tray. It is built around three non-negotiables:

1. **Smooth.** Animations run in sync with the compositor's frame clock and never stutter.
2. **Reactive.** State changes come in as events. The bar redraws only when something actually changed.
3. **Lightweight.** No bloat. Every dependency, feature flag and allocation must earn its place, and we measure instead of guessing.

We build it ourselves, not on a framework like Quickshell or AGS, because we want to own and understand every layer: the Wayland surface, the event sources, the rendering. When a goal conflicts with convenience, the goal wins.

## Read before changing anything

| Doc | What it covers |
|---|---|
| [docs/vision.md](docs/vision.md) | What we are building, the philosophy, what we will *not* do |
| [docs/platform.md](docs/platform.md) | Targets (Hyprland, Asahi aarch64, CachyOS x86_64) and their quirks |
| [docs/architecture.md](docs/architecture.md) | How the bar works: layer shell, the event loop, data sources, code layout |
| [docs/decisions.md](docs/decisions.md) | Every significant choice, with the alternatives and *why* |
| [docs/development.md](docs/development.md) | Build, run, measure, and the rules for writing code here |
| [docs/roadmap.md](docs/roadmap.md) | What exists, what is next, in what order |

## Quick facts

- Stack: **Rust + iced 0.14 + iced_layershell 0.19**, with the **tiny-skia** (CPU) renderer by default.
- Run it: `cargo run --release`. GPU renderer: `cargo run --release --features gpu`.
- Code: `src/main.rs` (layer-shell setup), `src/app.rs` (state, messages, view).
- Baseline: ~11.5 MB RSS and ~0% idle CPU (see [docs/development.md](docs/development.md#measuring)). Don't regress it without a reason written down in `docs/decisions.md`.

## Rules for agents

- **No polling** when an event source exists. Use Hyprland socket2, D-Bus signals and PipeWire listeners. `/proc` and `/sys` counters are the only things polled.
- **Never block the UI thread.** All I/O goes through iced subscriptions or tasks.
- **Every new dependency needs a reason.** Prefer `default-features = false`. Record non-obvious ones in `docs/decisions.md`.
- **Measure** RSS, idle CPU and binary size before and after any change that might affect them (dependencies, renderer, animation).
- **Keep both architectures building:** aarch64 (Asahi) and x86_64 (CachyOS). No arch-specific code without a fallback.
- **Keep the docs true.** If you change a decision, update `docs/decisions.md` and say why. Don't just delete the old reasoning: mark it superseded.
