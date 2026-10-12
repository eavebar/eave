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
| [docs/architecture.md](docs/architecture.md) | How the bar works: host / widgets / services, the event loop, popovers, code layout |
| [docs/plugins.md](docs/plugins.md) | Using and writing plugins: plugins.toml, config.toml, the Widget trait, the manifest |
| [docs/running.md](docs/running.md) | Starting the bar (systemd user service or Hyprland config), config reload, rebuild and restart |
| [docs/decisions.md](docs/decisions.md) | Every significant choice, with the alternatives and *why* |
| [docs/development.md](docs/development.md) | Build, run, measure, and the rules for writing code here |
| [docs/roadmap.md](docs/roadmap.md) | What exists, what is next, in what order |

## Quick facts

- Stack: **Rust + iced 0.14 + iced_layershell 0.19**, with the **tiny-skia** (CPU) renderer by default.
- **Everything is a plugin.** With no `~/.config/eave/config.toml` the bar is empty. Plugins are crates compiled in by `eave-build`, never loaded at runtime ([docs/plugins.md](docs/plugins.md)).
- `config.toml` reloads live. `plugins.toml` needs `eave-build --restart`. The bar runs best as a systemd user service ([docs/running.md](docs/running.md)).
- Run it: `cargo run --release`. GPU renderer: `cargo run --release --features gpu`. Tests: `cargo test --workspace`.
- Code: `crates/eave-api` (plugin SDK: `Widget`, closed `ui` vocabulary), `crates/eave` (host: surfaces, layout, theme, popovers), `crates/eave-build` (builds a binary with third-party plugins), `crates/eave-{hyprland,dbus,pulse}` (shared services), `plugins/{clock,workspaces,window,keyboard,battery,network,volume}`.
- Baseline: ~11.8 MB RSS with two clocks and ~0% idle CPU (see [docs/development.md](docs/development.md#measuring)). Don't regress it without a reason written down in `docs/decisions.md`.

## Rules for agents

- **No polling** when an event source exists. Use Hyprland socket2, D-Bus signals and audio server subscriptions. `/proc` and `/sys` counters are the only things polled.
- **Never block the UI thread.** All I/O goes through iced subscriptions or tasks.
- **Every new dependency needs a reason.** Prefer `default-features = false`. Record non-obvious ones in `docs/decisions.md`.
- **Measure** RSS, idle CPU and binary size before and after any change that might affect them (dependencies, renderer, animation).
- **Keep both architectures building:** aarch64 (Asahi) and x86_64 (CachyOS). No arch-specific code without a fallback.
- **Plugins describe, the host draws.** Never give plugins colors, sizes, raw iced elements or control over layout. Extend the `ui` vocabulary in `eave-api` and render it in `crates/eave/src/kit.rs` instead.
- **Built-in widgets are plugins.** They use only the public `eave-api`. No private shortcuts into the host.
- **Keep the docs true.** If you change a decision, update `docs/decisions.md` and say why. Don't just delete the old reasoning: mark it superseded.

## Git workflow for agents

- **Work in your own worktree and branch.** Never edit on `main` directly. Create a branch and a worktree for it under `.worktrees/` (git ignores that folder), e.g. `git worktree add .worktrees/<branch> -b <branch>`.
- **Merging means cleaning up.** When asked to merge a branch, merge it, then delete the branch both locally and on the remote (`git push origin --delete <branch>`), and remove its worktree (`git worktree remove .worktrees/<branch>`).
- **Check before deleting work that isn't yours.** If asked to merge branches or worktrees you did not create in the current session, first check them for uncommitted changes (`git -C <worktree> status`) and for commits that haven't been pushed or merged. If you find any, stop and ask the user what to do. Don't delete anything until they answer.
