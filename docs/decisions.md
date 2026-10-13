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

**What:** a plugin is a Rust crate in a git repository, with an `eave-plugin.toml` manifest at the root *(superseded by [D-46](#d-46-collections-several-plugins-per-repository-one-widget-per-plugin): the manifest sits beside the crate's Cargo.toml, and a repository can hold several plugins)*. It depends on `eave-api = "0.x"` and exports `pub fn register(&mut eave_api::Registry)`. Users list plugins in `~/.config/eave/plugins.toml` *(superseded by [D-30](#d-30-plugins-are-referenced-where-theyre-used-in-configtoml): config.toml's `widget = "owner/repo"` names them now, and plugins.toml is gone)*. `eave-build` clones them, checks each manifest's `api` against the eave checkout, generates a small crate whose `main` calls `eave::run(&[a::register, b::register])`, and builds it with cargo (the model is Caddy's xcaddy). The generated `[patch.crates-io]` points `eave-api` at the eave checkout, so the host and every plugin share one copy. That works even though eave-api isn't on crates.io (tested).

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

**Superseded for workspaces by [D-38](#d-38-app-icons-found-and-drawn-by-the-host)** (2026-10-12). Once workspaces showed their app icons, dimming the ones you aren't on made the active one clear enough, so the workspaces widget dropped the switcher, its mark and the accent color. The switcher primitive stays for other "one of N" choices.

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
- Keep NetworkManager and add iwd's D-Bus API as a fallback: two backends that still miss systemd-networkd and dhcpcd, and a "which daemon is this?" check on every machine. *(Partly superseded by D-42: the item and details stay kernel-only, but the popover's Wi-Fi controls use iwd when it's there.)*
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
- **Drag:** while the button is held, the island is pulled toward the pointer through a rubber band. It moves almost 1:1 at first, then approaches `DRAG_LIMIT_X`/`DRAG_LIMIT_Y` without reaching them. *(Superseded by [D-45](#d-45-drags-stretch-near-the-pointer-popovers-too): only part of the drag moves the whole island; the rest stretches it near the pointer.)*
- **Release:** a damped spring (`SPRING_STIFFNESS`, damping ratio `SPRING_DAMPING` 0.5) brings offset and squeeze back, with a small overshoot.
- **Frames and events:** the springs step on `RedrawRequested`, like the switcher (D-16). A frame is requested only until every spring is at rest. The island only *watches* pointer events and never captures them, so items handle their own clicks.
- **Long press:** an item's primary click (its `on_press`, or opening its popover) now fires on *release*, and only if the press was shorter than `LONG_PRESS` (400 ms) and the pointer is still over the item. Pressing and holding to play with an island therefore opens nothing. A drag that ends off the item cancels too, like a normal button. Right click and scroll still act at once.
- **Surface:** it gained `DRAG_ROOM` (6 px) of transparent space below the islands, so an island can dip down without being cut off. The exclusive zone covers it, so the strip never sits over windows. Windows start 6 px lower. Popovers drop their top margin by the same amount, so the island-to-popover gap stays at `PANEL_GAP`. *(Superseded by [D-45](#d-45-drags-stretch-near-the-pointer-popovers-too): `DRAG_ROOM` is now 14 px and only 6 of it, `RESERVED_ROOM`, is in the exclusive zone. Windows stay where they were.)*

**Alternatives:**
- quads with a bigger radius: iced quads only do circular corners, so the corners always meet the edges with a visible kink.
- a vector or raster image for the background: it has to stretch to any width, so it would still need a path.
- a surface taller than its exclusive zone: the extra strip would sit over the top of windows and swallow their clicks, because we can't set an input region through iced_layershell. *(Wrong, corrected by [D-28](#d-28-popovers-grow-out-of-their-island-on-the-bar-surface): iced_layershell 0.19 has `SetInputRegion`. Keeping `DRAG_ROOM` inside the exclusive zone is still simpler.)* *(Taken after all by [D-45](#d-45-drags-stretch-near-the-pointer-popovers-too), with the extra strip outside the input region.)*
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

**What:** `widget = "…"` in config.toml both places a widget and says where it comes from: `<source>[@rev][#widget]`. No `/` is a built-in (`plugins/<name>` of the eave checkout; *since [D-46](#d-46-collections-several-plugins-per-repository-one-widget-per-plugin), the plugin of that name in the eave checkout's collection*), `owner/repo` is GitHub, `host/path` and anything with a `:` are git URLs, and a path starting with `/`, `~` or `.` is a local directory. The widget is the repository's name without `.git` and a leading `eave-`, unless `#widget` names it. *(Since [D-46](#d-46-collections-several-plugins-per-repository-one-widget-per-plugin), that name also picks the plugin, and eave-build checks it against the manifests.)* A top-level `eave = "<source>[@rev]"` picks eave's own source, and defaults to `eaveshell/eave`. `eave-build` reads the same file: it compiles in exactly the widgets the config uses, built-ins included, and errors when one source is asked for at two revisions. The bar looks widgets up by the name part (`config::widget_name`). eave-build has its own copy of the grammar, because depending on `eave-api` would pull iced into a CLI. Both copies are tested against the same cases. `eave-rebuild.path` now watches config.toml.

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
- Git sources are cloned once into `$XDG_DATA_HOME/eave/src` and then left alone. Without a `@rev` a checkout stays at the commit it has. A `@rev` that isn't there yet (a pin just edited) is fetched. Only `eave-build update` fetches everything, moves each source to the newest commit of its `@rev` or default branch, and prints `old → new (N new commits)`. When eave itself moved, it also runs `cargo install --locked --path <eave>/crates/eave-build`, after the bar is in place. *Since [D-53](#d-53-eave-on-the-aur-shipping-eave-build-from-release-tags):* only when eave-build runs from cargo's bin directory and isn't a packaged build. A package manager updates it otherwise.
- `eave-build` (which replaces `--install` and `--restart`) always builds, then compares the result with `~/.local/bin/eave` byte for byte. It installs and restarts the bar only when they differ.
- `eave-build init` writes a starter config.toml (`dist/config.toml`, kept if one exists) and the three systemd units. `install.sh` is a thin POSIX script around it: check for `cargo` and `git`, `cargo install --git … eave-build`, `init`, build, enable the units. Re-running it never updates.
- The rebuild unit runs eave-build by its full path, which `init` writes in (2026-10-12). *Superseded:* `ExecStart=eave-build` with `Environment=PATH=%h/.cargo/bin:…`. systemd 262 resolves a bare name only in its compiled-in search path, never through the unit's `PATH`, so every rebuild failed with status 203/EXEC. Since the units can change, `eave-build update` runs the new eave-build's `init` after reinstalling it, then `systemctl --user daemon-reload`.

**Alternatives:**
- fetch on every build (as before): saving config.toml could upgrade eave and every plugin behind your back, through the path unit.
- a lockfile recording each source's commit: reproducible across machines, but one more file. The checkouts already are the lock on one machine, and pins cover the rest.
- compare a hash of the generated crate instead of the binary: misses changes in local-path sources, which is how plugins are developed.
- a distribution package (AUR): later. It doesn't fit compiled-in plugins without a toolchain anyway. *Superseded by [D-53](#d-53-eave-on-the-aur-shipping-eave-build-from-release-tags):* the package ships eave-build and keeps the toolchain as a runtime dependency.

**Why:** the path unit now fires on every config save, so a build has to be cheap and inert when nothing changed. Explicit updates keep the bar from changing unless you asked. Sources moved out of the cache because clearing a cache must never change versions.

**Gotcha:** `eave-build` embeds `dist/` with `include_str!`, so it builds only from the repository (`cargo install --path` or `--git`), not as a standalone crate.

---

## D-32: Plugins can nudge their island

**Status:** accepted (2026-10-12)

**What:** `Effect::nudge(Nudge)` lets a widget react physically to its own events. `Nudge` is `Up`, `Down`, `Left` or `Right`. It says which way something went, not how to move. The host maps it:
- **`Up` / `Down`:** the island's top bulges up above the item, or its bottom dips below it, about 2 px, then settles flat. The bump is a raised cosine `NUDGE_BUMP_WIDTH` (64 px, *now 176*, see below) wide, and the squircle path *warps* around it: every point moves vertically by the bump's height at its x. The full height applies on the bumped edge, eased down to nothing halfway across the shape. So a bump next to a corner bends the end of the corner along with it and keeps its peak over the item, and the opposite edge never moves. The warp flattens the path into pieces of at most 1 px, so it only runs while a bump shows.
  - *Superseded on the way:* a hump confined to the straight part of the edge. Clamped clear of the corners, it slid away from an item near the island's end. Cut short on the corner side instead, it stayed over the item but rose steeply on that side.
  The whole island also moves about half a pixel the same way (one physical pixel at scale 2).
  The nudged item rides along with its bump, by `NUDGE_ITEM_FOLLOW` (half) of it. *Superseded by the jelly below, which gives the same half by itself.*
- **`Left` / `Right`:** the whole island travels about 5 px that way and comes back.
- **The outline is a jelly** (2026-10-13, `jelly.rs`). It is a ring of springs all the way round the island's edge: 24 across the top and bottom, 6 down each side, each one how far out (or in) its piece of the edge is. Each is pulled back to 0 (`JELLY_STIFFNESS` 600, `JELLY_DAMPING` 0.85, the NUDGE feel) and toward its neighbors (`JELLY_COUPLING` 1400). The pull toward the neighbors does the smoothing. A nudge just pokes the ring with velocity kicks, and the ring spreads the poke along the edge and round the corners into a soft curve, then settles flat. Nothing that pokes it decides what curve it makes. The ring is drawn by warping the squircle through it: each side moves the points near it, eased out over half the shape's short way, and the ring is sampled smoothly between springs (Catmull-Rom).
  - **Up / down:** the edge over the item is kicked in a raised cosine `NUDGE_BUMP_WIDTH` wide (`JELLY_BUMP`, peaks about 2 px). `NUDGE_BUMP_WIDTH` went 64 → 112 → 176 px (2026-10-13): the user wanted the bump smoother and larger, then wider still. Wider kicks still peak about 2.1 px (2.05 before), since kicks bring springs up to their speed rather than add, so it's a gentler slope, not a taller one. Both sides are kicked to lean the same way (`JELLY_LEAN`), so the edge it's going toward widens, about 1.3 px each side.
  - **Left / right:** the top and bottom are kicked to lean (`JELLY_LEAN`, a sine from the end it's leaving to the end it's going toward). The leading end swells about 1.6 px each way, so it's about 3 px taller, and the trailing end flattens as much.
  - Items read the ring at their center (`Jelly::at`). They move by how far its middle moved there, so an item under a 2 px bump rises 1 px, as `NUDGE_ITEM_FOLLOW` used to do by hand. They squish by how much taller the shape is there (D-40's per-leaf squish).
  - Kicks in a row don't pile up. Each brings a spring up to its speed rather than adding to it (as the old bump did with `max`), and less of it the further out the spring already is. *Superseded:* adding kicks up. Holding the volume key ran the bump into the end of its room, where the hard limit flattened it frame to frame, and it flickered. Now a nudge every three frames levels off around 2.8 px.
  - The ring stays inside the frame's `NUDGE_BUMP_ROOM`, which now goes round the sides too. While it moves, the shape cache is rebuilt every frame through a flattened, warped path, as the bump already was. At rest it's all zeros and draws the cached squircle.
  - *Superseded:* the bump was its own spring with a fixed raised-cosine shape (`squircle::Bump`), and the item followed it by `NUDGE_ITEM_FOLLOW`. A first lean, the same day, was a linear taper of the whole path read straight off the push springs' displacement (`squircle::Taper`). It had no motion of its own, so it snapped in with the kick, and the linear keystone sheared the corners. The user found it not smooth and asked for one mechanism that smooths out any change to the shape by itself. Both special cases became pokes on the jelly.
  - Measured (Asahi, release, the user's config): binary 8,350,784 bytes before and after (the jelly is a few hundred bytes of code; no dependencies). RSS 16.3 MB 10 s after start (15.9 before). Idle CPU 0–1 ticks a second when nothing moved, the same as `main`'s bar running beside it, though not a clean 20 s run: the bar was being tried at the time. While it wobbles, each frame flattens and warps the path, as the bump already did.

The island's movement uses springs of its own (`push_x`, `push_y`), added to the drag offset. Their feel is `NUDGE`: stiffness 600, damping ratio 0.85, so the island comes back without swinging past. *Superseded:* kicking the drag spring itself. Its playful overshoot made a push feel too springy. The bump first used the playful spring at about 3 px, which felt too intense. It now uses `NUDGE` too, at about 2 px. All of these are velocity kicks on springs at rest at 0, so repeated nudges blend instead of restarting. A nudge is dropped while the island is held or has a popover. `volume` nudges on each server reading where the default sink's level moved, so its own scroll steps, media keys and other mixers all look the same. Its optimistic update doesn't nudge, so nothing nudges twice. Each change also shows the percent instead of the icon for 1.5 s, in the same space (D-33). The plugin owns that, because it's content, not motion. Its timer is `eave_api::time::sleep`, a lazy wrapper around `tokio::time::sleep`: tokio's timer is already in the build through iced's backend, so it adds no code. It's lazy because tokio's own `sleep` registers with the runtime as soon as it's called, and panics outside it. `workspaces` nudges right or left when the active workspace's id goes up or down.

**Paced nudges** (2026-10-13): `Effect::nudge_lasting(nudge, duration)` says how long the change the nudge follows takes on screen. The island keeps a `pace`, that duration over `NUDGE_LASTING` (190 ms, about when a plain push is back within 5 % of its peak), clamped to 0.5–5. The push springs and the jelly run on time divided by the pace, with the same kicks, so the motion goes just as far and only takes longer. A nudge without a duration sets the pace back to 1. `workspaces` reads Hyprland's `workspacesIn` animation from `j/animations` (following unset ones up to `workspaces` and `global`; speed is in deciseconds), with its snapshot and again on `configreloaded`, and nudges over that long. With the user's `workspaces` speed of 4 (400 ms), the push peaks around 90 ms instead of 43 and settles with the workspace slide. The user found the push too quick next to Hyprland's own animation. This is the plugin describing what happened (how long it takes), not styling the motion: the host still picks the motion. *Alternative:* a fixed slower `NUDGE` feel for pushes. It would drift out of step whenever the user changes their Hyprland animations. One more socket1 request per snapshot, no new dependencies.

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

**Revised by [D-39](#d-39-expand-is-a-growing-alternate-secondary-clicks-on-buttons)** (2026-10-12): the slot still is the only thing following the pointer, but `SetHovered` now hands alternates the pointer and the item's bounds, and each alternate decides whether it's hovered. Ordinary ones still go by the whole item, as above. Growing ones (`expand`) go by their own width, and wait for the pointer to rest.

## D-36: Popovers take the keyboard while open

**Status:** accepted (2026-10-12)

**What:** while a popover is open, its bar surface has `KeyboardInteractivity::Exclusive`. Any key press, Esc or another, closes it. The host tracks which surface holds the keyboard (`Bar::keyboard`). After every update, `hand_keyboard` gives it to the surface with the open popover and sets the one that had it back to `None`. That covers a popover moving to another monitor's bar, too. The keyboard is handed back as soon as the popover starts closing, not when its island is back. Key events are only listened to while a popover is open, like pointer events (D-14).

**Why `Exclusive`:** D-14 tried `OnDemand` and found that with `input:follow_mouse = 1`, hovering a window takes the focus away. Exclusive focus stays on the layer surface whatever the pointer does. We react to key presses only, never to focus changes, so hover can't close anything.

**Trade-offs:**
- The key that closes the popover is consumed, so it doesn't reach the window that had focus. Hyprland handles its own keybinds before clients, so Super+… binds still work. A lone modifier press closes the popover too.
- A popover can't hold text input that ignores keys. A future search field would need its own rule. *(Revised by D-41: keys stay with a focused text field, and Esc always closes.)*

**Rejected:** a Hyprland submap with a `catchall` bind, set while a popover is open, like the click bind. It would take over the user's own submaps, and would also consume the key.

**Measured** (Asahi, release, the user's config, no popover open): binary 7,498,776 bytes, the same as before. RSS ~14.3 MB for both main and this branch, run one after the other.

---

## D-37: A charging battery ripples and gulps

**Status:** proposed (2026-10-12)

**What:** while `Icon::Battery { charging: true }` is on screen, the icon is a liquid being poured. The edge of the charge always ripples gently (0.3 px, 1.2 Hz). Every `BATTERY_GULP_EVERY` (8 s) it gulps `BATTERY_GULPS` (3) times, `BATTERY_GULP` (380 ms) each and a little weaker each time. With every gulp the charge surges 0.5 px toward the nub and the whole icon swells from its middle (12% taller, 6% narrower). The pour replaces the charging bolt, which is gone: the motion and the `Good` tone say "charging" on their own. All of this is in the host (`icon.rs`). The plugin still only says `charging`, so no plugin draws or animates anything. Once charging stops, it asks for no more frames.

**The percent gulps too:** hovering a charging battery shows its percent wrapped in `ui::gulp(content)`, a new word in the vocabulary (`gulp.rs`). The host swells the content from its middle (up to `GULP_SWELL`, 8%) and back, slowly (`GULP`, 700 ms each, against the icon's 380 ms), one gulp after another with no rest, for as long as it's on screen. The swell is only drawn, so the layout doesn't move. Unlike the icon, it follows the compositor's frame clock: it only runs while the pointer is over the battery, so its cost lasts as long as the hover does. Inside an alternate that shows something else, it gets no frames and asks for none.

**30 frames a second, not the frame clock:** the animation asks for its next frame `BATTERY_FRAME` (1/30 s) later (`RedrawRequest::At`), not for the compositor's next frame. This is a deliberate exception to "in sync with the compositor's frame clock" ([vision.md](vision.md)), made by the user to keep a never-ending animation cheap. When something else animates, the battery is drawn on those frames too, at its position for that moment.

**Cost:** measured on Asahi (aarch64, release, 120 Hz, scale 2, the user's config, charging), 30 s averages after a 10 s warm-up, all in one session:

| Build | CPU while charging |
|---|---|
| main | 0.4%, 1.5% |
| 30 fps (this) | 3.5%, 5.3% |
| 15 fps | 4.4%, 4.0%, 4.1% |

So it costs **~3–4% CPU for as long as it's charging**. Not charging: unchanged. 15 and 30 fps are not distinguishable through the noise, so a frame's cost isn't the whole cost, and we keep 30 fps, which moves more smoothly. Not looked into yet. The 120 Hz version read ~12% with an earlier script that rounded down to whole percents. We accept the cost because it only runs on AC power. Binary unchanged (7,498,776 bytes), RSS within noise.

**Superseded first version:** gulps only, in bursts, with the ripple settling after each one and no frames in between (`RedrawRequest::At`). It cost ~5% (7% with longer, more frequent bursts). The user found the gulps too faint and the liquid too lively, and wanted it to ripple all the time, but more subtly.

---

## D-38: App icons, found and drawn by the host

**Status:** accepted (2026-10-12)

**What:** `ui::app_icon(app_id)` adds an application's icon to the vocabulary. The plugin names the app by its window class or app id. The host finds the icon, loads it and draws it at `ICON_SIZE`. `crates/eave/src/apps.rs` finds the app's desktop entry in `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`. It matches the file name first, then `StartupWMClass`, then the name ignoring case or a reverse-DNS prefix, and reads `Icon=`. That name is looked up in the user's icon theme and the themes it `Inherits=`, then in `hicolor`, then in `pixmaps`. The best file is the smallest one at least 32 px, or a scalable one, with SVG winning ties. It falls back to the biggest. The theme is `gtk-icon-theme-name` from GTK 3's (or GTK 4's) `settings.ini`. The lookup runs on tokio's blocking pool, once per app id. The result is drawn there, once, into a 32 px square bitmap: SVGs with resvg, and PNGs halved bilinearly until close, then fitted bicubically. The host keeps the bitmaps in `Bar::icons`, shared by every widget. After each update it walks the items, and the open popover, for app ids it hasn't loaded or started loading. A reload clears the cache, so a new icon theme shows. Until an icon has loaded, or when there is none, the host shows the id's first letter in the same box, so nothing moves when it arrives. `Tone::Dim` draws an icon at `DIM_APP_ICON` opacity (0.45), and other tones leave it as it is: an app icon keeps its own colors.

The workspaces widget is its first user. Each workspace shows its name and the icons of its apps, two at most and one per app, then `+n`, where n counts the windows those icons leave out. Resting the pointer on a workspace shows one icon per window (`expand`, [D-39](#d-39-expand-is-a-growing-alternate-secondary-clicks-on-buttons)). Right-clicking one lists its windows with their titles, kept live by `windowtitlev2`, and clicking a window there focuses it. Workspaces you aren't on are dimmed. That replaces the switcher's mark and the accent color (D-16).

**Why the host and not the plugin:** "plugins describe, the host draws" (D-13). Plugins would otherwise each read the filesystem and hand raw images to the kit. The tray and a future dock need the same lookup.

**Why `settings.ini` and not gsettings:** gsettings means dconf, a daemon or a binary, which D-23 rules out. Every GTK setup writes `settings.ini`, nwg-look and lxappearance included. Without one, `hicolor` still has almost every app's own icon.

**Why resvg after D-29 said no:** D-29 turned down SVG for five status glyphs that are simpler as paths. App icons are third-party art, and most themes ship only SVGs. We use resvg directly with `default-features = false`, so no text, no system fonts and no embedded raster images, and draw into a bitmap ourselves. We tried iced's `svg` feature first. It turns on resvg's defaults (rustybuzz, fontdb loading, gif, jpeg, webp) and costs +1.94 MiB of binary. It would also keep every parsed SVG tree in memory and render on the UI thread. PNG decoding comes from tiny-skia's own `png-format`, which resvg already pulls in. Drawing the bitmaps needs iced's `image-without-codecs`.

**Measured** (Asahi, release, the user's config: workspaces + volume + network + battery + clock, two to three workspaces with windows; each run 10 s after a 5 s settle, alternating):
- binary: 7,498,776 → 8,285,248 bytes (+0.75 MiB). With iced's `svg` feature it was 9,530,432 bytes.
- RSS: ~14.3 MB → ~15.7 MB.
- idle CPU: unchanged, 0.2–0.4 % in quiet runs for both.

**Alternatives:**
- An icon font: it can't hold arbitrary apps' icons.
- Plugins passing image bytes: that breaks D-13, and every plugin would repeat the lookup.
- `freedesktop-icons` or `linicon` crates: they're thin wrappers over the same lookup, without our size choice, and one more dependency for ~150 lines.
- Following the full icon theme spec (`MinSize`/`MaxSize`/`Threshold` matching): we only ever want 32 px, so "smallest at or above 32, else the biggest" is enough.

---

## D-39: `expand` is a growing alternate; secondary clicks on buttons

**Status:** accepted (2026-10-12)

**What:** `ui::expand(collapsed, expanded)` is an alternate (D-33) with `grows: true`. `Kind::Alternate` gains that field, and `expand` builds `alternate([collapsed, expanded], 0).on_hover(1)` with it set. So it fades and slides exactly like `hover` on volume, battery and clock, and turns around mid-fade the same way. A growing alternate differs only where its changing size calls for it:
- **Size:** it's as wide as the option on screen, not the widest one. The height stays the tallest, so nothing jumps up or down. The width changes at the fade's midpoint, when the new option comes on screen. The island springs to it (D-27), so the old option fades out at its own size and the new one fades in as the island grows or shrinks.
- **Hover area:** it's hovered by the pointer over its own width, at any height within the item, not by the whole item. The full height still counts, so a pointer pushed against the screen edge hovers it, and each workspace in a row has its own hover.
- **Rest:** it changes only once the pointer has rested on it for `HOVER_REST` (200 ms), and changes back as soon as the pointer leaves. It waits with `request_redraw_at`, never polling.

There's still one hover path for everything. `Slot` follows the pointer, and nothing else does. `SetHovered` no longer carries a verdict. It carries the item's bounds, the pointer now and before, and the time, and each alternate decides for itself: an ordinary one by the item, a growing one by its own width. The slide direction (`toward`) moved from the slot into the alternate, so it's worked out from whichever region was crossed. The slot runs `SetHovered` when the pointer comes onto or leaves the item, as before. It also runs it on moves within the item, but only when the item has a growing alternate, which it learns through a `HasGrowing` query once per view. It asks for a frame only if an alternate reports a change.

`button(...).on_secondary(message)` sends a message on a right click on that button. The kit wraps the button in a `mouse_area`. A popover opens on the bar where its item was last pressed. A press on an inner button is captured before `Slot` sees it, so `Slot` now sends `Touched(window, id)` before its content sees any press on the item. That lets a right click on a workspace open the popover even if the item was never left-clicked.

**Why the rest:** growing shifts everything after it. Without the rest, sweeping the pointer across the workspaces opens each in turn, and the one you're heading for keeps moving away. Only growing alternates wait, because only they move their neighbors. The volume percent still shows at once.

**Superseded:** the first version of this decision, on the same branch. `expand` was its own node and its own host widget (`expand.rs`). It tracked hover itself, by checking the pointer against its own bounds, and swapped its two views instantly. That made two hover paths and two motions, so an expanding workspace moved differently from every other hover in the bar. Any plugin choosing between `hover` and `expand` would have got different behavior. The rule is one behavior for every plugin: one hover path, through the slot, and one widget that changes what it shows, the alternate. The original text:

> `ui::expand(collapsed, expanded)` shows `collapsed`, or `expanded` once the pointer has rested over it for `EXPAND_DELAY` (200 ms). It collapses as soon as the pointer leaves. It's only as big as what it shows, so the item grows and the island springs to its new width (D-27). It counts as hovered across its whole width, from the top of the bar to its bottom edge, so a pointer pushed against the screen edge still hovers it. `crates/eave/src/expand.rs` lays out only the child it shows. When hover changes what it shows, it invalidates the layout, and it waits for the delay with `request_redraw_at`, never polling.
>
> **Why not `alternate`/`hover` (D-33, D-35):** an alternate keeps room for its biggest option, so the item doesn't change size, and it reacts to the whole item. Here the extra room is the point, and each workspace in the row needs its own hover.

---

## D-40: A popover vocabulary: lists, sections, controls, one panel width

**Status:** accepted (2026-10-13)

**What:** popovers were a heading, labels and plain buttons, laid out at their natural size. That can't show a Wi-Fi list with the current network marked and an info button at the right edge, a volume slider, or battery health that reads at a glance. The `ui` vocabulary grows by:
- **Rows:** `list_item(title)` with `.subtitle`, `.leading`, `.trailing`, `.selected` and `.on_press`; `list(rows)`, which scrolls past `LIST_MAX_ROWS`.
- **Structure:** `section(title)` (a small caption, with an optional trailing node), `header(title)` (the page title, with `.on_back` for sub-pages), `detail(name, value)` (a dim name, the value right-aligned), `fill()` (pushes what follows to the end of a row).
- **Controls:** `slider`, `toggle`, `meter`, `text_input` (D-41), and `button(..).prominent()` for the one main action of a page.
- **Emphasis:** `display(node)` (large text: the battery's percent), `mark(node)` (a filled accent circle: today), `badge(text)` (a keyboard layout code).
- **Icons:** lock, info, back, forward, check, refresh, close, reveal, bolt, plug, mic, speaker, headphones, display.
- **`Panel::on_open` / `on_close`:** messages the host sends when the popover opens and starts closing, so a plugin scans or subscribes only while it's open, and resets its pages after.

Every popover is laid out at one width, `PANEL_WIDTH`. Height stays natural. With a known width, rows can push trailing content to the edge, sliders can fill, and the calendar spans the panel. Moving between pages of one popover then only morphs the height.

The designs were agreed first as mockups, kept in `docs/mockups/`.

**Why one width:** the island's overlay laid panels out unbounded, so `Fill` anywhere inside resolved to infinity. Letting plugins pick a width would break D-13. Sizes are the host's.

**Alternatives:**
- A few panel widths chosen by plugins (`Panel::compact()`): the short lists look fine at the one width, so not yet.
- A canvas node: D-13 says primitives first.

**How it's drawn:** `kit::panel` renders a popover's nodes knowing it's in a panel (`kit::node` for the bar collapses `fill` and doesn't scroll lists). New host widgets: `toggle.rs` and `meter.rs` ease on the frame clock and ask for no frames at rest, `field.rs` boxes iced's text input, `clip.rs` cuts a long line and fades its end into what's behind it (iced 0.14 can't ellipsize). The island lays the panel out at `PANEL_WIDTH` and passes the visible part of its shape as the viewport, so a scrolling list clips to the shape while it grows. `Node::map` now takes `Clone + 'static` functions, because sliders and text fields keep it to map the messages they make later. The counter got a size and font for `display`.

**Measured** (Asahi, release, the user's config, alternating runs, no popover open): binary 8,285,248 → 8,547,392 bytes for the kit (+256 KB: iced's text input, slider and scrollable, and the new widgets). RSS ~15.7 → ~16.5 MB, idle CPU unchanged (6–7 ticks per 10 s for both). The rest of the branch's growth is in D-42.

---

## D-41: Text fields in popovers

**Status:** accepted (2026-10-13). Amends D-36.

**What:** `ui::text_input(placeholder, value, on_input)`, with `.on_submit` and `.secret()`. It's for joining a Wi-Fi network with a password. It focuses itself when it appears. D-36's rule changes to: any key closes the popover unless a text field in it is focused. Esc always closes it, focused or not. The island asks the panel's widgets whether a field has focus (a widget operation) and keeps every key while one does, so a lone Shift or an arrow doesn't close it.

**Why Esc always closes:** iced's own text input unfocuses on the first Esc. A second press would then be needed, and closing resets the page anyway.

**Secrets:** a plugin holding a password keeps it out of `Debug`, clears it on submit and in `on_close`.

---

## D-42: Wi-Fi controls through iwd

**Status:** accepted (2026-10-13). Amends D-23 for the network popover.

**What:** the kernel can list networks but can't join a WPA one, so the network popover's controls go through iwd's D-Bus API, in `crates/eave-iwd` on `eave-dbus`: the list of networks with signal, security and whether they're saved; scan; connect (with a passphrase through an iwd agent eave serves); disconnect; forget; auto-join; power. The bar item and the connection details stay kernel-only (`eave-net`). Without iwd, or without permission to use it (iwd allows root and the `wheel` and `network` groups), the popover shows the kernel's details and says Wi-Fi controls need iwd.

The popover talks to iwd only while it's open (D-40's `on_open`/`on_close`). With it closed, nothing listens.

**Why iwd and not NetworkManager:** iwd's API is small (station, network, known network, agent), and NetworkManager can run on top of it. NetworkManager's API is much larger and doesn't cover iwd-only or systemd-networkd setups.

**What we give up:** controls on wpa_supplicant-only machines. They keep the read-only details.

**How:** `eave_dbus::watch_all` re-reads on any signal from iwd (objects come and go while it scans), and the snapshot is one `GetManagedObjects` plus `Station.GetOrderedNetworks` for the signal order. Being turned away (`AccessDenied`) or iwd not running are states, not errors, so the popover can say which. The agent is a zbus interface served on the shared system connection and registered before each password join. iwd sends the request to the agent of whoever called `Connect`, so NetworkManager's agent doesn't get it. Checked live on this machine, where NetworkManager runs on top of iwd: the listing, signal order, saved networks and diagnostics come through.

**Measured** (Asahi, release): the network popover, eave-dbus (first used here) and eave-iwd add ~787 KB of binary (8,612,928 → 9,399,648 bytes): zbus's object server, the `interface` macro's code, the fdo proxies, and zbus's tokio integration. zbus was already in the binary through `mundy`, on async-io. Building eave-dbus on zbus's `async-io` instead of `tokio` saves ~200 KB (9,202,864 bytes) but brings back the second executor D-7 avoided, so it stays on tokio. With the popover closed nothing watches iwd, so idle RSS and CPU don't change.

**Other popovers on this branch:** eave-pulse now lists inputs (monitors left out) and apps' streams, with each device's kind from its port or properties. Its one event thread subscribes to sinks, sources, sink inputs and the server, and tells watchers what changed, so stream events never wake the bar item. Streams are read only while the sound popover is open, and a slider drag sends one level at a time. +64 KB. The battery reads design capacity, cycles, temperature, health, its charge limit and whether a charger is plugged in, from the same sysfs files on the same events. The keyboard reads layout names once from xkb's `evdev.lst`, a static data file, not a daemon.

## D-43: Tabs, and the host owns the current tab

**Status:** accepted (2026-10-13).

**What:** `tabs([(name, page), …])` puts a popover's pages behind a pill-shaped strip of names, one page showing at a time: the battery's Power and Health, the sound popover's Output, Input and Apps. A tinted mark slides behind the current name with the switcher's slide, and the panel's shape eases to the new page's height through the morph it already has. *(It didn't: the surface shrank at once and cut the shape off, and a taller page made the shape start back toward the island for a frame. Fixed by [D-51](#d-51-a-popover-keeps-its-room-until-its-shape-lands-tabs-hand-their-pages-over), which also slides and fades the pages.)* Which tab is current lives in the host's widget state. The panel's state goes away when the popover closes, so each popover opens on its first tab. Pages that aren't showing keep their state (a scrolled list stays scrolled), but get no events and no operations, so a field on a hidden tab can't take focus. In the bar, only the first page shows.

**Why the host owns it:** which tab is on screen is like hover, scrolling or focus: it's about looking, not about the plugin's state. Owning it in the host keeps plugins to describing pages, with no index to store and no message to handle, and it can't go stale against the tabs a plugin shows (a tab that goes away falls back to the first).

**Alternatives:** a `switcher`-style strip where the plugin keeps the index and handles a message per tab: more code in every plugin, for control nobody needed yet (opening on a tab other than the first). An underline under the current name instead of a pill: the pill matches the switcher and the list's selected rows.

**Measured** (Asahi, release): the binary's size didn't change (9,399,648 bytes; the code fits in existing section padding). All pages are described each time a popover is built, but only while it's open, so idle RSS and CPU don't change.

---

## D-44: Popovers squish opening and splat closing

**Status:** accepted (2026-10-12). Numbered D-40 on its branch; renumbered when merged after the popover redesign's D-40–D-43.

**What:** a popover's morph (D-28) gets squash and stretch on top. **Opening squishes on the way:** as it sets off, the shape is squeezed in from the sides (and a little taller, by the bulge), then eases back as it grows, with nothing to hit when it gets there. **Closing splats:** the island lands like it hit the top. The top stays on the bar, and the shape splats up into it, a little flatter and wider, then eases back. The edges, the fades and their timing, `progress` and `PopoverClosed` are untouched: the splat only changes the drawn shape (`Motion::shape`).
- Two springs, `stretch_x` and `stretch_y`, in pixels added to the width and height. They rest at 0 and get one velocity kick each way of the morph, like a nudge (D-32). Opening kicks `stretch_x` down as soon as it has room (`SQUISH_OPEN_SQUEEZE`, 2 × the panel's width per second: about 5 % narrower at the peak, ~70 ms in). Closing kicks flatter (`stretch_y` down) and wider (`stretch_x` up). Their feel (`SQUISH_STIFFNESS` 250, `SQUISH_DAMPING` 0.75) is just under critical, so it squashes and eases back with a rebound of about 3 %, too little to see.
- The closing kick comes when the shape's bottom edge is within `SQUISH_LAND` (5 px) of the island's, so the splat peaks about four frames later, as the shape arrives. `SQUISH_CLOSE_FLATTEN` 50 and `SQUISH_CLOSE_WIDEN` 125 px/s: about 3 px flatter and 4 px wider on a 32 px island, bulge included.
- The top stays put; all the change in height goes to the bottom. The kicks are in pixels, not a share of the size: the surface only has `EDGE_GAP` beside the islands, so a wider splat would be cut off.
- Stretching along one axis pulls the other in by `SQUISH_BULGE` (0.5) per pixel. The shape stretches about its center across.
- *Superseded, same branch, in order:*
  1. Opening squeezed in from the sides and swung wider; closing landed flattened.
  2. An overshoot instead: opening too tall, closing too wide, first with the top pinned (it looked nailed to the bar), then with the top taking 40 % of every change in height (`SQUISH_TOP`).
  3. A splat against the edge each way hit: the bottom when opening (top dipping), the top when closing. That was on a springy spring (400 / 0.35) that swung back visibly. The user wanted it smoother, so the spring went near critical. Then they asked for a smaller closing splat (it was 70 / 180 px/s) and none on opening: opening should squish, not splat. So opening went back to the squeeze of step 1, now on the smooth spring, so it eases back instead of swinging wider.

  The rule that came out of it: overshooting past an edge reads as bouncing through it. Splatting against an edge that stays still reads as hitting it. Only closing, the island returning to the bar, has something to hit. Opening has nothing to hit, so its squish is in the motion, not at the end.
- The island's items ride along and squish with it. `Motion::follow` moves each one to where the stretch takes its center, and the island sets how much the shape is stretched (`Motion::squish`, width and height ratios) around drawing its items. Each thing the host draws squishes itself about its own center, one axis at a time:
  - **Why per leaf:** the renderer only scales the same both ways. iced_tiny_skia reads one `scale_factor()` from a `Transformation`, and draws text at a font size. So nothing outside an item can stretch it along one axis. A canvas frame can (`scale_nonuniform`, as the battery's gulp already did), and an image can be drawn into any rectangle.
  - **Symbol icons** (`icon.rs`) squish their canvas frame. **App icons** are drawn by a small host widget (`apps.rs`, `Picture`) into stretched bounds, not snapped to pixels while squished. **Labels** (`label.rs`) and **counters** are plain text at rest. While squished they draw their text with canvas `fill_text`, which both renderers turn into glyph outlines under a non-uniform transform. Outlines aren't hinted, so text is a touch softer for the moment a squish lasts.
  - **How the squish gets there** (`squish.rs`): a thread-local the island sets around drawing its items and puts back after. It changes every frame, only while drawing, and has to reach leaves inside alternates, buttons and gulps. Widgets have no way to hand something down the tree per draw: operations run at layout time. Drawing is on the one UI thread. Panels aren't squished: their stretch is 1–2 %.
  - *Superseded:* items first scaled uniformly with the shape's height (one `Transformation` per item). Squishing both axes at once looked wrong to the user: an icon shrank instead of squishing.
  - **Measured** (Asahi, release, the user's config, 10 s after start, no popover opened): RSS 15.7 MB on `main`, 15.8–15.9 MB here. Idle CPU the same (11 ticks in 20 s each). Binary 8,285,248 → 8,350,784 bytes (+64 KB, the canvas text path). No new dependencies. At rest every leaf takes the old path. *Gotcha:* the first run or two after a fresh build sometimes sat at 23–33 MB, on the commit before this change too, so compare several runs.
- Turning around mid-morph squishes or splats again. A popover taken away without closing doesn't.
- Hit testing still uses the edges: the difference is a few pixels for a fraction of a second.

**Alternatives:**
- underdamped feels on the edges themselves: the overshoot scales with the distance travelled, so a tall panel would swing past the bottom of its surface and a wide one off the screen edge.
- overshoot driven by the edges' velocity: the morph is near critical and slow by the time it arrives, so there's little to drive it.

**Superseded:** the first version on the same branch squeezed the opening shape in from the sides as it set off (a kick as soon as it had room, by 1.2 × the panel's width per second), so it swung wider as it arrived, and the closing island landed a few pixels flattened. The user wanted it to overshoot instead: past its height opening, past its width closing. Kicking the landing by progress (at 85 %) was also tried; it peaked while the shape was still far taller than the island, where it didn't show, hence `SQUISH_LAND` in pixels.

**Cost:** no dependencies. At rest the stretch springs are 0 and nothing changes. Each open and close animates a little longer, while the last swing settles (the morph test's bound went from 60 to 75 frames).

## D-45: Drags stretch near the pointer, popovers too

**Status:** accepted (2026-10-13)

**What:** a drag no longer moves an island as one rigid piece (D-26). It stretches the island where it was grabbed, like pulling something soft, and it works on an open popover too.
- **The pull:** the drag amount is still the rubber-banded `x`/`y` springs, now on a softer feel of their own (`DRAG_STIFFNESS` 220, `DRAG_DAMPING` 0.7), so the stretch eases after the pointer and back with only a small overshoot. Only `DRAG_FOLLOW` (0.5) of it moves the whole island. The rest moves the outline near the grab point, all of it at the grab and less further away, down to nothing at `DRAG_REACH` (200 px). The falloff is a smootherstep, flat at both ends, so the island bends in one long curve with no visible bend where it was grabbed or where the stretch stops.
  - *Superseded, same branch:* `DRAG_FOLLOW` 0.35, `DRAG_REACH` 60 px with a smoothstep, on the press's springy `PLAYFUL` feel (600 / 0.5). The user found it too localised (a dent more than a stretch) and not smooth enough. So under the pointer the island goes the whole way, and its far end barely moves. The same field moves each item by where its center is, and a popover's content by its center. The outline is warped through `squircle::warp`, after the jelly (D-32), and the shape cache is rebuilt only while it moves, as for the jelly.
- **Limits:** `DRAG_LIMIT_X` stays 10 and `DRAG_LIMIT_Y` goes 5 → 6. These are now how far the part under the pointer goes, so the rest of the island moves less than before. *(First 18 and 10 on this branch; the user wanted it to move less.)* The canvas frame gets that much room on top of `NUDGE_BUMP_ROOM`, and the items' clip grows by the current stretch.
- **The grab point** is kept, relative to the shape, until the springs settle after letting go, so the spring back wobbles where it was grabbed.
- **Popovers:** a press on an open popover grabs it only if the panel's content didn't capture it. Presses on sliders, buttons, rows, fields and scrollbars don't. Dragging the volume slider shouldn't stretch the panel. The overlay follows the drag itself, because what it captures never reaches the island.
- **Surface:** a downward pull needs more room than the 6 px D-26 left. The surface is now `SURFACE_HEIGHT`, with `DRAG_ROOM` (14 px) below the islands. The exclusive zone and the input strip stay `BAR_HEIGHT`, with only `RESERVED_ROOM` (6 px) below, so windows don't move. The extra 8 px hang transparently over the top of the windows and let their clicks through: the input region (D-28) is now set when each bar surface opens, not only when a popover comes or goes. A held drag still gets motion outside it, through the pointer's implicit grab.

**Alternatives:**
- the old rigid translate, with bigger limits: the island just slides further. It doesn't stretch.
- pulling the jelly (D-32): its springs only move along the edge's normal, so a sideways drag in the middle of a wide island couldn't stretch it at all.
- any press on a popover grabs it, as on the bar: it fights every control in the panel.
- a taller exclusive zone: windows would start 8 px lower for an effect that lasts while a button is held.

**Measured** (Asahi, release, the user's config, ~16 s after start, alternating runs): binary 9,465,184 bytes either way. RSS ~17.1 MB here, 16.6–17.3 MB on `main`, within noise: the taller surface adds ~200 KB per buffer at scale 2. Idle CPU unchanged (6–7 ticks per 10 s). No new dependencies. *Gotcha (as in D-44):* the first run or two after a build sat at 33–44 MB on both, so compare several runs.

---

## D-46: Collections: several plugins per repository, one widget per plugin

**Status:** accepted (2026-10-13). Changes the manifest of [D-12](#d-12-third-party-plugins-compiled-in-by-eave-build) and what `#name` means in [D-30](#d-30-plugins-are-referenced-where-theyre-used-in-configtoml).

**What:**
- A plugin is one crate that registers one widget, named by its manifest. `eave-plugin.toml` sits beside the crate's `Cargo.toml` and holds `name`, `description` and `api`. `crate` and `path` are gone: the crate's name is read from its Cargo.toml. eave-build rejects old manifests that still have them, and says to remove them.
- A repository's root holds either one `eave-plugin.toml` (a single plugin) or an `eave-collection.toml` whose `[collection] plugins = [...]` lists plugin directories. Having both is an error, and so are two plugins with one name.
- `<source>[#name]` picks the plugin called `name`, or, without `#`, the one named like the repository (without `eave-`). This is checked for every reference. Before, `#name` was never checked and a bare reference was checked against the single manifest. A missing name fails the build and lists the ones that are there, as `<source>#<name>`. Each plugin of a collection is compiled in only if the config uses it.
- eave's own root has an `eave-collection.toml` listing `plugins/*`. A built-in is the plugin of that name in it, not the directory `plugins/<name>`. A test keeps the list in step with the directories.

**Alternatives:**
- a `widgets = [...]` list in the single manifest, for one crate registering several widgets: makes `#name` checkable, but all of them are compiled in to use one, and "the" plugin's `name` stays a confusing default.
- `[[plugin]]` tables in one file at the root: one file per repository, but a plugin's manifest no longer travels with its crate, so moving a crate between repositories means editing two files.
- finding plugins by globbing for `*/eave-plugin.toml`: nothing to keep in step, but a slow, surprising walk over a big repository, and no way to leave a crate out.

**Why:** one name means one crate means one widget. That makes every reference checkable at build time, and the errors can say what to write instead. Each crate's manifest sits beside its Cargo.toml, so the manifest doesn't need to say where the crate is.

**Cost:** existing manifests have to drop `crate` and `path`. Only eave's own plugins existed. A crate can no longer offer several widgets under one plugin: split it, or put the shared code in a library crate of the collection. Pinning `eave = "…@rev"` to a commit before this change no longer builds with the new eave-build, since that checkout has no collection. The bar itself is unchanged: `config::widget_name` already gave the name, and the bar still reports a widget a plugin didn't register.

---

## D-47: Notices: plugins list notifications, the right island turns into them

**Status:** accepted (2026-10-13). Narrowed by [D-52](#d-52-notifications-travel-only-over-the-bus): only the plugin serving the notification bus lists notices. Other plugins send their notifications over the bus. The text below is kept as it was decided.

**What:** a widget can list notifications to show on screen: `Widget::notices() -> Vec<Notice<Message>>`, newest first, next to `item()` and `popover()`. While there are any, the right island morphs into a panel of them, like a popover.
- **A notice is structured, not a node:** a `key`, a summary, and optionally the app's name and icon, a plain-text body, an `Urgency` (`Low`, `Normal`, `Critical`), actions (label and message), `on_press` (a click on the card), `on_dismiss` (its close button) and `expires(after, message)`. The host lays out each one (`kit::notice`): a thin bar across the top for the time it has left on screen (`countdown.rs`, only if it expires, standing still while its clock is stopped), app icon and name with a close button, the summary, the body cut off after four lines, and the actions, with the first one prominent. Critical names the app in the critical tone, and Low dims the summary.
- **Declarative, diffed by key:** after each of a widget's messages, the host compares its notices with the ones it keeps. A new key comes in on top, a changed one updates in place (its time starts over if its text changed), and a missing key leaves the panel, which springs to its new size.
- **The host owns time:** a notice's clock runs only while the panel is on screen and not pointed at. When it runs out, the widget gets the `expires` message, and dropping the notice is up to the widget. Pointing at the panel stops every clock, and leaving it gives each at least a second back. One timer is set, for the next notice to expire. Nothing ticks.
- **Where:** the right zone's island morphs into a panel of every notice, newest first, with a line between each and the next (`kit::notices`). It's the popover morph (D-28, D-44): placed like a popover, the island's items fade out, the panel's content fades in, and it squishes as it sets off. The panel springs to its new size as notices come and go. When the last one leaves, it folds back into the island, fading out the last ones it showed. The host keeps it as a `NoticePanel`, not a popover: nobody opens it, it doesn't take the keyboard, and a click elsewhere doesn't close it.
- **One thing at a time:** an open popover on the right island takes the island over. The island morphs straight from the notices to the popover, and the notices wait with their clocks stopped. When the popover closes, the island morphs straight back to the notices, without going back to itself first. For that, an island's popover has a key (`island::Popover::key`): when the key changes while the island is a popover, the shape springs on from where it is, and the new content fades in once it's halfway.
  - *Superseded, same branch:* each notice was a card of its own, growing out of the island's underside and stacked down from it, while the island stayed itself (`island::Toast`, an overlay next to the items'). The user wanted the island itself to morph, like a popover, with dividers between notices.
- **Which bar:** the focused monitor's. The bar only opens on the active output for now (`StartMode` default), so that's the first bar surface. With a bar per output it will follow Hyprland's `focusedmonv2`.

**Alternatives:**
- commands on `Effect` (`notify`, `withdraw`): the host would hold state the widget also holds, and the two could drift apart. Declaring them, like `popover()`, keeps one source of truth.
- a `Node` as the card's content: plugins would lay out notification cards each their own way, which breaks "plugins describe, the host draws".
- leaving notifications to an external daemon (mako, dunst, swaync): its popups would look nothing like eave and couldn't grow out of an island. With notices, eave works with any engine: eave's own (D-48), or a bridge plugin for another engine that can run without drawing.
- notices waiting below an open popover: both at once, for an island that would be doing two things.
- a card per notice, apart from the island: superseded above.

**Why:** it keeps eave engine-agnostic: whatever receives notifications, the host shows them, in eave's style, with eave's motion. And the plugin only says what a notification is.

**Cost:** no dependencies. Tokio's `time` feature in the host, already in the build through eave-api. With no notices, nothing runs: no timer, no panel. The countdown bar redraws the surface while it runs, a pixel at a time but at most 30 times a second, like the battery's gulp. *Measured* (Asahi, release, the user's config, 5 s with one notification): 3 ticks idle, 31 with a notification that doesn't expire (the morph in and out), 68–69 with one that does. At 60 fps it was 114, so 30 fps halves the countdown's cost. A slower bar would be cheaper still, but would move in visible steps. While notices show, the right island's items are under the panel, as under a popover: to reach them, wait for the notices to go, or dismiss them.

---

## D-48: eave-notify: eave's own notification server

**Status:** accepted (2026-10-13). Moved to its own repository, [eaveshell/eave-notifications](https://github.com/eaveshell/eave-notifications), with its plugin ([D-50](#d-50-notifications-in-their-own-repository-eave-api-and-the-services-on-cratesio)); its decisions continue there.

**What:** `crates/eave-notify` serves the freedesktop notification spec (`org.freedesktop.Notifications`, 1.2) on the session bus, inside the bar. `plugins/notifications` shows what it receives as notices (D-47), with a bell in the bar and the latest notifications in its popover.
- **The server only receives and reports.** `watch()` starts it on first use and yields whether it's serving, then every notification and every `CloseNotification`. `close(id, reason)` and `invoke(id, key)` send `NotificationClosed` and `ActionInvoked`. Ids count up from 1, and `replaces_id` keeps the id. It advertises `actions`, `body`, `body-markup` and `inline-reply` (D-49). What to show and for how long is the plugin's business.
- **Made plain on arrival:** body markup is stripped to text, `<br>` becomes a line break, and entities are decoded (`markup.rs`). A `<` that doesn't open a tag stays. The icon is an app id: the `desktop-entry` hint, or `app_icon` when it names an icon rather than a file. The host looks it up in the icon theme like every app icon. Images (`image-data`, `image-path`, icon files) are left out for now: the vocabulary has no picture primitive yet.
- **Timeouts:** `-1` is the plugin's default (`timeout`, 5 s), `0` is never, and critical notifications never expire, as the spec suggests.
- **Never pushes another daemon out.** It asks for the name with no flags. If mako, dunst or swaync has it, the bus puts eave in line, the bell is dimmed, and the popover names the daemon (its pid's `/proc/<pid>/comm`). When that daemon quits, the name passes to eave, and it says so. And since it doesn't allow replacement, a daemon started later can't take the name from it. The bus calls are made directly, not through zbus's `request_name`, which takes its flags as a type zbus doesn't export, and which would start watching for the name only after asking.
- **The plugin:** a notice per notification, with its actions as buttons. The default action is a click on it. A notification leaves the island after its time (and stays in the list), on its close button, or when an action is picked (which also closes it). The bell counts the notifications that arrived since the popover was last opened and that you didn't deal with yourself: one you closed, clicked or picked an action of doesn't count, and one that expired does. The popover has two tabs. New lists the ones still waiting for you. History logs every one that arrived, newest first, whatever became of it, without its actions, since the app was told it closed. Both keep the latest `history` (100), with how long ago each arrived. 100 because it's in memory only and starts empty with the bar: about a day's worth for most people, a few tens of KB at most, and still short enough to scroll. A click on one there shows all of its body and a button per action, the default one first (labelled `Open` if it has no label of its own), and a click again hides them: it doesn't act on it or take it away. The list's times are re-counted every 30 s, only while it's open. The popover also has a Clear button and a do not disturb toggle. Right click on the bell toggles it too. With do not disturb, notifications only go to the list, and the island gets a nudge instead. Critical ones still show.

**Alternatives:**
- an external daemon, with a plugin reading its state: mako, dunst and swaync each draw their own popups and each have their own control interface, so eave would show a count and nothing in its own style. D-47 keeps that door open: a plugin for another engine can feed the same panel, if that engine can run without drawing.
- a separate eave notification daemon process: one more process (each iced and layer-shell process costs ~10 MB RSS), and its popups couldn't grow out of the bar's islands.
- replacing the current owner (`ReplaceExisting`): eave would steal notifications from a daemon the user still runs on purpose.
- keeping history on disk: notifications sent while the bar restarts would still be lost, and storing what notifications say is a privacy question on its own.

**Why:** the bar owns the screen edge the notifications come out of, and their look. Serving the spec in-process costs one more D-Bus connection and no new dependencies.

**Measured** (Asahi, release, the user's config, ~18 s after start, several alternating runs): binary 9,465,184 → 10,186,160 bytes (+704 KiB). The host's half (D-47) adds nothing measurable. Of the rest, about 130 KB is this code, and the rest is zbus and zvariant code instantiated for serving an interface: the session connection, the object server, and decoding the hints map. RSS 17.0–17.1 MB before, 17.1–17.4 MB with the same config, and 17.7–17.9 MB with the notifications widget (its session bus connection). Idle CPU unchanged: 2–3 ticks per 10 s in every case. *Gotcha:* some runs of each build, including the old one without the widget, jumped to 24–47 MB and up to 294 ticks while the desktop was in use. No notification arrived during them (dbus-monitor), so compare several quiet runs.

**Cost:**
- the bar is now also the notification daemon. If it crashes or restarts, notifications sent meanwhile are lost, and the list starts empty after a restart.
- users switching from mako, dunst or swaync have to stop starting it.
- +704 KiB of binary when the widget is used. eave-build compiles it in only then.

---

## D-49: Answering a notification: a field, and the keyboard only once asked

**Status:** accepted (2026-10-13)

**What:** a notification can take a typed answer, like a reply to a message.
- **The spec has no such thing, so we follow KDE's extension**, which messengers (Telegram, KDE Connect and others) use. The server advertises `inline-reply`. An app that wants an answer adds an `inline-reply` action, and may say what the field shows while empty (`x-kde-reply-placeholder-text`) and what the send button says (`x-kde-reply-submit-button-text`). The answer goes back as a `NotificationReplied(id, text)` signal. `eave-notify` takes that action out of the actions (`Notification::reply`), and `reply(id, text)` sends the signal.
- **In the API:** `Notice::reply(Reply::new(placeholder, value, on_input).on_submit(..).on_cancel(..))`. The host draws a text field with a send button on the notice, in its tone, below the body. A `Reply` keeps `on_input` behind an `Rc`, because the host keeps notices it can clone.
- **The keyboard only once asked.** While a notice on screen has a field, the bar takes the keyboard for the notifications' surface, as for a popover (D-36), and the field takes the focus as it appears (D-41). Then no notice expires, and Esc sends `on_cancel`. So a notification with a reply never takes the focus by itself: the plugin shows a Reply button first, and the field only after a click on it. Sending takes the notification away, after telling the app it closed. An empty answer isn't sent.
- **In the popover's list too:** the expanded notification has the Reply button, then a field, which works like any popover's.

**Alternatives:**
- the field always there, and the bar taking the keyboard whenever a notification can be answered: every message would take the focus from what you're typing in.
- `KeyboardInteractivity::OnDemand` while a field shows, so the compositor gives the bar the keyboard on a click: with `follow_mouse`, the focus would leave the field as soon as the pointer left the bar (D-36).
- answering only in the popover: one more click, for the thing a message notification is most often for.

**Why:** answering a message from where it appears is the point of a notification that offers it, and asking first keeps the bar from ever taking the keyboard unasked.

**Cost:** no dependencies. While a field shows, the bar has the keyboard, as while a popover is open.

---

## D-50: Notifications in their own repository; eave-api and the services on crates.io

**Status:** accepted (2026-10-13). Moves [D-48](#d-48-eave-notify-eaves-own-notification-server)'s crates out of eave.

**What:**
- `eave-notify` and the `notifications` plugin live in [eaveshell/eave-notifications](https://github.com/eaveshell/eave-notifications), as a collection of their own. The config entry is `widget = "eaveshell/eave-notifications"`. eave keeps what any notification source needs: the notice API (D-47) and replies (D-49), the island's panel and the bell icon.
- `eave-api` and the service crates (`eave-dbus`, `eave-hyprland`, `eave-net`, `eave-pulse`, `eave-iwd`) are published on crates.io, so a plugin outside eave builds and tests on its own. `eave-build` still patches them to the checkout it builds against.
- The organisation is now `eaveshell` (it was `eavebar`): the bar is the first piece of a shell.

**Alternatives:**
- keeping notifications built in: eave's own engine would be a special case, where D-47 says an engine is just a plugin. It's also the only built-in that takes over a desktop-wide role, which not everyone wants from their bar.
- not publishing, with a `[patch]` in every plugin repository: each would need an eave checkout beside it to build.
- publishing the host (`eave`) and `eave-build` too: the host relies on the patched `softbuffer` in `vendor/`, which a published crate can't carry, and eave-build installs from git.

**Why:** the engine never depended on eave (D-48), and a separate repository proves the notice API works for a plugin eave doesn't ship.

**Cost:** a published version can only be yanked, not deleted, and `eave-api` now follows semver: a breaking change is 0.2. Changes that touch both the notice API and the plugin are two commits in two repositories.

---

## D-51: A popover keeps its room until its shape lands; tabs hand their pages over

**Status:** accepted (2026-10-13).

**What was wrong:** a popover or the notices panel that changed size while open snapped instead of morphing. You saw it when dismissing a notification or switching to a tab of another height.
- **Shrinking:** the island reported the new, smaller rect at once (`on_placed`). The host shrank the surface and the input region to it on the next commit. The shape's edges were still springing down, but the surface cut off everything below the new bottom.
- **Growing:** the new rect didn't fit the surface yet, so for the frame or two before the surface grew, the island treated the popover as having nowhere to go. It aimed back at the island and started fading the panel out, then turned around.

**What:**
- **The island reports the room it needs, not just where the panel goes** (`Motion::reach`). While the shape's edges are moving, that's the new place joined with what it last reported. Once they're at rest, it's the new place alone. So a grow is reported at once, and a shrink keeps its room until the shape has landed, then gives it back at once. The surface resizes twice per change, never every frame, which would reallocate its buffers on every frame. The report also runs after the island steps, so the frame the shape lands on sends it: the overlay sees each event before the island moves, and no event follows the last frame.
- **A growing panel holds its shape while the surface grows.** The island keeps the last place that fit, and an open popover that doesn't fit yet stays there instead of heading back to the island. A first open still waits at the island (D-28). This covers popovers and the notices panel (D-47), which both go through `island::Popover`.
- **Tabs hand their pages over** (`handover.rs`): the old page slides `TAB_SLIDE` (16 px) away from the new tab as it fades out, then the new one slides in from that side, over `TAB_FADE` (240 ms) in all. Switching back mid-way turns around from what's on screen.
  - This is the alternate's fade (D-33), taken out of `alternate.rs` into a `Handover` that both use. It fades out and then in, rather than crossing, for the same reason: fades cover with the island's color.
  - The tabs take the new page's height at once, so the shape eases to it while the old page goes. The old page is laid out too, until it's gone, and is clipped by the shape if it's taller.
  - The slide is clipped to what the shape shows (the viewport), because a nested layer isn't clipped by the one around it. Only the current page gets events and operations, as before (D-43).

**Alternatives:**
- **Resize the surface every frame to the shape:** a new buffer on every frame of every morph.
- **Keep the surface at its tallest size while a popover is open:** the input region would have to follow the shape, and a long list scrolled away would keep a tall surface for no reason.
- **Change the tabs' height at the handover's midpoint, as a growing alternate does:** the old page would never be clipped, but the shape would wait 120 ms before moving.

**Cost:** for the ~300 ms a shrink takes, the input region still covers the part the shape is leaving, so a click there goes to the popover. No new dependencies.

**Measured** (release, the user's config, 15 s after start, 10 s windows, alternating runs): binary 9,465,184 bytes either way. RSS ~16.8 MB on both, and 26–33 ticks per 10 s on both. The bar was measured beside the user's running one, so its ticks run higher than the baseline table. As in D-48, an occasional run of either build jumped to 30–45 MB and 150+ ticks while the desktop was in use.

---

## D-52: Notifications travel only over the bus

**Status:** accepted (2026-10-13). Narrows [D-47](#d-47-notices-plugins-list-notifications-the-right-island-turns-into-them).

**What:** there is one way to send a notification, the freedesktop one: `Notify` on `org.freedesktop.Notifications`, on the session bus. Plugins send theirs that way, like any app.
- **Whoever owns the name shows it.** With `eaveshell/eave-notifications` in config.toml, that's the bar. Otherwise it's mako, dunst, swaync or whatever else the user runs. With no daemon at all, the notification is lost, as any app's would be.
- **`notices()` is for the engine only.** It's how the plugin that serves the bus hands what it received to the host to draw (plugins describe, the host draws). No other plugin returns notices: a low battery warning goes over the bus, gets the daemon's history and do not disturb, and shows under any daemon.
- **eave's extras are vendor hints.** Anything the spec lacks travels in hints named `x-eave-<name>`, and the engine advertises each one it understands as an `x-eave-<name>` capability, as the spec says vendor extensions should be. Daemons that don't know a hint ignore it, so the notification still shows without the extra. KDE's inline reply (D-49) works the same way.

**Alternatives:**
- deleting `notices()`: eave-notifications would have no way to show what it receives in the island.
- having the host draw only the notices of the plugin that owns the bus name: the rule enforced in code, for an API with one user. The docs say it, and a plugin that ignores them only shows its own notices in its own bar.
- plugins choosing between `notices()` and the bus: two paths, where one skips the daemon's history and do not disturb, and only works in eave.

**Why:** one path. A plugin's notification behaves like every other app's, whichever daemon the user picked, and the bar shows it only because it's the daemon.

**Cost:** no code changes. A plugin that sends notifications opens a session bus connection (`zbus::Connection::session()`; `eave-dbus` re-exports zbus). If no daemon runs, its notifications are lost.

---

## D-53: eave on the AUR, shipping eave-build from release tags

**Status:** accepted (2026-10-13). Amends [D-31](#d-31-updates-are-explicit-installs-only-when-the-binary-changed).

**What:** an AUR package named `eave` ([dist/aur](../dist/aur/README.md)), built from the GitHub tarball of a `vX.Y.Z` tag. Releases start with `v0.1.0`.
- **It ships `eave-build` only**, in `/usr/bin`, plus the license and docs. The bar is still built per user into `~/.local/bin/eave` with the plugins config.toml uses (D-12, D-30). So `cargo`, `git`, `pkgconf` and `libxkbcommon` are runtime dependencies, not just build ones.
- **A packaged eave-build builds its own release.** The PKGBUILD sets `EAVE_PACKAGE` at build time. A config.toml without `eave = …` then builds `eaveshell/eave@v<version>` instead of whatever main was at the first clone. When the package is upgraded, the new pin isn't in the checkout yet, so the next build fetches it (D-31) and the bar moves with the package. An explicit `eave = …` still wins.
- **It never updates itself.** `eave-build update` runs `cargo install` on itself only when it isn't a packaged build and runs from cargo's bin directory (`$CARGO_INSTALL_ROOT/bin`, otherwise `$CARGO_HOME/bin`). Otherwise it says the package manager updates it. It still runs `init` to rewrite the units.
- **The units stay in `~/.config/systemd/user`**, written by `eave-build init` as before. Their `ExecStart` is eave-build's own path, `/usr/bin/eave-build` here. The package's install script prints the three commands to run as the user, and a reminder to run `eave-build init` after an upgrade.
- CI builds the package from every commit in an Arch container and runs namcap ([.github/workflows/ci.yml](../.github/workflows/ci.yml)).

**Alternatives:**
- **`eave-git`, built from main:** no releases to cut, but `eave-build update` and the package would both move eave, and every package build would be a different commit.
- **Ship the bar in the package:** it would be a bar with a fixed set of plugins. config.toml naming any other plugin would need a user build anyway.
- **Ship the units in `/usr/lib/systemd/user`:** a pacman upgrade would refresh them by itself. But `PathChanged=` has to follow `XDG_CONFIG_HOME`, which only `init` knows, and a user copy in `~/.config` would shadow the packaged one for everyone who used install.sh before. One place, written by one program.
- **Leave the default eave unpinned under a package:** the package would build eave from main while being called 0.1.0, and could build an eave newer than its own eave-build understands.
- **`depends=(rust)`:** rustup users would have to swap toolchains. Both `rust` and `rustup` provide `cargo`.

**Why:** `paru -S eave` is how Arch users expect to install a bar, and pacman should own what it installed. Pinning eave to the package's release makes the version you see in pacman the version you run.

**Cost:** someone has to cut releases: tag, `updpkgsums`, `.SRCINFO`, push to the AUR. A user who installs `rust` only for eave gets a toolchain of roughly a gigabyte. Plugins are still not updated by pacman; `eave-build update` does that. eave-build's binary is unchanged in size, since the packaged default is one more string.
