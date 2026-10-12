# Decisions

Each entry records what we chose, what else we considered, and **why**. When a decision changes, don't delete it: mark it `Superseded by D-N` and add the new one. The old reasoning tells you what changed.

---

## D-1: Build our own bar instead of configuring an existing one

**Status:** accepted (2026-10-12)

**Alternatives:** configure Waybar; build on **Quickshell** (QML/QtQuick), **AGS/Astal** (TypeScript + GTK4), **eww** or **Fabric**.

**Why:** Quickshell is the strongest of these (smooth, GPU-rendered, lots of built-in services), and it's where much of the Hyprland community has moved. But all of these mean building on a framework whose runtime, abstractions and memory floor we don't control. The goals are smoothness, low weight and understanding every layer ([vision.md](vision.md)), so we build ourselves.

**Cost we accept:** more work, especially for the tray (SNI) and audio, which frameworks hand you for free.

---

## D-2: Rust

**Status:** accepted (2026-10-12)

**Alternatives:** C++ (Waybar's language), C, Python, TypeScript.

**Why:** native performance with no runtime or GC, so the memory floor stays low. Async I/O (tokio) fits an event-driven bar. A good Wayland and D-Bus ecosystem (`smithay-client-toolkit`, `zbus`). Memory safety matters for a long-running process that parses external input (IPC, D-Bus). And the author wants to use it.

---

## D-3: iced + iced_layershell for UI

**Status:** accepted (2026-10-12)

**Alternatives:**
- **gtk4-rs + gtk4-layer-shell:** mature, and text, tray icons and accessibility are already solved. Rejected because it pulls in the full GTK stack, which works against "lightweight". GObject subclassing and `Rc<RefCell<>>` are awkward in Rust. Animations are limited to CSS transitions unless we add libadwaita, which is even more weight.
- **Raw smithay-client-toolkit + tiny-skia/vello + cosmic-text:** the most control and the smallest footprint. Rejected *for now* because we'd have to write layout, widgets and input handling before shipping a single module (roughly 2–3× the work). It stays the escape hatch if iced ever gets in the way of smoothness or weight.
- **Slint, egui:** less established for layer-shell surfaces. egui is immediate-mode, so it redraws continuously by nature, which contradicts "idle = no work".

**Why iced:** it's pure Rust with no GTK or Qt. The Elm architecture (`Message → update → view`) maps exactly onto an event-driven bar. It only redraws when needed. It has animation support and a choice of renderers. It's proven for panels: System76's COSMIC desktop builds its panel on iced with layer-shell. `iced_layershell` (waycrate) is a third-party binding, so we pin its version against iced (0.19 ↔ iced 0.14).

---

## D-4: CPU renderer (tiny-skia) by default, GPU (wgpu) optional

**Status:** accepted (2026-10-12)

**Measured** on Asahi (aarch64), release build, idle bar with clock:

| Renderer | Binary | RSS | Idle CPU |
|---|---|---|---|
| tiny-skia (default) | 5.6 MB | ~11.5 MB | ~0.2% |
| wgpu (`--features gpu`) | 9.5 MB | ~72 MB | ~1% |

**Why:** a bar is a tiny surface (about 1512×32 logical px). Rasterizing it on the CPU is cheap, even at 60 fps during an animation. A GPU context costs about 60 MB of driver and Vulkan mappings, and buys nothing at this size. The `gpu` feature is kept so we can re-measure if animations get heavy (blur, shadows, many simultaneous transitions).

**Revisit if:** animation frames show up as CPU spikes or dropped frames with tiny-skia.

---

## D-5: Minimal iced feature set

**Status:** accepted (2026-10-12)

`iced` uses `default-features = false` with only `tiny-skia`, `wayland`, `tokio` and `crisp`.

**Why:** the defaults include `wgpu`, `x11`, `thread-pool`, `web-colors` and `linux-theme-detection`. We don't support X11 ([vision.md](vision.md#non-goals)). `tokio` is our async runtime, so the thread pool is redundant. Theme detection is pointless since the bar has its own theme. Each removed feature means less to compile, a smaller binary and fewer dependencies.

---

## D-6: Write our own Hyprland IPC client

**Status:** accepted, not yet implemented (2026-10-12)

**Alternative:** the `hyprland` crate (hyprland-rs).

**Why:** the protocol is two Unix sockets with line-based events (`EVENT>>DATA`) and JSON replies. A focused client is about 100 lines, has no extra dependencies, and parses only the events we use. Owning it also means we decide how to handle Hyprland's IPC changes between releases (tolerant parsing, ignore unknown events), instead of waiting on a crate update.

---

## D-7: zbus for D-Bus

**Status:** accepted, not yet used (2026-10-12)

**Alternatives:** `dbus` crate (libdbus bindings), shelling out to `busctl`/`gdbus`.

**Why:** pure Rust, async-native, and typed proxies are generated from interface definitions. One crate covers UPower, MPRIS, NetworkManager, BlueZ, notifications and the tray. One shared connection serves all modules.

---

## D-8: chrono with only the `clock` feature

**Status:** accepted (2026-10-12)

**Why:** we need local time plus `strftime`-style formatting, and nothing else (no serde, no timezone database). `default-features = false` keeps it small. `jiff` would also work. We picked chrono because it's ubiquitous and already well understood.

---

## D-9: Pin `winit-core = "=0.31.0-beta.2"`

**Status:** accepted, temporary (2026-10-12)

**Why:** `iced_layershell` 0.19.1 depends on `iced_exdevtools`, which declares `winit-core = "0.31.0-beta.2"`. Cargo resolves that to `beta.3`, which added an enum variant that `iced_exdevtools` matches non-exhaustively, so the build fails. Pinning in `Cargo.toml` (not only `Cargo.lock`) keeps fresh builds working, for example on CachyOS.

**Remove when:** a newer `iced_layershell`/`iced_exdevtools` builds against the latest `winit-core`. Test by deleting the line and running `cargo update && cargo build`.

---

## D-10: Size-oriented release profile

**Status:** accepted (2026-10-12)

`lto = "thin"`, `codegen-units = 1`, `strip = true`, `panic = "abort"`.

**Why:** smaller and faster binaries for a process that runs all session long. `panic = "abort"` is acceptable because a panic is a bug to fix, not something to recover from, and the systemd user service restarts the bar after one ([D-16](#d-16-a-systemd-user-service-is-the-default-way-to-run-the-bar)). *(Corrected 2026-10-12: this used to say the bar "is supervised by Hyprland's `exec-once` setup or a user service". `exec-once` supervises nothing. A bar started that way stays dead after a panic until the next login.)* Full `lto = true` can be measured later. Thin LTO keeps release builds tolerable while iterating.

---

## D-11: Three layers. Plugins are widgets, and only widgets are plugins

**Status:** accepted (2026-10-12)

**What:** the code is split into three layers:

1. **Host** (`crates/eave`): every surface (the bar and popovers), the layout zones, the theme, rendering, click routing and the popover lifecycle. Not pluggable.
2. **Services:** shared event sources (Hyprland IPC, D-Bus, PipeWire). Not built yet. They will arrive with the Hyprland step, as a crate that widgets subscribe to, so one socket feeds many widgets.
3. **Widgets** (plugins): config, state, messages, `update`, a subscription, an `item()` for the bar and an optional `popover()`.

A widget **cannot change the bar's layout**. It can only *declare* things about its own item: hidden, its content, what a click sends. Bar geometry, zones, order, per-monitor bars and the notch belong to the config and the host. If those ever need to be extensible, that's a separate kind of extension, to be designed when there's a second use for it.

**Prior art considered:** Waybar (modules in fixed zones, styled by user CSS: free-for-all styling), ironbar (controller + widget + optional popup per module: the closest model), KDE plasmoids (compact representation in the panel, full representation in the popup), macOS status items (the system owns placement and style), Quickshell/AGS (no bar at all, services plus user code), COSMIC applets (one process per applet: too heavy), GNOME extensions (monkey-patch anything: breaks every release).

**Why:** the host owning layout, style and motion is what keeps the bar coherent and smooth, whoever writes the plugins. Splitting data (services) from UI (widgets) lets two widgets share one event stream.

---

## D-12: Third-party plugins, compiled in by `eave-build`

**Status:** accepted (2026-10-12). Reverses part of the "no plugin system for other people" non-goal in [vision.md](vision.md).

**What:** a plugin is a Rust crate in a git repository, with an `eave-plugin.toml` manifest at the root. It depends on `eave-api = "0.x"` and exports `pub fn register(&mut eave_api::Registry)`. Users list plugins in `~/.config/eave/plugins.toml`. `eave-build` clones them, checks each manifest's `api` against the eave checkout, generates a small crate whose `main` calls `eave::run(&[a::register, b::register])`, and builds it with cargo (the model is Caddy's xcaddy). The generated `[patch.crates-io]` points `eave-api` at the eave checkout, so the host and every plugin share one copy. That works even though eave-api isn't on crates.io (tested).

**Alternatives:**
- **Dynamic libraries:** Rust has no stable ABI, so every plugin would have to be built with the exact same compiler and eave version anyway, plus `unsafe` loading. All of the risk, none of the benefit.
- **WASM (wasmtime):** isolation and runtime loading, at a cost of several MB of binary, a large RSS increase, and a serialization boundary for every view. Against "lightweight".
- **External processes with a JSON protocol** (Waybar `custom`, i3bar): good for scripts, poor for real logic like popovers with state. It could be added later as one plugin that speaks a protocol built on the same UI vocabulary.
- **Plugin dependencies declared as git URLs:** cargo still fetches a patched git source, which fails offline or for private repos. A plain version requirement plus `[patch.crates-io]` avoids that.

**Why:** compiled-in plugins cost nothing at runtime: there's no loader, no ABI and no sandbox. The compiler checks every plugin against the API, and a plugin is an ordinary crate anyone can publish.

**Cost we accept:** adding a plugin means a rebuild. `eave-build` keeps its target directory in `~/.cache/eave`, so only the first build is slow.

---

## D-13: Plugins describe UI with a closed vocabulary

**Status:** accepted (2026-10-12)

**What:** widgets return `eave_api::ui::Item` (bar) and `Panel` (popover), built only from `label`, `heading`, `row`, `column`, `grid`, `button`, `separator` and `empty`, plus a semantic `Tone` (`Normal`, `Dim`, `Accent`, `Warning`, `Critical`). There are no colors, sizes, fonts or padding in the API. `eave-api` depends on `iced_runtime`/`iced_futures` only (for `Task` and `Subscription`), not on iced's widgets or renderer, so a plugin has no way to build a raw iced element. `crates/eave/src/kit.rs` is the only place nodes become pixels.

**Alternatives:** plugins return iced `Element`s (total freedom, total style drift, and plugins could break smoothness), or a closed vocabulary plus a `canvas` escape hatch (deferred: no use for it yet).

**Why:** consistent style across every plugin, and the host can animate any primitive (a meter easing to its value, for example) without plugins hand-rolling it. A missing primitive becomes a deliberate addition to the kit, not a one-off.

**Revisit when:** a real widget (graphs, album art, tray icons) can't be expressed. Add primitives first, a canvas last.

---

## D-14: Popovers are host-owned layer surfaces, and the host runs as a daemon

**Status:** accepted (2026-10-12)

**What:** clicking an item that has a popover (and no `on_press` of its own) opens a new `wlr-layer-shell` surface on the `overlay` layer, anchored top-left with a zero exclusive zone, so the compositor places it under the bar. It's created at 1×1 and drawn invisibly first. The `Measure` widget lays the panel out at its natural size, then the host resizes the surface to fit and centers it under the item, clamped to the screen. Only then is the panel drawn. One popover is open at a time. It closes on a second click on its item, when its widget asks, or 400 ms after the pointer leaves it. It uses the namespace `eave-popover`, so Hyprland `layerrule`s (blur, animation) can target it. Multiple surfaces need `iced_layershell::daemon` with `#[to_layer_message(multi)]`, which is also the path to one bar per monitor.

**Alternative:** `xdg_popup` (`NewPopUp`). The compositor would handle dismissal on outside click, but we'd lose control over position, size and motion.

**Gotcha:** `iced_layershell`'s margin tuples are `(top, right, bottom, left)`, the protocol order, even though the macro's doc comment says `(top, left, bottom, right)`.

**Open issue:** clicking *outside* an open popover doesn't close it yet. The options are `KeyboardInteractivity::OnDemand` with focus loss (needs testing on Hyprland), or a transparent full-screen catcher surface (with tiny-skia, a full-output buffer could cost tens of MB of RSS on the MacBook panel; measure first).

**Measured** (Asahi, release): RSS goes from ~11.8 MB to ~14.5 MB while the clock popover is open, and drops back when it closes.

---

## D-15: Type erasure through `Arc<dyn Any>` messages

**Status:** accepted (2026-10-12)

**What:** the host keeps `Vec<Box<dyn ErasedWidget>>`. A blanket impl turns every `Widget` into an `ErasedWidget`. Messages travel as `AnyMessage` (an `Arc<dyn Any + Send + Sync>`, cheap to clone as iced requires), and the host routes them back to the instance that produced them, whose impl downcasts. Instances are addressed by a stable id, not their position, since [D-17](#d-17-configtoml-is-hot-reloaded-through-inotify).

**Alternative:** a generated enum of all plugin widgets and messages. It's zero-cost, but `eave-build` would have to generate far more code, and the host couldn't be a normal library.

**Why:** with this approach the generated code is a single line of `register` calls. The cost is one allocation per message, which is nothing at a bar's message rate.

**Note:** each instance's subscription is tagged with its id (`Subscription::with`), so two widgets subscribing to the same thing don't share it today. Shared services (D-11) will fix that for the expensive sources.

---

## D-16: A systemd user service is the default way to run the bar

**Status:** accepted (2026-10-12)

**What:** `dist/systemd/eave.service` (`Restart=on-failure`, tied to `graphical-session.target`) is the documented default ([running.md](running.md)). Starting the bar from the Hyprland config stays fully supported, and `eave-build --restart` handles both setups.

**Alternatives:** only `exec-once` (or `hl.exec_cmd` in Lua configs), which is what Waybar users usually do. Or *requiring* systemd.

**Why:** with `panic = "abort"` ([D-10](decisions.md)), something has to restart the bar, and `exec-once` doesn't. The service also gives clean restarts (`systemctl --user restart eave`), logs in the journal, and memory accounting in `systemctl --user status`, which helps with "measure, don't guess". Both target distros (CachyOS, Asahi) run systemd.

**Cost we accept:**
- systemd must see the session's environment. That means uwsm, or `dbus-update-activation-environment --systemd --all` in the Hyprland config.
- `graphical-session.target` must be active. Without uwsm, nothing starts it, so the docs include a small `hyprland-session.target` that the Hyprland config starts. This is the most likely "my service doesn't start" cause, and it's documented.
- Non-systemd distros (Artix, Void) use the Hyprland-config route. Nothing in eave depends on systemd.

---

## D-17: `config.toml` is hot-reloaded through inotify

**Status:** accepted (2026-10-12)

**What:** `crates/eave/src/watch.rs` watches the config directory (not the file) with inotify, for complete writes (`CLOSE_WRITE`, never `MODIFY`), renames and deletion. If the directory doesn't exist, it watches the parent until it appears. The blocking inotify reads, and reading and parsing the file, run on the watcher's own thread, which sends the result to an iced subscription over an unbounded channel (a newer config is never dropped in favour of an older one), so `update` does no I/O. The host then compares the result with the current config:
- if it equals the current config, nothing happens (editors fire several events per save);
- if it doesn't parse, the error goes to stderr and **the current bar stays**;
- otherwise the instances are rebuilt. An instance whose entry is unchanged is kept as it is: state, subscription, open popover. Instances are addressed by a stable id instead of their position, so a reload can't route a leftover message to whichever widget now sits at the old index, and messages for removed widgets are dropped.

**Alternatives:**
- the `notify` crate: cross-platform, which we don't need, and more dependencies.
- the `inotify` crate's async stream: needs tokio's `net` feature, which adds mio.
- tokio `AsyncFd` on the inotify fd: same `net` requirement.
- polling the file's mtime: against "no polling" when an event source exists.

We use `rustix::fs::inotify`. rustix 1.x was already in the tree through the Wayland stack, so it's a direct dependency, not a new crate.

**Measured** (Asahi, release, two clocks): RSS ~11.7 MB (unchanged within noise), idle CPU 0%, one more thread, binary +65 KB (5.78 → 5.84 MiB).

---

## D-18: Rebuilding for `plugins.toml` happens outside the bar

**Status:** accepted (2026-10-12)

**What:** `eave-build --restart` builds, installs, then restarts a running bar on the new binary:
- If `eave.service` is active, it runs `systemctl --user restart`.
- Otherwise it finds our processes running `~/.local/bin/eave` in `/proc` (exe link, including the ` (deleted)` suffix left by the install), stops them, and relaunches through Hyprland. Lua configs (Hyprland 0.56+) get `hyprctl eval 'hl.exec_cmd(…)'`; older ones fall back to `hyprctl dispatch exec`.
- It runs only after a successful install, so a failed build never takes the bar down. If no bar is running, it doesn't start one.

An optional `dist/systemd/eave-rebuild.path` runs it whenever `plugins.toml` changes, at idle CPU and I/O priority. A failure sends a notification (`notify-send`, falling back to `hyprctl notify`).

**Alternatives:**
- the bar watches `plugins.toml` and runs cargo itself: a toolchain dependency at runtime, a long CPU spike, and a process managing its own replacement. Against "lightweight".
- a `--watch` mode in `eave-build`: a daemon of our own that needs debouncing, when systemd path units already do this.
- relaunching as a detached child of `eave-build`: the bar would inherit the terminal's environment instead of Hyprland's.

**Why:** adding a plugin needs a rebuild by design ([D-12](decisions.md)). This keeps it to one command, or none, without the bar paying for it.

**Gotcha:** in Lua configs, `hyprctl dispatch <args>` is evaluated as `hl.dispatch(<args>)`, so the classic `hyprctl dispatch exec <cmd>` fails there.
