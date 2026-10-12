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

**Status:** accepted, implemented in `crates/eave-hyprland` (2026-10-12, see [D-17](#d-17-one-shared-hyprland-event-reader-snapshot-then-events))

**Alternative:** the `hyprland` crate (hyprland-rs).

**Why:** the protocol is two Unix sockets with line-based events (`EVENT>>DATA`) and JSON replies. A focused client is about 100 lines, has no extra dependencies, and parses only the events we use. Owning it also means we decide how to handle Hyprland's IPC changes between releases (tolerant parsing, ignore unknown events), instead of waiting on a crate update.

---

## D-7: zbus for D-Bus

**Status:** accepted, in use since [D-18](#d-18-eave-dbus-one-system-connection-snapshot-on-propertieschanged) (2026-10-12)

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

**Why:** smaller and faster binaries for a process that runs all session long. `panic = "abort"` is acceptable because a panic is a bug to fix, not something to recover from, and the systemd user service restarts the bar after one ([D-20](#d-20-a-systemd-user-service-is-the-default-way-to-run-the-bar)). *(Corrected 2026-10-12: this used to say the bar "is supervised by Hyprland's `exec-once` setup or a user service". `exec-once` supervises nothing. A bar started that way stays dead after a panic until the next login.)* Full `lto = true` can be measured later. Thin LTO keeps release builds tolerable while iterating.

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

**What:** a plugin is a Rust crate in a git repository, with an `eave-plugin.toml` manifest at the root. It depends on `eave-api = "0.x"` and exports `pub fn register(&mut eave_api::Registry)`. Users list plugins in `~/.config/eave/plugins.toml` *(superseded by [D-30](#d-30-plugins-are-referenced-where-theyre-used-in-configtoml): config.toml's `widget = "owner/repo"` names them now, and plugins.toml is gone)*. `eave-build` clones them, checks each manifest's `api` against the eave checkout, generates a small crate whose `main` calls `eave::run(&[a::register, b::register])`, and builds it with cargo (the model is Caddy's xcaddy). The generated `[patch.crates-io]` points `eave-api` at the eave checkout, so the host and every plugin share one copy. That works even though eave-api isn't on crates.io (tested).

**Alternatives:**
- **Dynamic libraries:** Rust has no stable ABI, so every plugin would have to be built with the exact same compiler and eave version anyway, plus `unsafe` loading. All of the risk, none of the benefit.
- **WASM (wasmtime):** isolation and runtime loading, at a cost of several MB of binary, a large RSS increase, and a serialization boundary for every view. Against "lightweight".
- **External processes with a JSON protocol** (Waybar `custom`, i3bar): good for scripts, poor for real logic like popovers with state. It could be added later as one plugin that speaks a protocol built on the same UI vocabulary.
- **Plugin dependencies declared as git URLs:** cargo still fetches a patched git source, which fails offline or for private repos. A plain version requirement plus `[patch.crates-io]` avoids that.

**Why:** compiled-in plugins cost nothing at runtime: there's no loader, no ABI and no sandbox. The compiler checks every plugin against the API, and a plugin is an ordinary crate anyone can publish.

**Cost we accept:** adding a plugin means a rebuild. `eave-build` keeps its target directory in `~/.cache/eave`, so only the first build is slow. *(Since [D-31](#d-31-updates-are-explicit-installs-only-when-the-binary-changed), the source checkouts live in `~/.local/share/eave/src` and only the build is in the cache.)*

---

## D-13: Plugins describe UI with a closed vocabulary

**Status:** accepted (2026-10-12)

**What:** widgets return `eave_api::ui::Item` (bar) and `Panel` (popover), built only from `label`, `heading`, `row`, `column`, `grid`, `button`, `separator` and `empty` (later `switcher` (D-16) and `icon` (D-29)), plus a semantic `Tone` (`Normal`, `Dim`, `Accent`, `Warning`, `Critical`). There are no colors, sizes, fonts or padding in the API. `eave-api` depends on `iced_runtime`/`iced_futures` only (for `Task` and `Subscription`), not on iced's widgets or renderer, so a plugin has no way to build a raw iced element. `crates/eave/src/kit.rs` is the only place nodes become pixels.

**Alternatives:** plugins return iced `Element`s (total freedom, total style drift, and plugins could break smoothness), or a closed vocabulary plus a `canvas` escape hatch (deferred: no use for it yet).

**Why:** consistent style across every plugin, and the host can animate any primitive (a meter easing to its value, for example) without plugins hand-rolling it. A missing primitive becomes a deliberate addition to the kit, not a one-off.

**Revisit when:** a real widget (graphs, album art, tray icons) can't be expressed. Add primitives first, a canvas last.

---

## D-14: Popovers are host-owned layer surfaces, and the host runs as a daemon

**Status:** accepted (2026-10-12). The separate popover surface, `Measure` and centering under the item are superseded by [D-28](#d-28-popovers-grow-out-of-their-island-on-the-bar-surface); the daemon and outside-click parts stand.

**What:** clicking an item that has a popover (and no `on_press` of its own) opens a new `wlr-layer-shell` surface on the `overlay` layer, anchored top-left with a zero exclusive zone, so the compositor places it under the bar. It's created at 1×1 and drawn invisibly first. The `Measure` widget lays the panel out at its natural size, then the host resizes the surface to fit and centers it under the item, clamped to the screen. Only then is the panel drawn. One popover is open at a time. It closes on a second click on its item, when its widget asks, or ~~400 ms after the pointer leaves it~~ (dropped 2026-10-12: a popover should stay open until dismissed, see below). It uses the namespace `eave-popover`, so Hyprland `layerrule`s (blur, animation) can target it. Multiple surfaces need `iced_layershell::daemon` with `#[to_layer_message(multi)]`, which is also the path to one bar per monitor.

**Alternative:** `xdg_popup` (`NewPopUp`). The compositor would handle dismissal on outside click, but we'd lose control over position, size and motion.

**Gotcha:** `iced_layershell`'s margin tuples are `(top, right, bottom, left)`, the protocol order, even though the macro's doc comment says `(top, left, bottom, right)`.

~~**Open issue:** clicking *outside* an open popover doesn't close it yet. The options are `KeyboardInteractivity::OnDemand` with focus loss (needs testing on Hyprland), or a transparent full-screen catcher surface (with tiny-skia, a full-output buffer could cost tens of MB of RSS on the MacBook panel; measure first).~~

**Outside clicks (2026-10-12, resolves the open issue above):** while a popover is open, `eave_hyprland::report_clicks` installs a non-consuming left-click bind whose dispatcher is `event`. Hyprland then sends `custom>>eave-click` on socket2 for every click anywhere, and the click still reaches what was under it. The host closes the popover on that event unless the pointer is over one of its own surfaces, which it tracks with enter and leave events while the popover is open. It starts out as inside, because a popover opens from a click on the bar. Clicks on the bar are handled there instead:
- A click nothing handled (empty space, a plain item) arrives as an ignored pointer event.
- An item whose content handled the click (a workspace button) reports it through `Slot::on_inner_press`.
- An item with its own `on_press` closes another item's popover in `Pressed`.

A key press closes it too, since 2026-10-12 ([D-36](#d-36-popovers-take-the-keyboard-while-open)). The bind goes away when the popover closes. Overlapping on and off requests are serialized, and the last one wins. On Hyprland 0.56+ the bind is a Lua handle (`hl.bind(…, { non_consuming = true })`, removed with `:remove()`), and an install first removes any bind a crashed eave left behind. Older Hyprland gets `keyword bindn` / `keyword unbind ,mouse:272`. That unbind would also drop a user's own unmodified left-click bind, which is rare. Cost: the host now depends on `eave-hyprland`, which was already in the tree through the plugins. It's Hyprland-only, like the rest of eave ([platform.md](platform.md)). Measured (Asahi, release, two clocks, no popover open): RSS and idle CPU the same as before (~12.6 MB, idle CPU unchanged), binary +48 bytes. The 400 ms pointer-leave close went with it: a popover now stays open until it's dismissed. That also dropped the host's direct `tokio` dependency.

*Tried first:* `KeyboardInteractivity::OnDemand` and closing on focus loss. It works with click-to-focus, but with `input:follow_mouse = 1` (Hyprland's default) hovering a window takes the focus, so the popover closed as soon as the pointer left it. There's no way to tell that hover from a click.
*Rejected:* a full-screen catcher surface. It swallows the click, and the click should reach what's under it. It also costs a full-output tiny-skia buffer. Also rejected: a grabbed `xdg_popup`. The compositor would handle dismissal, but a popup can't be resized after it's created, and it would undo the positioning control described above.

**Measured** (Asahi, release): RSS goes from ~11.8 MB to ~14.5 MB while the clock popover is open, and drops back when it closes.

---

## D-15: Type erasure through `Arc<dyn Any>` messages

**Status:** accepted (2026-10-12)

**What:** the host keeps `Vec<Box<dyn ErasedWidget>>`. A blanket impl turns every `Widget` into an `ErasedWidget`. Messages travel as `AnyMessage` (an `Arc<dyn Any + Send + Sync>`, cheap to clone as iced requires), and the host routes them back to the instance that produced them, whose impl downcasts. Instances are addressed by a stable id, not their position, since [D-21](#d-21-configtoml-is-hot-reloaded-through-inotify).

**Alternative:** a generated enum of all plugin widgets and messages. It's zero-cost, but `eave-build` would have to generate far more code, and the host couldn't be a normal library.

**Why:** with this approach the generated code is a single line of `register` calls. The cost is one allocation per message, which is nothing at a bar's message rate.

**Note:** each instance's subscription is tagged with its id (`Subscription::with`), so two widgets subscribing to the same thing don't share it today. Shared services (D-11) will fix that for the expensive sources.

---

## D-16: `switcher` primitive and item scroll; the host animates the mark

**Status:** accepted (2026-10-12)

**What:** two additions to the vocabulary ([D-13](#d-13-plugins-describe-ui-with-a-closed-vocabulary)). `ui::switcher(options, current)` is a row of `(node, message)` choices, one of them current. `Item::on_scroll(up, down)` sends one message per scroll step. The host draws the switcher's mark (`theme::SWITCHER_MARK`) behind the current option. When `current` changes, the mark eases (cubic out, `theme::SWITCHER_SLIDE` = 180 ms) from wherever it is to the new option. Each step runs on `RedrawRequested`, which iced_layershell sends when the compositor's frame callback fires, and asks for another frame only until the mark arrives. The target is kept relative to the switcher, so a neighbour changing width moves the mark with the row instead of making it slide. `Slot` sums touchpad pixels (`theme::SCROLL_STEP` = 30 px per step) and caps one wheel event at 5 steps.

**Alternatives:** an `Accent` tone alone (no motion, which fails the smoothness goal the workspaces widget was meant to prove). A general `animate` wrapper plugins could put around anything: plugins would then choose motion, which is the host's job. Ticking a timer from the widget: polling, and not in step with frames.

**Why:** "workspace indicator" is really "one of N is current", which volume sinks or power profiles will need too, so it's one semantic primitive and not a workspace-specific one. The host owns the motion, so every switcher moves the same way.

**Measured** (Asahi, release, workspaces + window + clock + keyboard): binary 6.1 MiB (from 5.8), RSS ~12.2 MB, idle CPU 0.1%. Not yet measured: CPU *during* a slide (roadmap step 2).

---

## D-17: One shared Hyprland event reader: snapshot, then events

**Status:** accepted (2026-10-12)

**What:** `crates/eave-hyprland` starts one tokio task, the first time a widget watches, that reads socket2 and parses lines into `Event`s (unknown or malformed lines are dropped). It sends them on a `tokio::sync::broadcast` channel (capacity 64). `watch(snapshot)` subscribes to that channel *before* calling the widget's snapshot function, then yields `Update::Snapshot` followed by `Update::Event`s, so nothing falls between the two. If a watcher lags behind the channel, or socket2 reconnects (backoff from 1 s to 30 s), it takes a new snapshot. Snapshot failures are logged and retried on the next event, never polled. Socket1 requests (`query`, `dispatch`, `request`) open one connection each, which is how Hyprland's protocol works. Hyprland 0.56 moved to a Lua config, and socket1 `dispatch X` now runs `hl.dispatch(X)`, so dispatchers are written `hl.dsp.focus({ workspace = 3 })`. `dispatch(lua, legacy)` takes both forms, tries Lua first, and remembers which one worked. New dependency: `serde_json` (`default-features = false`) for `j/` replies. `tokio` and `futures-util` were already in the tree, so only features were added.

**Alternatives:** share by subscription identity. The host tags every widget's subscription with its instance id ([D-15](#d-15-type-erasure-through-arcdyn-any-messages)), so identical subscriptions never merge, and changing that would leak service identity into the host. Let each widget re-query on every event: simple, but a socket1 round trip per event per widget. Hand-parse JSON: fragile across Hyprland versions.

**Why:** a broadcast in the service crate shares the connection no matter how the host wires subscriptions. Snapshot-then-events means no widget needs its own catch-up logic. The service is an ordinary public crate, so built-in plugins don't get a private shortcut, and `eave-build` patches it like `eave-api`.

**Cost:** cargo warns "patch `eave-hyprland` was not used" in `eave-build` builds that have no Hyprland plugin. It's harmless. Revisit if more service crates make this noisy.

---

## D-18: `eave-dbus`: one system connection, snapshot on PropertiesChanged

**Status:** accepted (2026-10-12). Its use for `battery` (UPower) and `network` (NetworkManager) is **superseded by [D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel)**: both now read the kernel. `eave-dbus` stays for media, Bluetooth, notifications and the tray. The text below is kept as it was decided.

**What:** `crates/eave-dbus` holds one shared system bus connection (`zbus`, `default-features = false` plus `tokio`, so there's no second executor). `watch(service, snapshot)` subscribes to `PropertiesChanged` from `service` with a match rule, then calls the widget's snapshot function. The snapshot returns the state and the object paths it read. After that, a signal on one of those paths takes a new snapshot. A burst of signals is drained, then there's a 50 ms settle, so a burst leads to one re-read. A snapshot equal to the last one isn't yielded. `battery` reads UPower's DisplayDevice. `network` follows NetworkManager's root object, then the primary connection, its device, the active access point and the IP4 config, and re-reads when any of them changes, so a reconnect to a new AP is picked up.

**Alternatives:** typed `#[proxy]` interfaces with one `receive_*_changed` stream per property: more code per widget and more match rules, for no gain at a bar's message rate. Signal payloads applied as deltas: fiddly per interface, and an object switch (a new AP) needs a full re-read anyway. `busctl`/`nmcli` subprocesses: rejected in D-7.

**Why:** snapshot-then-changes is the same model as `eave-hyprland` ([D-17](#d-17-one-shared-hyprland-event-reader-snapshot-then-events)), so widgets stay simple. Watching only the paths the snapshot used stops NetworkManager's scan updates for other APs from waking us. Skipping equal snapshots keeps "redraw only on change".

**Measured** (Asahi, release, workspaces + window + clock + keyboard + network + battery): binary 7.0 MiB (+0.9 MiB for zbus), RSS ~13.5 MB (+1.3 MB), idle CPU 0.1%. zbus is the largest dependency so far. It's accepted because media, Bluetooth, notifications and the tray all need it too.

**Open:** iwd-only systems without NetworkManager. `eave-build` now patches `eave-dbus` as well. *(Resolved by D-23: network no longer goes through NetworkManager.)*

---

## D-19: Volume over the PulseAudio protocol, not libpipewire

**Status:** accepted (2026-10-12). Replaces the "PipeWire listener (`pipewire` crate)" plan in [architecture.md](architecture.md) and the roadmap.

**What:** `crates/eave-pulse` speaks the PulseAudio native protocol through the pure-Rust `pulseaudio` crate (0.3), to `pipewire-pulse`, which PipeWire desktops run by default. It uses only the crate's protocol module, not its client (the client spawns its own reactor thread and has no subscribe or set-volume calls). One `eave-pulse` thread holds a connection subscribed to sink and server events and blocks reading it. Each event goes out on a `broadcast` as "something changed". Watchers drain a burst, then read the server info and sink list once on tokio's blocking pool, on a short-lived connection, and skip readings that didn't change. Commands (volume, mute, default sink) also use short-lived connections. The volume widget shows a new level at once and lets the server's reading confirm it. Steps snap to multiples of `step`, and it never sets above `max` (default 100).

**Alternatives:**
- **`pipewire` crate (libpipewire bindings):** native, but it needs libpipewire and libclang at build time. Reading the default sink's volume means following the `default` metadata object, binding the node and decoding SPA pods. Setting it properly goes through the device route, the way WirePlumber does. All that is several hundred lines of unsafe-adjacent code for one number.
- **`libpulse-binding`:** C library, own main loop. No better than the above.
- **`wpctl`/`pactl` subprocesses:** polling or parsing text, rejected as in D-7.

**Why:** pure Rust, with no C toolchain needed on Asahi or CachyOS. The protocol gives exactly the model a volume widget needs (sinks, default, volume, mute, change events). And it's what `pavucontrol` and most bars use, so it's well exercised against pipewire-pulse.

**Cost:** it depends on pipewire-pulse running. It does by default on both targets, and the widget hides if there's no server. One more OS thread (blocked in `read`, so no CPU).

**Revisited in [D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel)** (2026-10-12), with the goal "core plugins work on any Hyprland machine". We keep this. pipewire-pulse (or PulseAudio itself, which speaks the same protocol) is the de facto audio API on Linux desktops: browsers, pavucontrol and every other bar use it. The only way around it is native PipeWire, which means libpipewire (C, plus bindgen and libclang to build), since no usable pure-Rust PipeWire client exists. That trade isn't worth it.

**Measured** (Asahi, release, all seven widgets): binary 7.3 MiB (+0.25 MiB), RSS ~13.9 MB (+0.4 MB), idle CPU 0.1%.

---

## D-20: A systemd user service is the default way to run the bar

**Status:** accepted (2026-10-12)

**What:** `dist/systemd/eave.service` (`Restart=on-failure`, tied to `graphical-session.target`) is the documented default ([running.md](running.md)). Starting the bar from the Hyprland config stays fully supported, and `eave-build --restart` handles both setups. *(Plain `eave-build` since D-31.)*

**Alternatives:** only `exec-once` (or `hl.exec_cmd` in Lua configs), which is what Waybar users usually do. Or *requiring* systemd.

**Why:** with `panic = "abort"` ([D-10](decisions.md)), something has to restart the bar, and `exec-once` doesn't. The service also gives clean restarts (`systemctl --user restart eave`), logs in the journal, and memory accounting in `systemctl --user status`, which helps with "measure, don't guess". Both target distros (CachyOS, Asahi) run systemd.

**Cost we accept:**
- systemd must see the session's environment. That means uwsm, or `dbus-update-activation-environment --systemd --all` in the Hyprland config.
- `graphical-session.target` must be active. Without uwsm, nothing starts it, so the docs include a small `hyprland-session.target` that the Hyprland config starts. This is the most likely "my service doesn't start" cause, and it's documented.
- Non-systemd distros (Artix, Void) use the Hyprland-config route. Nothing in eave depends on systemd.

---

## D-21: `config.toml` is hot-reloaded through inotify

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

## D-22: Rebuilding for `plugins.toml` happens outside the bar

**Status:** accepted (2026-10-12). The restart and the path unit stand. What they watch changed in [D-30](#d-30-plugins-are-referenced-where-theyre-used-in-configtoml) (config.toml, not plugins.toml), and the command in [D-31](#d-31-updates-are-explicit-installs-only-when-the-binary-changed) (`eave-build`, which restarts only when the binary changed).

**What:** `eave-build --restart` builds, installs, then restarts a running bar on the new binary:
- If `eave.service` is active, it runs `systemctl --user restart`.
- Otherwise it finds our processes running `~/.local/bin/eave` in `/proc` (exe link, including the ` (deleted)` suffix left by the install), stops them, and relaunches through Hyprland. Lua configs (Hyprland 0.56+) get `eval hl.exec_cmd(…)`; older ones fall back to `dispatch exec`. *(Since [D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel): both go straight to `.socket.sock`, and processes are stopped with `kill(2)` through rustix. It used to run the `hyprctl` and `kill` binaries.)*
- It runs only after a successful install, so a failed build never takes the bar down. If no bar is running, it doesn't start one.

An optional `dist/systemd/eave-rebuild.path` runs it whenever `plugins.toml` changes, at idle CPU and I/O priority. A failure sends a notification (`notify-send`, falling back to `hyprctl notify`).

**Alternatives:**
- the bar watches `plugins.toml` and runs cargo itself: a toolchain dependency at runtime, a long CPU spike, and a process managing its own replacement. Against "lightweight".
- a `--watch` mode in `eave-build`: a daemon of our own that needs debouncing, when systemd path units already do this.
- relaunching as a detached child of `eave-build`: the bar would inherit the terminal's environment instead of Hyprland's.

**Why:** adding a plugin needs a rebuild by design ([D-12](decisions.md)). This keeps it to one command, or none, without the bar paying for it.

**Gotcha:** in Lua configs, `hyprctl dispatch <args>` is evaluated as `hl.dispatch(<args>)`, so the classic `hyprctl dispatch exec <cmd>` fails there.

**Gotcha:** the rebuild unit runs under systemd, which often lacks `HYPRLAND_INSTANCE_SIGNATURE` without uwsm. The install still happened, but the restart failed, and that was reported as a failed rebuild. Since 2026-10-12, `eave-build` falls back to the only instance under `$XDG_RUNTIME_DIR/hypr` (or `/tmp/hypr`) when the variable is missing.

---

## D-23: Core plugins depend only on Hyprland and the kernel

**Status:** accepted (2026-10-12). Supersedes D-18's battery and network parts.

**What:** the bar and its core plugins must work on any Hyprland machine without checking what else is installed. An audit found three plugins tied to this machine's setup:
- `battery` read UPower. Minimal Hyprland installs often don't have it. It now reads `/sys/class/power_supply` directly (`plugins/battery/src/sysfs.rs`). System batteries (`type = Battery`, `scope` not `Device`, so a mouse doesn't count) are combined the way UPower's display device does it: percentage = stored energy / full energy (charge × design voltage when a driver only reports charge), and a state that is charging if any battery charges. Watts and time left come from `power_now` (or `current_now × voltage_now`), or from the kernel's own `time_to_*_now` for a single battery. One `eave-battery` thread blocks on a `NETLINK_KOBJECT_UEVENT` socket (rustix), so plugging in shows at once. Drivers don't send an event for every percent, so it also re-reads every 30 s, but only while a battery exists. A desktop never wakes.
- `network` read NetworkManager, so iwd, systemd-networkd and dhcpcd setups got nothing, and the widget showed "Offline" forever. It now uses `crates/eave-net`, which asks the kernel. rtnetlink provides links, addresses and default routes (primary = the lowest-metric IPv4 default in the main table, else IPv6), and nl80211 provides the SSID, frequency and AP signal. One `eave-net` thread blocks on rtnetlink's link/address/route multicast groups, drains a burst for 50 ms, re-reads, and publishes only changes. Signal strength has no event, so it's re-read every 10 s while the primary link is Wi-Fi. Without Wi-Fi it never wakes. Nothing needs privileges.
- `eave-build --restart` ran `kill` and `hyprctl`. It now uses `kill(2)` through rustix and writes to Hyprland's `.socket.sock` itself.

Both new watchers publish through a `tokio::sync::watch` channel, so every widget instance shares one thread.

**Crates:** `netlink-packet-core`, `netlink-packet-route` and `netlink-packet-generic` (rust-netlink, pure Rust) build and parse the messages. The sockets are plain blocking rustix sockets on our own thread, so there's no `netlink-sys`/`netlink-proto` and no tokio `net` or mio, for the same reason as D-21. We parse the few nl80211 attributes ourselves (SSID, frequency, station signal), because `wl-nl80211` comes with an async stack (`netlink-proto`, `genetlink`, `futures-executor`, two `thiserror`s) for what is three requests. rustix needs its `fs` feature next to `net`, because rustix 1.1 doesn't build `net` alone.

**Alternatives:**
- Keep NetworkManager and add iwd's D-Bus API as a fallback: two backends that still miss systemd-networkd and dhcpcd, and a "which daemon is this?" check on every machine.
- `/proc/net/wireless` for the signal: it needs the kernel's wireless-extensions compatibility, which isn't always built in, and has no SSID.
- Keep UPower and fall back to sysfs: two code paths for the same number.
- Listen to nl80211's `mlme` multicast group for connect/disconnect events: rtnetlink already reports those through link and address changes, and roaming within one network is picked up by the 10 s signal re-read.

**What we give up:** NetworkManager's "limited connectivity" state. Reproducing it means probing a portal over HTTP. Also NetworkManager's connection names: wired connections show as "Wired" and the interface. Tunnels in a separate routing table (wg-quick, Tailscale) don't count as primary. The physical link shows instead.

**Not worth it (kept as is):**
- Volume still needs pipewire-pulse or PulseAudio (see the note in D-19).
- Fonts come from the system. Every desktop has some. Revisit only if a glyph we use (↑ ‹ › − …) turns out to be missing somewhere.
- `eave-build` still runs `systemctl`, but only when systemd is there. Talking to systemd over D-Bus would bring zbus into a CLI for nothing. `cargo` and `git` are inherent to a build tool.

**Measured** (Asahi, release, all seven widgets, 60 s idle): binary 7.28 → 7.03 MiB (UPower and NetworkManager no longer go through zbus. zbus itself stays, because `iced_layershell` pulls it in through `mundy`). RSS ~14.5 MB before and after, in the same session. Idle CPU is the same within noise: 12–25 ticks per 60 s for both binaries, driven by Hyprland window-title events. The `eave-net` thread used 30 ms of CPU in 70 s on Wi-Fi, most of it startup. The battery thread used none. `ldd` still shows only libc, libm and libgcc_s.

---

## D-24: Floating islands on one transparent surface

**Status:** accepted (2026-10-12)

**What:** the bar no longer paints a full-width strip. Each zone (left, center, right) is an island: a rounded container (`ISLAND_RADIUS`, `ISLAND_BG`, the popover's hairline border) as wide as its items. A zone with no visible items has no island. Each zone still takes a third of the width, so the center island stays centered on screen. The islands sit `EDGE_GAP` (6 px, the same as `PANEL_GAP`) from the top, left and right edges. All of it is drawn on the one transparent layer surface. The gap is padding inside the surface, which is `ISLAND_HEIGHT + EDGE_GAP` tall, and that's also the exclusive zone. *(Superseded in part by [D-26](#d-26-squircle-islands-that-squeeze-and-spring-back): the surface and exclusive zone also include `DRAG_ROOM` below the islands, and islands are squircles instead of rounded rectangles.)*

**Alternatives:**
- one layer surface per island: clicks between islands would reach the desktop, but it costs three buffers, and popovers would need anchoring across surfaces. Every surface would also have to resize whenever its content's width changes.
- a layer-shell margin for the edge gap instead of padding: surface coordinates would no longer match screen coordinates, so item bounds and `popover_x` would need offsets. How the margin adds to the exclusive zone would also be up to the compositor.

**Why:** a purely visual change shouldn't cost surfaces or memory. With padding, the item bounds and the popover placement code stay as they were.

**Trade-off:** the transparent gaps around and between islands still belong to the bar surface, so clicks there go nowhere. That area is inside the exclusive zone, where no windows sit anyway.

**Measured** (x86_64, release, two clocks, side by side after 40 s): RSS 12.6 MB on `main`, 13.1 MB with islands. The surface is 6 px taller, so its buffers are larger, about +0.3 MB at scale 2. Idle CPU 0%, binary size unchanged (7.63 MB). The islands only actually show through once [D-25](#d-25-a-patched-softbuffer-so-the-cpu-renderer-keeps-alpha) is in.

---

## D-25: A patched softbuffer so the CPU renderer keeps alpha

**Status:** accepted (2026-10-12)

**What:** `vendor/softbuffer` is softbuffer 0.4.8 with one change: its Wayland backend creates `wl_shm` buffers as `Argb8888` instead of `Xrgb8888`. The workspace uses it through `[patch.crates-io]`, and `eave-build` writes the same patch into the manifests it generates. `vendor/` is excluded from the workspace, so we don't test or lint it.

**Why:** the tiny-skia renderer ([D-4](#d-4-cpu-renderer-tiny-skia-by-default-gpu-wgpu-optional)) presents through softbuffer, which hardcodes `Xrgb8888` on Wayland. The compositor ignores the alpha byte, so every transparent pixel came out black: the gaps around the islands ([D-24](#d-24-floating-islands-on-one-transparent-surface)) and the corners of popovers. Rounded popovers had never actually been round. tiny-skia already writes premultiplied BGRA, which is the in-memory layout `Argb8888` expects, and the protocol requires every compositor to support that format. So changing the format string is the whole fix.

**Alternatives:**
- the `gpu` renderer: wgpu negotiates a premultiplied alpha surface, but costs ~60 MB of RSS (D-4).
- an upstream softbuffer with alpha support: 0.4.8 is the latest release and has no such option. Drop the patch once one does.
- fake the corners by painting the wallpaper color: impossible, since we don't know what's behind the bar.

**Cost:** about 370 KB of vendored source, and the patch has to be redone whenever iced moves to a newer softbuffer.

**Measured** (x86_64, release, two clocks, three builds side by side after 40 s): RSS 12.6 MB on `main`, 13.1 MB with islands, 13.8 MB with islands and this patch. The patch doesn't change our allocations. Our guess is that Hyprland holds an alpha buffer longer, so softbuffer allocates one more (one buffer is ~0.9 MB at 3024×76). Idle CPU 0%. Binary size unchanged.

---

## D-26: Squircle islands that squeeze and spring back

**Status:** accepted (2026-10-12)

**What:** islands are drawn by a host widget, `island.rs`, instead of a styled container.
- **Shape:** a squircle. Each corner is a short circular arc eased into the edges by a cubic on either side (Figma's corner smoothing with "preserve smoothing", `ISLAND_RADIUS` 14 with `ISLAND_SMOOTHING` 0.6). The island is too short for the full spread, so the easing cubics are squeezed to fit instead of the smoothing being reduced. The ends stay fully curved and smooth up to a radius of half the height. The fill is opaque black (`ISLAND_BG`). The path is built once per island size in a `canvas::Cache`. Animation frames only move it.
- **Press:** pressing anywhere on an island squeezes each side in by `PRESS_INSET` px. It's a fixed pixel amount, not a percentage, so wide and narrow islands react alike.
- **Drag:** while the button is held, the island is pulled toward the pointer through a rubber band. It moves almost 1:1 at first, then approaches `DRAG_LIMIT_X`/`DRAG_LIMIT_Y` without reaching them.
- **Release:** a damped spring (`SPRING_STIFFNESS`, damping ratio `SPRING_DAMPING` 0.5) brings offset and squeeze back, with a small overshoot.
- **Frames and events:** the springs step on `RedrawRequested`, like the switcher (D-16). A frame is requested only until every spring is at rest. The island only *watches* pointer events and never captures them, so items handle their own clicks.
- **Long press:** an item's primary click (its `on_press`, or opening its popover) now fires on *release*, and only if the press was shorter than `LONG_PRESS` (400 ms) and the pointer is still over the item. Pressing and holding to play with an island therefore opens nothing. A drag that ends off the item cancels too, like a normal button. Right click and scroll still act at once.
- **Surface:** it gained `DRAG_ROOM` (6 px) of transparent space below the islands, so an island can dip down without being cut off. The exclusive zone covers it, so the strip never sits over windows. Windows start 6 px lower. Popovers drop their top margin by the same amount, so the island-to-popover gap stays at `PANEL_GAP`.

**Alternatives:**
- quads with a bigger radius: iced quads only do circular corners, so the corners always meet the edges with a visible kink.
- a vector or raster image for the background: it has to stretch to any width, so it would still need a path.
- a surface taller than its exclusive zone: the extra strip would sit over the top of windows and swallow their clicks, because we can't set an input region through iced_layershell. *(Wrong, corrected by [D-28](#d-28-popovers-grow-out-of-their-island-on-the-bar-surface): iced_layershell 0.19 has `SetInputRegion`. Keeping `DRAG_ROOM` inside the exclusive zone is still simpler.)*
- follow the pointer directly while dragging: feels stiff. Going through the spring gives it some weight.

**Cost:** in the default CPU build, the geometry code was already compiled in (iced_layershell enables `iced_graphics/geometry`), so the `canvas` feature added no crate, and binary size is unchanged (7.63 MB, x86_64 numbers in D-24; 7,630,000 bytes on aarch64 before and after). The `gpu` build now also pulls in `lyon_tessellation`, which wgpu needs to fill paths.

**Measured** (aarch64 Asahi, release, two clocks, old and new binaries started side by side, read after 40 s): RSS 13.1 MB before, 13.3 MB after. The difference is the taller surface's shm buffer (+0.26 MB). Idle CPU 0 ticks over 10 s, so nothing keeps asking for frames. *Still to do:* CPU while dragging, with the debug overlay.

---

## D-27: Islands morph when their content changes

**Status:** accepted (2026-10-12)

**What:** an island never snaps to a new shape.
- **Shape:** its four edges are springs in surface coordinates that chase the island's layout bounds (`MORPH_STIFFNESS` 380, damping ratio `MORPH_DAMPING` 0.9, so it settles without wobbling). A new island grows out of a sliver at its center. The squircle `Cache` is keyed by size, so it's rebuilt each frame only while the shape moves, and once at rest.
- **Items:** `Island` lays its items out itself, keyed by instance id, instead of wrapping a `row`. Each key has an x spring and a fade spring. An item that stays slides from where it was drawn to its new place. A new one appears in place and fades in. Hit testing uses the final layout throughout, which is fine for the ~300 ms a move takes.
- **Fade:** iced has no group opacity. The island is opaque (`ISLAND_BG`), so fading content toward it is the same as covering it with that color at `1 − shown` alpha. The quad gets a layer of its own, because text draws after quads within a layer. It's clipped to the shape, because a nested layer isn't clipped by the one around it.
- Everything is clipped to the shape's rectangle while it moves, so nothing pokes out.

**Not done:** a removed item vanishes at once, and its neighbors slide into the gap while the shape shrinks around them. Fading it out would mean keeping its last rendering after the widget stopped producing it.

**Alternatives:** clip-reveal only (items jump to their final places and the moving shape reveals them): simpler, but the items visibly snap. Animating the layout itself (re-laying out every frame): more work per frame, and other zones would shift along.

**Cost:** binary size unchanged (7,433,240 bytes on aarch64 before and after D-27 and D-28 together). Idle RSS unchanged (Asahi, release, workspaces/clock/volume/network/battery, side by side after 20 s: 14.2 MB `main`, 14.1 MB here). Idle CPU 0 ticks: nothing asks for frames at rest.

---

## D-28: Popovers grow out of their island, on the bar surface

**Status:** accepted (2026-10-12). Supersedes D-14's separate popover surface.

**What:** clicking an item with a popover turns its whole island into the popover. The island's edges spring from the island to the panel's place: top edge on the island, growing from the island's left edge, right edge or center (left, right or center zone), never narrower than the island, `EDGE_GAP` from the screen sides. Its items fade out, the panel's content fades in once the items are gone and the shape is halfway there, and closing reverses it. The two never show at once: each fades by covering itself with the island's color (D-27), so where they overlap, one's cover would also darken the other. Before this rule, the panel's cover hid the bottom half of the still-fading icons at once, and the icons' covers made the panel's text flicker as it came in. The handover fades run on their own critically damped spring (`FADE_STIFFNESS` 1500), quicker than the shape, so the content still arrives while the shape moves. The panel takes the island's opaque black, so it's one continuous shape.
- The panel is drawn in an iced **overlay** of the island: laid out against the whole surface, drawn over the other islands, and given events first. It captures presses on its shape, so an unhandled click on the panel doesn't close it.
- The **bar surface grows** to fit (`SizeChange` to `(0, panel bottom + EDGE_GAP)`) while a popover is there, and its exclusive zone stays `BAR_HEIGHT`, so windows never move. An **input region** of the bar strip plus the panel lets the pointer reach windows beside and below it (`SetInputRegion`, which iced_layershell 0.19 has, contrary to what D-26 said). The island holds the morph until the surface is tall enough, so it never draws into a surface that hasn't grown yet.
- The host keeps a closing popover until the island reports `PopoverClosed`, then shrinks the surface. Reopening while closing turns the same morph around. Another island's popover can open while the first one is still closing.
- `Measure`, the `eave-popover` surface and its namespace, `PANEL_BG`, `PANEL_RADIUS` and `PANEL_GAP` are gone. `Slot` no longer reports item bounds.

**Why one surface:** a shape can only morph seamlessly if one surface draws all of it. Two surfaces commit independently, so a handoff between the bar's island and a popover surface risks a frame where both or neither show.

**Alternatives:**
- keep the separate surface, overlapping the bar (`exclusive_zone: -1`, top margin 0), draw the morph there and hide the bar's island during the handoff: the one-frame seam above, plus two surfaces' buffers.
- centered under the clicked item, as before: the shape would have to travel sideways, and doesn't read as the island becoming the popover.

**Trade-off:** layer-shell surfaces anchored left and right are full width, so the grown bar buffer is full width too. softbuffer never shrinks its shm pool (`wl_shm_pool` can only grow), so after the first popover the memory for the tallest surface so far stays. Measured (Asahi, scale 2, 3024 px wide, release): 14.1 MB before the first popover, 22.4 MB with the volume popover open (surface 44 → 184 logical px), and it stays at ~22.6 MB after closing. D-14's separate surface measured +2.7 MB while open. Recreating the buffer when the surface shrinks a lot would get it back. That would be another change to `vendor/softbuffer` (D-25).

**Gotcha:** the panel reports a cursor other than `None` over its shape, so the islands under it don't react to the pointer. That can't be `Idle`: iced_layershell 0.19 turns `Idle` into the busy cursor. It reports `ContextMenu`, which iced_layershell shows as the default arrow.

*Tried and dropped:* the items rising and the content rising into place as they fade. The shape's top edge stays put and there are only ~5 px above an icon, so the island's edge cut the items' tops off and they looked like they were shrinking.

**Gotcha:** iced_layershell's `SetInputRegion` reuses one `wl_region` for every call and only subtracts the surface's *current* size before handing it to the callback. Right after a `SizeChange` that may still be the old size, so the host's callback clears the whole plane first and then adds the bar strip and each panel. The host sends the region together with every `SizeChange`.

---

## D-29: Semantic icons, drawn as vectors by the host

**Status:** accepted (2026-10-12)

**What:** `eave_api::ui::icon(Icon)` adds status symbols to the vocabulary. `Icon` is a closed enum that names *what* to show and how full it is: `Volume { level, muted }`, `Wifi { strength: Option }`, `Wired`, `Battery { charge, charging }`. It has no sizes or colors. `crates/eave/src/icon.rs` draws each one as canvas paths on a 16-unit grid, in the tone of the text around it. Parts that are off (quiet waves, missing signal bars) stay at `ICON_OFF_ALPHA`, so an icon reads as a gauge. The host decides how a fraction maps to bars: volume lights 1/2/3 waves from 20/50/80 %, and Wi-Fi lights 1/2/3 arcs from 25/50/75 %. Slashes (muted, offline) and the charging bolt are cut out with a rim of `ISLAND_BG`, so they assume they sit on an island. The volume, network and battery widgets now show icons in the bar. The text moved to their popovers, except the battery percent, which stays next to its icon.

**Alternatives:**
- an icon font (Nerd Fonts, Material Symbols): a font the system must have, missing glyphs show as boxes, and a glyph can't be filled to an arbitrary level.
- SVG files: the `svg` feature pulls in resvg and usvg, which is a lot of code to draw five shapes, and partial fill still needs code.
- let plugins draw (a canvas in the API): rejected by D-13. Every plugin would need its own icons, and they would drift apart.

**Why:** the `canvas` feature is already compiled in for the islands (D-26), so this costs no crate. The vectors are crisp at any scale factor, and the semantic enum keeps the visuals in the host.

**Measured** (aarch64 Asahi, release, workspaces/clock/volume/network/battery, old and new binaries in turn): binary 7,433,240 bytes before and after, RSS 14.2 MB before and 14.2 MB after, idle CPU 3 and 5 ticks over 10 s (noise).

---

## D-30: Plugins are referenced where they're used, in config.toml

**Status:** accepted (2026-10-12). Replaces `plugins.toml` from [D-12](#d-12-third-party-plugins-compiled-in-by-eave-build).

**What:** `widget = "…"` in config.toml both places a widget and says where it comes from: `<source>[@rev][#widget]`. No `/` is a built-in (`plugins/<name>` of the eave checkout), `owner/repo` is GitHub, `host/path` and anything with a `:` are git URLs, and a path starting with `/`, `~` or `.` is a local directory. The widget is the repository's name without `.git` and a leading `eave-`, unless `#widget` names it. A top-level `eave = "<source>[@rev]"` picks eave's own source, and defaults to `francescomengarelli/eave`. `eave-build` reads the same file: it compiles in exactly the widgets the config uses, built-ins included, and errors when one source is asked for at two revisions. The bar looks widgets up by the name part (`config::widget_name`). eave-build has its own copy of the grammar, because depending on `eave-api` would pull iced into a CLI. Both copies are tested against the same cases. `eave-rebuild.path` now watches config.toml.

**Alternatives:**
- keep `plugins.toml` beside config.toml: two files to keep in step, and a built-in had to be listed in one before it could be used in the other.
- a top-level `plugins = [...]` list in config.toml, with widgets placed by short name: one file, but still two places to edit for one widget.
- all built-ins always compiled in, so only third-party plugins need a rebuild: simpler, but the binary carries widgets nobody placed.

**Why:** one file, and one line per widget. Writing where a widget comes from where it is placed makes the config the whole description of the bar. The binary holds only what is used.

**Cost:** using a built-in you hadn't placed before is now a rebuild (the path unit does it, in well under a minute incrementally), and the bar skips it until then. Two plugins from different owners with the same repository name clash, as two registrations of one name did before. Use `#widget` if the plugin registers it under another name.

**Measured** (aarch64 Asahi, release): the installed binary with workspaces, window, clock, volume, network and battery is 7,367,704 bytes, against 7,433,240 for the default binary with all seven built-ins (−64 KB, the keyboard plugin). RSS and idle CPU weren't measured cleanly: run next to the live bar, both binaries varied more between runs (15–32 MB) than between each other. The code that runs is the same, minus one unused registration.

---

## D-31: Updates are explicit; installs only when the binary changed

**Status:** accepted (2026-10-12)

**What:**
- Git sources are cloned once into `$XDG_DATA_HOME/eave/src` and then left alone. Without a `@rev` a checkout stays at the commit it has. A `@rev` that isn't there yet (a pin just edited) is fetched. Only `eave-build update` fetches everything, moves each source to the newest commit of its `@rev` or default branch, and prints `old → new (N new commits)`. When eave itself moved, it also runs `cargo install --locked --path <eave>/crates/eave-build`, after the bar is in place.
- `eave-build` (which replaces `--install` and `--restart`) always builds, then compares the result with `~/.local/bin/eave` byte for byte. It installs and restarts the bar only when they differ.
- `eave-build init` writes a starter config.toml (`dist/config.toml`, kept if one exists) and the three systemd units. `install.sh` is a thin POSIX script around it: check for `cargo` and `git`, `cargo install --git … eave-build`, `init`, build, enable the units. Re-running it never updates.
- The rebuild unit runs eave-build by its full path, which `init` writes in (2026-10-12). *Superseded:* `ExecStart=eave-build` with `Environment=PATH=%h/.cargo/bin:…`. systemd 262 resolves a bare name only in its compiled-in search path, never through the unit's `PATH`, so every rebuild failed with status 203/EXEC. Since the units can change, `eave-build update` runs the new eave-build's `init` after reinstalling it, then `systemctl --user daemon-reload`.

**Alternatives:**
- fetch on every build (as before): saving config.toml could upgrade eave and every plugin behind your back, through the path unit.
- a lockfile recording each source's commit: reproducible across machines, but one more file. The checkouts already are the lock on one machine, and pins cover the rest.
- compare a hash of the generated crate instead of the binary: misses changes in local-path sources, which is how plugins are developed.
- a distribution package (AUR): later. It doesn't fit compiled-in plugins without a toolchain anyway.

**Why:** the path unit now fires on every config save, so a build has to be cheap and inert when nothing changed. Explicit updates keep the bar from changing unless you asked. Sources moved out of the cache because clearing a cache must never change versions.

**Gotcha:** `eave-build` embeds `dist/` with `include_str!`, so it builds only from the repository (`cargo install --path` or `--git`), not as a standalone crate.

---

## D-32: Plugins can nudge their island

**Status:** accepted (2026-10-12)

**What:** `Effect::nudge(Nudge)` lets a widget react physically to its own events. `Nudge` is `Up`, `Down`, `Left` or `Right`. It says which way something went, not how to move. The host maps it:
- **`Up` / `Down`:** the island's top bulges up above the item, or its bottom dips below it, about 2 px, then settles flat. The bump is a raised cosine `NUDGE_BUMP_WIDTH` (64 px) wide, and the squircle path *warps* around it: every point moves vertically by the bump's height at its x. The full height applies on the bumped edge, eased down to nothing halfway across the shape. So a bump next to a corner bends the end of the corner along with it and keeps its peak over the item, and the opposite edge never moves. The warp flattens the path into pieces of at most 1 px, so it only runs while a bump shows.
  - *Superseded on the way:* a hump confined to the straight part of the edge. Clamped clear of the corners, it slid away from an item near the island's end. Cut short on the corner side instead, it stayed over the item but rose steeply on that side.
  The whole island also moves about half a pixel the same way (one physical pixel at scale 2).
  The nudged item rides along with its bump, by `NUDGE_ITEM_FOLLOW` (half) of it.
- **`Left` / `Right`:** the whole island travels about 5 px that way and comes back.

The island's movement uses springs of its own (`push_x`, `push_y`), added to the drag offset. Their feel is `NUDGE`: stiffness 600, damping ratio 0.85, so the island comes back without swinging past. *Superseded:* kicking the drag spring itself. Its playful overshoot made a push feel too springy. The bump first used the playful spring at about 3 px, which felt too intense. It now uses `NUDGE` too, at about 2 px. All of these are velocity kicks on springs at rest at 0, so repeated nudges blend instead of restarting. A nudge is dropped while the island is held or has a popover. `volume` nudges on each server reading where the default sink's level moved, so its own scroll steps, media keys and other mixers all look the same. Its optimistic update doesn't nudge, so nothing nudges twice. Each change also shows the percent instead of the icon for 1.5 s, in the same space (D-33). The plugin owns that, because it's content, not motion. Its timer is `eave_api::time::sleep`, a lazy wrapper around `tokio::time::sleep`: tokio's timer is already in the build through iced's backend, so it adds no code. It's lazy because tokio's own `sleep` registers with the runtime as soon as it's called, and panics outside it. `workspaces` nudges right or left when the active workspace's id goes up or down.

**How it gets there:** the host numbers every nudge from one counter and keeps each instance's latest one. `view` hands an island its items' latest nudges, and the island plays the ones numbered above the last it saw. A brand-new island (a new surface, a reload) notes the current number without playing anything, so old nudges never replay. The island already steps on `RedrawRequested` and asks for frames until its springs rest (D-26), so a nudge needs no frame source of its own.

**Drawing:** a canvas frame clips to its size, so the squircle's frame now has `NUDGE_BUMP_ROOM` (4 px) above and below the shape. That fits inside `EDGE_GAP` above and `DRAG_ROOM` below, so the layout doesn't change. The `Cache` is cleared whenever the bump changes, which means every frame while it moves and once when it flattens. At rest it's built once, as before.

**Alternatives:**
- a general "animate" or "shake" API with amplitudes and durations: plugins would choose motion, which D-16 rejected.
- a separate attention signal (a pulse or glow) without a direction: it loses the sense of where things went, which is the point of the workspace push.
- nudging from the volume plugin's own `change()`: it misses changes made elsewhere. Nudging on both would double up on our own steps.

**Measured** (aarch64 Asahi, release, the default binary with `~/.config/eave/config.toml`, `main` and this branch in turn, read after 6 s): binary 7,433,240 bytes before and after, RSS 14.1 MB before and 14.2 MB after (noise), idle CPU time 0 in both. That was the first version. With the warp, the damped springs and the volume percent, the binary is 7,498,776 bytes (+64 KB, mostly lyon's curve flattening, which is now linked in). RSS and idle CPU haven't been measured again yet.

---

## D-33: Counters count, alternates fade

**Status:** accepted (2026-10-12)

**What:** two more primitives, added for the volume item but open to any plugin:
- **`ui::counter(value, range, unit)`** is a number with a unit, drawn like a label (`counter.rs`). It keeps room for the widest number in `range` (its digit count times the widest digit, plus the unit), and the number is centered in that room. So it holds its width as it counts, and the island doesn't twitch at 9→10 or 99→100. The range says what the number *is*, not a size, so the plugin still doesn't pick pixels. When the value changes, it counts there through every number in between (47, 48, 49 …) over `COUNTER_COUNT` (320 ms, cubic ease-out), so it's quick at first and slows into the new number. A change mid-count carries on from the number on screen.   - *Superseded:* an odometer, where each changed digit slid out of the line and the new one slid in, staggered from the ones place leftward. It was dropped for counting, which reads as the level moving rather than as digits being replaced.
- **`ui::alternate(options, current)`** shows one option at a time (`alternate.rs`). When `current` changes, the old option fades out and then the new one fades in, over `ALTERNATE_FADE` (240 ms). It's as big as its biggest option, with each option centered in it, so the island holds still when it changes. *Superseded:* sizing to the current option, with the island morphing to fit. The island changing width at every swap was distracting. Only the option on screen gets events, so a counter that comes in starts counting as it appears, from the number it last showed.

The volume item is now `alternate([icon, counter(volume, 0..=max, "%")], showing_level)`, where `max` is the configured cap (or the level, if something else set it higher). Since D-35 it also shows the counter while hovered (`.on_hover(1)`).

**Why primitives, not plugin drawing:** the same reasons as the switcher (D-16) and icons (D-29). The plugin says *a number* and *one of these*, and the motion stays the host's.

**Why fade out then in, not a crossfade:** iced has no group opacity. Content fades the way island items do (D-27): it's covered with the island's own color. A cover over one option would cover the other too wherever they overlap, so the two can't be on screen at once. Like icons' cut-outs, `alternate` assumes it sits on an island.

**Cost:** a counter shapes its text again only when the number on screen changes, at most once per number counted through. Frames are asked for only while something counts or fades.

---

## D-34: Width groups, set in config.toml

**Status:** accepted (2026-10-12)

**What:** an entry's `width_group = "name"` is read by the host, not passed to the widget. Items in the same island with the same group get slots as wide as the widest of them, and each item is centered in its slot. `Island::layout` lays every item out first, then places them by slot (`slots()`).

**Why:** the volume item keeps room for its percent (D-33), so its icon had visibly more space around it than the Wi-Fi icon next to it. Grouping gives its neighbors the same room, so the gaps match again.

**Why the user decides, in config:** which items should line up depends on the layout, so it belongs in the layout file. A plugin can't know its neighbors, and plugins never pick sizes (D-13).

**Alternatives:**
- a minimum width per item: a number to keep in sync with a font and an icon set, by hand.
- "as wide as widget X": a reference with a direction, and broken when X is hidden or removed. A group just has members.
- sizing the volume item to its icon, and letting the number overflow or the island morph: rejected. The width changed with every swap.

**Cost:** one extra pass over the items' widths per layout. The click target is still the item's own width, centered in the slot.

---

## D-35: Hover shows an alternate

**Status:** accepted (2026-10-12)

**What:** `Kind::Alternate` gains `hovered: Option<usize>`, set with `.on_hover(index)`. While the pointer is over the item, the alternate shows that option instead of `current`, with the usual fade (D-33). `ui::hover(normal, hovered)` is the short form for two options. A new `Tone::Good` (green) is for healthy or gaining states. With these:
- `volume` is `alternate([icon, counter], showing_level).on_hover(1)`: the percent on hover as well as after a change.
- `battery` shows only its icon, `Good` while charging and `Critical` at or below `critical` (10%). Hovering shows the percent. *Superseded:* the icon and the percent side by side.
- `clock` shows `hover_format` (default `%b %-d`, `Oct 21`) on hover.

**Why on `alternate`, not a node of its own:** volume needs both "after a change" and "on hover" to show the same counter. As two nested nodes there would be two counters and a pointless fade between them when hover starts during a change. One alternate with one target (`hovered` if hovered, else `current`) keeps a single counter, and a fade reverses mid-way the same way it does for `current`.

**Why the slot decides what "hovered" is:** the alternate's own bounds are only the text, but the pointer usually comes from the screen edge, above it. The item's `Slot` already is the click target, the full island height plus `ITEM_PADDING`, so that's the hover area too. The slot tracks `cursor.is_over` its bounds. Only when that changes does it run `SetHovered` (an iced `Operation`) over its content, which sets a flag in each alternate's state, and ask for a redraw. Alternates in popovers have no slot and never hover. *Rejected:* hover over the alternate's own bounds, which misses the edge; and letting the plugin track the pointer, which would wake the app on every move and give plugins a pointer they don't need.

**The slide:** a change from hover moves with the pointer. The slot works out which way it was going as it crossed the item's edge: the axis it moved most on between the last two events, or, coming from off the surface, the item's nearest edge to the pointer (relative to the item's size, so a wide item doesn't favor its top and bottom). The old option slides out that way as it fades (ease-in), and the new one comes in from the other side (ease-out). The slides are `ALTERNATE_SLIDE_X` (6 px) sideways and `ALTERNATE_SLIDE_Y` (4 px) up or down, which keeps them inside the island. They're clipped to that reach. A change not caused by hover, like volume's level, fades in place. Changing back mid-fade turns around from what's on screen: the same option, at the same opacity and offset, heads back the way it came. Before this, going back in the first half of a fade snapped the old option back to full, and in the second half restarted at full.

**Cost:** a bool compare per pointer event over the bar, inside the widget tree. Entering or leaving an item redraws for one fade (240 ms). Nothing reaches `update` in the app.

## D-36: Popovers take the keyboard while open

**Status:** accepted (2026-10-12)

**What:** while a popover is open, its bar surface has `KeyboardInteractivity::Exclusive`. Any key press, Esc or another, closes it. The host tracks which surface holds the keyboard (`Bar::keyboard`). After every update, `hand_keyboard` gives it to the surface with the open popover and sets the one that had it back to `None`. That covers a popover moving to another monitor's bar, too. The keyboard is handed back as soon as the popover starts closing, not when its island is back. Key events are only listened to while a popover is open, like pointer events (D-14).

**Why `Exclusive`:** D-14 tried `OnDemand` and found that with `input:follow_mouse = 1`, hovering a window takes the focus away. Exclusive focus stays on the layer surface whatever the pointer does. We react to key presses only, never to focus changes, so hover can't close anything.

**Trade-offs:**
- The key that closes the popover is consumed, so it doesn't reach the window that had focus. Hyprland handles its own keybinds before clients, so Super+… binds still work. A lone modifier press closes the popover too.
- A popover can't hold text input that ignores keys. A future search field would need its own rule.

**Rejected:** a Hyprland submap with a `catchall` bind, set while a popover is open, like the click bind. It would take over the user's own submaps, and would also consume the key.

**Measured** (Asahi, release, the user's config, no popover open): binary 7,498,776 bytes, the same as before. RSS ~14.3 MB for both main and this branch, run one after the other.


## D-37: A charging battery ripples and gulps

**Status:** proposed (2026-10-12)

**What:** while `Icon::Battery { charging: true }` is on screen, the icon is a liquid being poured. The edge of the charge always ripples gently (0.3 px, 1.2 Hz). Every `BATTERY_GULP_EVERY` (8 s) it gulps `BATTERY_GULPS` (3) times, `BATTERY_GULP` (380 ms) each and a little weaker each time. With every gulp the charge surges 0.5 px toward the nub and the whole icon swells from its middle (12% taller, 6% narrower). The pour replaces the charging bolt, which is gone: the motion and the `Good` tone say "charging" on their own. All of this is in the host (`icon.rs`). The plugin still only says `charging`, so no plugin draws or animates anything. Once charging stops, it asks for no more frames.

**15 frames a second, not the frame clock:** the animation asks for its next frame `BATTERY_FRAME` (1/15 s) later (`RedrawRequest::At`), not for the compositor's next frame. This is a deliberate exception to "in sync with the compositor's frame clock" ([vision.md](vision.md)), made by the user to keep a never-ending animation cheap. When something else animates, the battery is drawn on those frames too, at its position for that moment.

**Cost:** measured on Asahi (aarch64, release, 120 Hz, scale 2, the user's config, charging), 30 s averages after a 10 s warm-up, all in one session:

| Build | CPU while charging |
|---|---|
| main | 0.4%, 1.5% |
| 30 fps | 3.5%, 5.3% |
| 15 fps (this) | 4.4%, 4.0%, 4.1% |

So it costs **~3–4% CPU for as long as it's charging**. Not charging: unchanged. 15 and 30 fps are not distinguishable through the noise, so a frame's cost isn't the whole cost. Not looked into yet. The 120 Hz version read ~12% with an earlier script that rounded down to whole percents. We accept the cost because it only runs on AC power. Binary unchanged (7,498,776 bytes), RSS within noise.

**Superseded first version:** gulps only, in bursts, with the ripple settling after each one and no frames in between (`RedrawRequest::At`). It cost ~5% (7% with longer, more frequent bursts). The user found the gulps too faint and the liquid too lively, and wanted it to ripple all the time, but more subtly.
