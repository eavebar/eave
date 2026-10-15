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

**Status:** superseded by [D-67](#d-67-the-gpu-renders-by-default-renderer--cpu-in-configtoml) (2026-10-13). Accepted 2026-10-12. Its "revisit if" came true: a large popover dropped frames on the CPU.

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

**Update (2026-10-13):** `web-colors` came back with the `gpu` feature. Dropping it made the GPU blend in linear light, so its grays didn't match tiny-skia's ([D-68](#d-68-the-gpu-blends-in-srgb-like-tiny-skia-web-colors)). The rest of this decision stands.

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

**Symlinks** (2026-10-14): dotfiles managers (perch, stow, yadm…) make `config.toml` a symlink into their repository, and a write there never shows in the link's directory, so the bar didn't reload. The watcher also watches the directory the link resolves into, for the file's name there, and follows again after every change, so a re-pointed link is picked up too. Only the last hop of a chain is watched. Whatever writes config.toml (`eave calibrate`, `eave-build init`) writes through the link to its target and never replaces the link. No new dependency.

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

**What:** the bar no longer paints a full-width strip. Each zone (left, center, right) is an island: a rounded container (`ISLAND_RADIUS`, `ISLAND_BG`, the popover's hairline border, *dropped in [D-74](#d-74-islands-have-no-border)*) as wide as its items. A zone with no visible items has no island. Each zone still takes a third of the width, so the center island stays centered on screen. The islands sit `EDGE_GAP` (6 px, the same as `PANEL_GAP`) from the top, left and right edges. All of it is drawn on the one transparent layer surface. The gap is padding inside the surface, which is `ISLAND_HEIGHT + EDGE_GAP` tall, and that's also the exclusive zone. *(Superseded in part by [D-26](#d-26-squircle-islands-that-squeeze-and-spring-back): the surface and exclusive zone also include `DRAG_ROOM` below the islands, and islands are squircles instead of rounded rectangles.)*

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
- **Release:** a damped spring (`SPRING_STIFFNESS`, damping ratio `SPRING_DAMPING` 0.5) brings offset and squeeze back, with a small overshoot. *(Superseded by [D-83](#d-83-one-motion-language): the press and the drag share `HAND`.)*
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
- Git sources are cloned once into `$XDG_DATA_HOME/eave/src` and then left alone. Without a `@rev` a checkout stays at the commit it has. A `@rev` that isn't there yet (a pin just edited) is fetched. Only `eave-build update` fetches everything, moves each source to the newest commit of its `@rev` or default branch, and prints `old → new (N new commits)`. When eave itself moved, it also runs `cargo install --locked --path <eave>/crates/eave-build`, after the bar is in place. *Since [D-54](#d-54-eave-on-the-aur-shipping-eave-build-from-release-tags):* only when eave-build runs from cargo's bin directory and isn't a packaged build. A package manager updates it otherwise. *Superseded in part by [D-93](#d-93-update-reinstalls-eave-build-before-the-bar-not-after):* it reinstalls eave-build before the bar, and the new one builds it.
- `eave-build` (which replaces `--install` and `--restart`) always builds, then compares the result with `~/.local/bin/eave` byte for byte. It installs and restarts the bar only when they differ.
- `eave-build init` writes a starter config.toml (`dist/config.toml`, kept if one exists) and the three systemd units. `install.sh` is a thin POSIX script around it: check for `cargo` and `git`, `cargo install --git … eave-build`, `init`, build, enable the units. Re-running it never updates.
- The rebuild unit runs eave-build by its full path, which `init` writes in (2026-10-12). *Superseded:* `ExecStart=eave-build` with `Environment=PATH=%h/.cargo/bin:…`. systemd 262 resolves a bare name only in its compiled-in search path, never through the unit's `PATH`, so every rebuild failed with status 203/EXEC. Since the units can change, `eave-build update` runs the new eave-build's `init` after reinstalling it, then `systemctl --user daemon-reload`.

**Alternatives:**
- fetch on every build (as before): saving config.toml could upgrade eave and every plugin behind your back, through the path unit.
- a lockfile recording each source's commit: reproducible across machines, but one more file. The checkouts already are the lock on one machine, and pins cover the rest.
- compare a hash of the generated crate instead of the binary: misses changes in local-path sources, which is how plugins are developed.
- a distribution package (AUR): later. It doesn't fit compiled-in plugins without a toolchain anyway. *Superseded by [D-54](#d-54-eave-on-the-aur-shipping-eave-build-from-release-tags):* the package ships eave-build and keeps the toolchain as a runtime dependency.

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
- **`Left` / `Right`:** the whole island travels about 5 px that way and comes back. *Later* (2026-10-13): the item that nudged also gets a push of its own, `NUDGE_KICK_SPEED`, about 2.5 px further than its island, on its own spring (`ItemMotion::kick`, at the nudge's pace). So what changed, like the next track's cover or the active workspace, moves a little more than the rest. It squishes as it goes, `NUDGE_KICK_SQUISH` (3 %) narrower and half that taller per pixel out, about 7 % narrower at the furthest, as if it bumped into where it's going (`Motion::kicked`, through the same per-item squish as the jelly).
- **The outline is a jelly** (2026-10-13, `jelly.rs`). It is a ring of springs all the way round the island's edge: 24 across the top and bottom, 6 down each side, each one how far out (or in) its piece of the edge is. Each is pulled back to 0 (`JELLY_STIFFNESS` 600, `JELLY_DAMPING` 0.85, the NUDGE feel) and toward its neighbors (`JELLY_COUPLING` 1400). The pull toward the neighbors does the smoothing. A nudge just pokes the ring with velocity kicks, and the ring spreads the poke along the edge and round the corners into a soft curve, then settles flat. Nothing that pokes it decides what curve it makes. The ring is drawn by warping the squircle through it: each side moves the points near it, eased out over half the shape's short way, and the ring is sampled smoothly between springs (Catmull-Rom).
  - **Up / down:** the edge over the item is kicked in a raised cosine `NUDGE_BUMP_WIDTH` wide (`JELLY_BUMP`, peaks about 2 px). `NUDGE_BUMP_WIDTH` went 64 → 112 → 176 px (2026-10-13): the user wanted the bump smoother and larger, then wider still. Wider kicks still peak about 2.1 px (2.05 before), since kicks bring springs up to their speed rather than add, so it's a gentler slope, not a taller one. Both sides are kicked to lean the same way (`JELLY_LEAN`), so the edge it's going toward widens, about 1.3 px each side.
  - **Left / right:** the top and bottom are kicked to lean (`JELLY_LEAN`, a sine from the end it's leaving to the end it's going toward). The leading end swells about 1.6 px each way, so it's about 3 px taller, and the trailing end flattens as much.
  - Items read the ring at their center (`Jelly::at`). They move by how far its middle moved there, so an item under a 2 px bump rises 1 px, as `NUDGE_ITEM_FOLLOW` used to do by hand. They squish by how much taller the shape is there (D-40's per-leaf squish).
  - Kicks in a row don't pile up. Each brings a spring up to its speed rather than adding to it (as the old bump did with `max`), and less of it the further out the spring already is. *Superseded:* adding kicks up. Holding the volume key ran the bump into the end of its room, where the hard limit flattened it frame to frame, and it flickered. Now a nudge every three frames levels off around 2.8 px.
  - The ring stays inside the frame's `NUDGE_BUMP_ROOM`, which now goes round the sides too. While it moves, the shape cache is rebuilt every frame through a flattened, warped path, as the bump already was. At rest it's all zeros and draws the cached squircle.
  - *Superseded:* the bump was its own spring with a fixed raised-cosine shape (`squircle::Bump`), and the item followed it by `NUDGE_ITEM_FOLLOW`. A first lean, the same day, was a linear taper of the whole path read straight off the push springs' displacement (`squircle::Taper`). It had no motion of its own, so it snapped in with the kick, and the linear keystone sheared the corners. The user found it not smooth and asked for one mechanism that smooths out any change to the shape by itself. Both special cases became pokes on the jelly.
  - Measured (Asahi, release, the user's config): binary 8,350,784 bytes before and after (the jelly is a few hundred bytes of code; no dependencies). RSS 16.3 MB 10 s after start (15.9 before). Idle CPU 0–1 ticks a second when nothing moved, the same as `main`'s bar running beside it, though not a clean 20 s run: the bar was being tried at the time. While it wobbles, each frame flattens and warps the path, as the bump already did.

*(The springs, speeds and `NUDGE_LASTING` below are superseded by [D-83](#d-83-one-motion-language): every nudge rides `REACT`, the jelly's stiffness and damping are `REACT`'s, each kick is given as how far it goes, and a paced nudge divides by `REACT`'s settle time.)*

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
- Plugins passing image bytes: that breaks D-13, and every plugin would repeat the lookup. (A plugin's own icon file, compiled in, is allowed since [D-53](#d-53-plugins-ship-icon-files-uiimage-and-uisymbol). That's not a lookup.)
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
- Two springs, `stretch_x` and `stretch_y`, in pixels added to the width and height. They rest at 0 and get one velocity kick each way of the morph, like a nudge (D-32). Opening kicks `stretch_x` down as soon as it has room (`SQUISH_OPEN_SQUEEZE`, 2 × the panel's width per second: about 5 % narrower at the peak, ~70 ms in). Closing kicks flatter (`stretch_y` down) and wider (`stretch_x` up). Their feel (`SQUISH_STIFFNESS` 250, `SQUISH_DAMPING` 0.75) is just under critical, so it squashes and eases back with a rebound of about 3 %, too little to see. *(Superseded by [D-83](#d-83-one-motion-language): they ride `REACT`, quicker than this, and the kicks are given as how far they go: `SQUISH_OPEN` 5.5 % of the width, `SPLAT_FLATTEN` 1.4 px, `SPLAT_WIDEN` 3.5 px, the sizes these speeds had. `SQUISH_BULGE` is `BULGE`, shared with the item kick.)*
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
- **The pull:** the drag amount is still the rubber-banded `x`/`y` springs, now on a softer feel of their own (`DRAG_STIFFNESS` 220, `DRAG_DAMPING` 0.7), so the stretch eases after the pointer and back with only a small overshoot. *(Superseded by [D-83](#d-83-one-motion-language): the drag and the press share `HAND`.)* Only `DRAG_FOLLOW` (0.5) of it moves the whole island. The rest moves the outline near the grab point, all of it at the grab and less further away, down to nothing at `DRAG_REACH` (200 px). The falloff is a smootherstep, flat at both ends, so the island bends in one long curve with no visible bend where it was grabbed or where the stretch stops.
  - *Superseded, same branch:* `DRAG_FOLLOW` 0.35, `DRAG_REACH` 60 px with a smoothstep, on the press's springy `PLAYFUL` feel (600 / 0.5). The user found it too localised (a dent more than a stretch) and not smooth enough. So under the pointer the island goes the whole way, and its far end barely moves. The same field moves each item by where its center is, and a popover's content by its center. The outline is warped through `squircle::warp`, after the jelly (D-32), and the shape cache is rebuilt only while it moves, as for the jelly.
- **Limits:** `DRAG_LIMIT_X` stays 10 and `DRAG_LIMIT_Y` goes 5 → 6. These are now how far the part under the pointer goes, so the rest of the island moves less than before. *(First 18 and 10 on this branch; the user wanted it to move less.)* The canvas frame gets that much room on top of `NUDGE_BUMP_ROOM`, and the items' clip grows by the current stretch.
- **The grab point** is kept, relative to the shape, until the springs settle after letting go, so the spring back wobbles where it was grabbed.
- **Popovers:** a press on an open popover grabs it only if the panel's content didn't capture it. Presses on sliders, buttons, rows, fields and scrollbars don't. Dragging the volume slider shouldn't stretch the panel. The overlay follows the drag itself, because what it captures never reaches the island.
- **Surface:** a downward pull needs more room than the 6 px D-26 left. The surface is now `SURFACE_HEIGHT`, with `DRAG_ROOM` (14 px) below the islands. The exclusive zone and the input strip stay `BAR_HEIGHT`, with only `RESERVED_ROOM` (6 px) below, so windows don't move. *(They follow the islands' height since [D-76](#d-76-islands-are-as-tall-as-the-notch).)* The extra 8 px hang transparently over the top of the windows and let their clicks through: the input region (D-28) is now set when each bar surface opens, not only when a popover comes or goes. A held drag still gets motion outside it, through the pointer's implicit grab.

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

**Status:** accepted (2026-10-13). Narrowed by [D-52](#d-52-notifications-travel-only-over-the-bus): only the plugin serving the notification bus lists notices. Other plugins send their notifications over the bus. The card's layout is amended by [D-85](#d-85-notification-cards-fixed-slots-pictures-choices-stacks): the time and close button share a corner, pictures can be wide or pixels, cards of a thread stack, and at most three show. The text below is kept as it was decided.

**What:** a widget can list notifications to show on screen: `Widget::notices() -> Vec<Notice<Message>>`, newest first, next to `item()` and `popover()`. While there are any, the right island morphs into a panel of them, like a popover.
- **A notice is structured, not a node:** a `key`, a summary, and optionally the app's name and icon, a plain-text body, an `Urgency` (`Low`, `Normal`, `Critical`), actions (label and message), `on_press` (a click on the card), `on_dismiss` (its close button) and `expires(after, message)`. The host lays out each one (`kit::notice`): a thin bar across the top for the time it has left on screen (`countdown.rs`, only if it expires, standing still while its clock is stopped), app icon and name with a close button, the summary, the body cut off after four lines, and the actions, with the first one prominent. Critical names the app in the critical tone, and Low dims the summary. *Since 2026-10-13:* a notification's own picture (`Notice::image`) sits 40 px across beside the summary and body instead of in the app icon's place, the body is laid out in full and fades out at its fourth line rather than being squeezed into four (a cut-off Markdown list left a blank hole), and the parts are spaced apart: 8 px under the app's name, 2 between summary and body, 12 above the reply field and actions.
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
- **Made plain on arrival:** body markup is stripped to text, `<br>` becomes a line break, and entities are decoded (`markup.rs`). A `<` that doesn't open a tag stays. The icon is an app id: the `desktop-entry` hint, or `app_icon` when it names an icon rather than a file. The host looks it up in the icon theme like every app icon. Images (`image-data`, `image-path`, icon files) were left out at first, since the vocabulary had no picture primitive. *Superseded:* `image-path` and `app_icon` paths are now read and shown with `Notice::image` (eave-notify 0.2, [D-62](#d-62-reading-and-writing-in-popovers-markdown-a-text-area-scrolling-wide-panels)); raw `image-data` still isn't.
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

**Status:** accepted (2026-10-13). Partly superseded by [D-62](#d-62-reading-and-writing-in-popovers-markdown-a-text-area-scrolling-wide-panels): the field now shows with the notice, and the keyboard is taken on a click on it rather than on a Reply button first. The rest stands.

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

**Addendum (2026-10-13): landed means landed where it should be.** A shrink that a plugin caused by swapping its page still snapped. The Wi-Fi password page and going back from a Claude chat to its list both did it, while switching tabs didn't. A plugin's message rebuilds the UI, and the next event lays the panel out at its new size and reaches the overlay's `report` before the island's `update` has aimed the edges anywhere. The edges sat at rest on their old targets, so `reach` took them as landed and gave the room back at once. Now the edges count as landed only once they're at rest *on* where the shape should go: the panel, the last place that fit, or the island when closing. Tab switches were fine because their relayout happens inside the overlay's own update, after which the island aims before reporting. The tests had aimed right after placing, which hid this. One test now places without aiming.

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

## D-53: Plugins ship icon files: `ui::image` and `ui::symbol`

**Status:** accepted (2026-10-13). Narrows D-38's rejection of plugins passing image bytes.

**What:** a plugin can show an SVG or PNG file it ships, compiled in with `include_bytes!`. It does this when the vocabulary has no symbol for it and the user's icon theme can't be counted on, e.g. a product's logo.
- `ui::image(file)` draws the file in its own colors. As with `app_icon`, a tone only dims it.
- `ui::symbol(file)` keeps only the file's shape and draws it in the text's tone, like an `icon`.
- **The plugin still only describes.** The file is `&'static [u8]`, with no size or color: the host draws it at `ICON_SIZE`, and its alpha, tone and squish are the host's. It's still not a canvas (D-13).
- **The same pipeline as app icons** (D-38). A PNG is told apart by its signature, and anything else is read as an SVG. Each file is drawn once, on the blocking pool, into a 32 px bitmap, and kept by its address. The address is stable because the bytes are `'static`. Until the file is drawn, its room is kept empty, so nothing moves.
- **A symbol keeps its coverage, one byte per pixel.** It's drawn in a color the first time that color is asked for, and the six latest colors are kept, so a handle stays the same and the renderer uploads it once.
- An icon-only button holding one is drawn as a square icon button, like one holding an `icon`.
- The icon theme is looked up again on a config reload, but files stay, since they're compiled in.

**Alternatives:**
- **A path to a file installed beside the binary:** the bar is one binary (D-12), and a missing file would be a new way to fail.
- **Asking users to install the icon in their theme** (what eave-claude's README said): most never do, and the bar showed a C.
- **iced's `svg` feature:** rejected in D-38 for its size and for drawing on the UI thread.
- **Bytes taken at runtime (`Vec<u8>`), e.g. notification images:** the host would have to hash or compare them on every view to know them again. That's for later, with its own decision, if notification images come. *Later:* runtime pictures came as `ui::cover`, keyed by URL instead of by bytes ([D-57](#d-57-runtime-pictures-uicover-loaded-by-the-host-from-files-or-https)).

**Measured** (Asahi, release, two clocks, 8 s after start then 10 s windows, three alternating runs each): binary 9,465,184 → 9,530,720 bytes (+64 KiB). RSS 14.6–14.7 MB with it, 14.8 MB without, and 1–2 ticks per 10 s on both. No new dependencies: resvg, tiny-skia's PNG decoding and iced's `image-without-codecs` were already in for D-38.

**Cost:** each file a plugin shows is about 4 KB in memory (its bitmap), and a symbol up to 6 × 4 KB more for its colors. Plugins compiled with a file grow by its size.

---

## D-54: eave on the AUR, shipping eave-build from release tags

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

---

## D-55: Several islands per zone, collapsible items and dividers, set in config.toml

**Status:** accepted (2026-10-13). Extends [D-24](#d-24-floating-islands-on-one-transparent-surface) and [D-34](#d-34-width-groups-set-in-configtoml).

**What:** three layout options. Like `width_group`, the host reads them and never passes them to a widget.
- **`island = true`**, an entry with no widget, starts another island in the same zone, beside the one before. A zone's islands sit in a row, `ISLAND_SPACING` apart. A marker before any item, or right after another one, changes nothing, so it never makes an empty island.
- **`divider = true`**, an entry with no widget, puts a thin line before the next item (`kit::divider`). It shows only between two items that show: next to an item that's hidden (collapsed, or with nothing to show) or at an island's edge, it goes too. A divider with no item after it is warned about and dropped.
- **`collapsible = true`** on an item hides it while its island is collapsed. Every island starts collapsed. An island with collapsible items gets a chevron on the side facing the middle of the screen (a left island's right, a right island's left, a center island's right). It's there while the pointer is over the island (hovering grows the island to make room, and the chevron fades in), and stays while the island is expanded, so the items can be put back. Pressing it shows the collapsible items in their places, and pressing it again hides them. It points the way they come out while they're hidden, and back once they show. An island whose items are all collapsed stays as a small empty island, so the chevron can still be reached. Expanded islands are remembered across reloads while they keep collapsible items, but not across restarts.
- **Notifications** go to the right zone's last island, the one at the screen's edge. A popover on another right island doesn't hold them.
- **Zones push each other** (`zones.rs`). The zones used to be equal thirds of the bar, so an island that grew past its third (a left island expanding a long window title) slid under the center one. Now the left zone sits against the left edge, the right zone against the right, and the center zone is centered on screen unless a side reaches into it. Then it's pushed aside, `ISLAND_SPACING` clear. Islands spring to wherever layout puts them (D-27), so the pushed one glides over as the other grows. Within a zone, islands are a row and push each other the same way. Left and right zones that overlap each other are not handled.
- **An open popover makes room for itself.** The panel is `PANEL_WIDTH` wide (D-40), wider than most islands, and it used to grow over the islands beside it in its zone. While its popover is open, an island takes `max(its width, PANEL_WIDTH)` in the row. The island itself stays at the zone's side of that room (centered in the center zone), where the panel grows from, so its neighbors are pushed away and glide back once it starts closing. The notifications panel is a popover too, so it pushes the same way. `Island` keeps its own rectangle (`own_bounds`) apart from its layout's for the morph, presses, hover and placing the panel.
- **Expanding and collapsing squash the island** like a workspace switch (D-32): the island nudges itself sideways, toward the chevron when the items come out and back when they go, slowed to last `COLLAPSE_SQUASH` (450 ms). The island sees the change in `Toggle::expanded` itself, so the host sends nothing extra.
- **A pushed island piles up** (`Motion::lean_with_push`). While an island slides as a whole (both edges moving the same way, so a resize or a popover's morph doesn't count), its jelly's top and bottom edges are kicked out at the end it's pushed from every frame. The faster it goes, the harder (`PUSH_LEAN` at `PUSH_FULL_SPEED`, working up to it over `PUSH_BUILD`), so that end grows taller, about 1 px at most, as if what it passed piled up there, and the other end tucks in. When it stops, the swell is kicked across to the other end (`PUSH_WIGGLE`), and the jelly wiggles back flat by itself. It reuses the nudge's jelly, so the room it needs is the room nudges already have (`NUDGE_BUMP_ROOM`).

**How collapsing moves:** collapsed items are simply left out of the island's items, so the island's existing morph (D-27) animates it. The edges spring, the remaining items slide, and items coming back fade in. No new motion code.

**How the chevron knows about hover:** the island tracks the pointer over its own bounds, and on a change it asks for a new layout and a frame. The app gets no message, so hovering the bar still sends the app nothing (D-35). The chevron is a host `Slot` around `Icon::Back`/`Icon::Forward` that sends `ToggleCollapse(island)`. It's always laid out, so iced's operations and overlays find a real node. While hidden it takes no room, gets no events and isn't drawn.

**Alternatives:**
- **Named groups (`island = "audio"` on each widget):** robust to reordering, but islands would be ordered by where a name first appears, a second ordering rule beside the file's. With markers, order in the file is order on screen, like everything else.
- **`widget = "divider"`:** reads well, but `widget` names a plugin to compile in, and a divider is layout, which plugins don't do (D-13).
- **Hiding collapsed items with a fade-out:** the island drops an item as soon as it's gone from the list, so a fade-out would need the host to keep views of hidden items around. The slide of the rest and the springing edge already carry the motion.
- **A chevron always on screen:** clearer to discover, but it would cost room on every island with collapsible items all the time.

**Cost:** an island with collapsible items handles pointer motion over it (a bounds check per event) and relayouts on enter and leave. Islands without any collapsible item run the same per-event code as before. Binary size unchanged (9,530,720 bytes, aarch64 release, before and after).

---

## D-56: Media over MPRIS, in `eave-mpris` on the session bus

**Status:** accepted (2026-10-13).

**What:** `plugins/media` (`widget = "media"`) shows what any MPRIS player is playing. It is built on a new service crate, `crates/eave-mpris`. `eave-dbus` gains `session()`, a shared session bus connection like `system()`.
- **Watching:** one stream follows players coming and going (`NameOwnerChanged` for `org.mpris.MediaPlayer2.*`) and every signal from their object `/org/mpris/MediaPlayer2`: property changes and `Seeked`. A burst is drained, then after 50 ms every player is read again with `GetAll`, and an unchanged list isn't yielded. `eave-dbus::watch` doesn't fit, because it watches one fixed service, and players have names of their own.
- **Position:** players don't signal it as it advances. It's read with everything else, and `position_at(now)` works it out from the time it was read and the rate. The seek bar moves on a 1 s timer, but only while the popover is open and something plays.
- **`playerctld` is left out.** It mirrors the last active player, so that player would show twice.
- **Which player the bar follows:**
  1. One picked in the popover, until another one starts playing.
  2. Otherwise the one followed so far, if it's still playing.
  3. Otherwise the first one playing.
  4. Otherwise the one followed so far, then the first one.
  - Starting to play is the change of state from not playing to playing, so a player that has been playing the whole time doesn't take over from one picked by hand.
- **The item:** the track's cover ([D-57](#d-57-runtime-pictures-uicover-loaded-by-the-host-from-files-or-https)), then "Title · Artist", dimmed while paused.
  - `show = "hover"` shows the text only while hovered, and `show = "icon"` never shows it.
  - Right click plays or pauses, and the status changes at once, before the player confirms it. Scrolling skips tracks, and a skip nudges `Right` or `Left` when the player reports the new track, not when the request is sent.
  - The popover has a large cover, the title, artist and album, a seek bar, previous, play/pause and next, and a list of players when there are several.
  - *Later:* every track change on the followed player nudges, not only skips from the bar. MPRIS doesn't say which way a change went, so the plugin keeps the tracks the player went through (`Tracks`, by title alone: Chrome gives every track the same id, and sends a new track's title a moment before its artist). Going to the one before is `Left`, and going to the one after, or a new one, is `Right`, like a browser's history. That holds for media keys and the player's own buttons too. A skip back from the bar to a track it hasn't seen yet still goes `Left` (`SKIP_LANDS`, 3 s, since a skip back that only restarts the track brings none). A gap between tracks has no title and doesn't count. *Superseded:* every change the bar didn't ask for went `Right`, so going back with a media key nudged the wrong way.
  - *Later:* players go stopped with no track for a moment between songs, and the item used to vanish for that moment. Now the last track stays on screen for `GAP` (1 s) after the player has nothing to show, and the next track takes over from it. A player that really stops hides a second later.
  - *Later* ([D-59](#d-59-uicenter-and-a-centered-media-popover)): the popover is centered. The cover, title and "artist · album" sit in the middle, cut rather than wrapped, and play/pause is a filled circle between previous and next.
- **New `Icon`s:** `Media { playing }` (three level bars), `Play`, `Pause`, `Previous`, `Next`.

**The bars don't dance.** At first `Icon::Media` danced while playing, frame by frame like the charging battery (D-37). Measured on the full bar (workspaces, media, volume, network, battery, clock) with a player playing, in ticks per 10 s (100 is one core):

| Build | Ticks per 10 s |
|---|---|
| paused, or `main` | ~7 |
| dancing, 30 fps | ~113 |
| dancing, 15 fps | ~60 |
| dancing, 10 fps | ~45 |
| the icon alone on an empty bar, 30 fps | ~34 |

The host redraws the whole surface every frame, so the cost grows with everything else on the bar, not with the icon. (Overstated: only what changed is rasterized, but the whole tree is drawn and, until D-66, the whole surface was handed to the compositor. See [D-66](#d-66-a-patched-iced_tiny_skia-and-less-work-per-message).) It would also last as long as music plays, on battery power too. The user chose the cover in the bar instead, and the bars are now still: uneven while playing, low and level while paused. They show only when a player has no cover and doesn't name its app.

**Alternatives:**
- **Dancing in bursts** (a few seconds when a track starts, then still): it was offered, but the user preferred cover art.
- **Talking to `playerctld` only:** it would be one more daemon to have installed ([D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel)), and the popover couldn't list players.
- **Polling `Position`:** it would be a timer for as long as anything plays, and the seek bar only needs it while it's on screen.

**Measured** (Asahi, release; 8 s after start, then a 10 s window; config: workspaces, volume, network, battery, clock, plus media where noted):

| Build | RSS | Idle CPU |
|---|---|---|
| `main` | 16.8–16.9 MB | 3–4 ticks |
| this branch, without media | 17.7 MB | 3 ticks |
| this branch, with media, no player | 18.1 MB | 3 ticks |
| this branch, a paused player with a cover from a file | 19.0–19.1 MB | 4–5 ticks |
| this branch, a playing player with an https cover | 18.7–18.9 MB | 3–4 ticks |

Run side by side, `main` and this branch read the same CPU through spikes caused by other activity on the machine.

Binary: 9,530,720 bytes on `main`. The plugin and `eave-mpris` add 459 KB, making 9,989,528. Covers add the rest ([D-57](#d-57-runtime-pictures-uicover-loaded-by-the-host-from-files-or-https)).

---

## D-57: Runtime pictures: `ui::cover`, loaded by the host from files or https

**Status:** accepted (2026-10-13). Adds what D-53 left for later: pictures that come at runtime. Shipped files stay `ui::image`.

**What:** `ui::cover(url, fallback)` is a picture a program hands over by URL, like a track's cover art from `mpris:artUrl`. `.large()` makes it a popover's main picture, `COVER_SIZE` (128 px) square. Otherwise it's drawn at icon size.
- The plugin still only describes. It gives a URL and what to show in its place. The host fetches, decodes and draws, and picks the size and the corner radius.
- **Keyed by URL.** Comparing a string on every view is cheap, which is what made runtime bytes a problem in D-53: the host would have to hash or compare the bytes themselves.
- **Loading** runs on tokio's blocking pool, once per URL:
  - `file://` URLs are read, percent-decoded.
  - `https://` URLs are downloaded with `ureq` (rustls on ring), checked against the system's CA certificates (`rustls-platform-verifier`), with a 10 s timeout. A fresh agent is made for each download, because a kept one held rustls and the certificates in memory: 22.5 MB RSS after one cover, against 18.8 MB with a fresh agent.
  - Anything else (`http://`, `data:`) isn't loaded.
  - Files and downloads are capped at 8 MB, and pictures at 4096 px a side.
- **Decoding:** PNG through tiny-skia, which was already in the build for icons. JPEG through `zune-jpeg` (pure Rust, no default features), the format most covers come in.
- **Drawing:** the middle square is cropped (a video's 16:9 thumbnail shows its middle), shrunk by averaging, and given rounded corners. This is done twice, at 32 px for the icon and 256 px for the popover, and only those bitmaps are kept (~4 KB and ~256 KB). Smaller pictures aren't enlarged when drawn; the renderer scales them.
- **Fallback:** it shows until the cover is there, and in its place if the cover can't be had. Media uses the player's app icon (Spotify, Firefox, mpv), so a missing or slow cover never leaves a hole. In the popover the fallback is `empty()`: the popover springs to the cover's size when it arrives. *Later:* between tracks, media keeps the last cover instead of its app icon (`Art`). Chrome sends a new track before its cover, and the icon flashed in between. The fallback of a new cover is now the covers before it, nested (`cover(new, cover(older, cover(oldest, icon)))`, the last `COVERS_TRIED` = 3), which the host still has, since a fallback's cover is wanted too. The first one there shows. One isn't enough: Chrome writes each cover to a temporary file and deletes it as soon as it writes the next, two or three times a track within ~40 ms, so the one just before is often a file that was gone before it could be read. A track sent with no cover keeps the last one for `GAP` (1 s), then shows the icon.
- **Cache:** past 4 covers, the ones not on screen are dropped.

**Alternatives:**
- **Local files only:** Spotify and some web players give https URLs, and the user wanted those covers too.
- **The plugin reading the bytes and passing them in** (`Vec<u8>` or a handle): this would put I/O and decoding in every plugin, and the host would still need a stable key.
- **The `image` crate's JPEG feature:** it brings the same `zune-jpeg` through a larger API.
- **WebP:** not seen from the players tried so far. It can be added if a player needs it.
- **`webpki-roots` (bundled certificates):** a copy of the CA list to keep up to date inside the binary, where the system's own list already is.

**Measured** (Asahi, release):

| Build | Binary (bytes) |
|---|---|
| media, no covers | 9,989,528 |
| + covers from files: `zune-jpeg`, cropping and rounding | 10,186,136 (+197 KB) |
| + covers over https: `ureq`, `rustls`, `ring`, the platform verifier | 11,693,568 (+1.51 MB) |

RSS and CPU are in D-56: a cover costs about 1 MB of RSS whether it came from a file or https, and nothing in idle CPU.

**Cost:**
- This is the first time the bar reaches the network. The URLs come from programs on the user's own session bus.
- https is most of the binary growth (+1.5 MB, 16%). It's worth revisiting if a lighter TLS stack appears, or if Spotify starts handing over local files.

---

## D-58: A collapsed item that nudges comes out for a moment

**Status:** accepted (2026-10-13). Extends [D-55](#d-55-several-islands-per-zone-collapsible-items-and-dividers-set-in-configtoml) and [D-32](#d-32-plugins-can-nudge-their-island).

**What:** when a widget whose item is collapsed asks for a nudge, the host shows that item (only that one, not the island's other collapsible items) for `REVEAL_SHOWN` (2 s) after the last nudge, then hides it again. Raising the volume with a collapsed volume item shows the new level. Each instance remembers which reveal shows it (`Instance::revealed`, numbered by `Bar::reveals`), and a timer per reveal sends `Unreveal(id, reveal)`. Only the last one counts, so every nudge keeps it out longer. While the item's popover is open it stays out, and the timer starts again when it fires. Expanding the island makes reveals moot and clears them.

**Why the nudge is the signal:** a nudge already means "what this item shows just changed, and it's worth seeing". Every widget that nudges (volume, media tracks, a future brightness) gets the reveal with no new API, and widgets don't need to know whether they're collapsed. The item comes and goes through the island's morph (D-27), the same motion as pressing the chevron.

**Alternatives:**
- **`Effect::reveal()`:** an explicit request, separate from the motion. It's more API for what is, so far, always said together with a nudge. It can still be added if a widget ever needs one without the other.
- **Expanding the whole island:** shows items that didn't change, and pulls the chevron's state into a timer.

**Cost:** one sleeping task per nudge from a collapsed item. Nothing polls.

---

## D-59: `ui::center` and a centered media popover

**Status:** accepted (2026-10-13). Extends [D-40](#d-40-a-popover-vocabulary-lists-sections-controls-one-panel-width) and [D-56](#d-56-media-over-mpris-in-eave-mpris-on-the-session-bus).

**What:** the media popover was a plain column, with everything on its left. Long titles wrapped onto several lines, the player's name showed twice for a track with no title, and `.prominent()` did nothing on the icon-only play button. Now:
- **`ui::center(node)`** centers `node` across the panel's width (`container(..).center_x(Fill)`). In the bar it's just `node`. Labels inside it keep to one line and are cut and faded at the edge (`Clip`, which is only as wide as its text, so short text centers).
- **A `heading` in a popover keeps to one line**, cut like the header's title. Every heading so far is a title, and a two-line title pushed the panel taller as tracks changed.
- **A prominent icon button** is a filled accent circle, `PROMINENT_ICON_BUTTON` (36 px), with the icon in `ON_ACCENT`. *Superseded by [D-61](#d-61-black-and-white-and-color-only-as-a-signal): a white disc (or the cover's color), `PROMINENT_BUTTON`.*
- **The media popover** is the header with the player's name, then the cover, the title and "artist · album" centered, the seek bar, and previous, play/pause and next centered, play/pause prominent. The player list follows as before.

**Alternatives:**
- **An alignment option on `column`:** it would center everything in it, the seek bar's times and the player list included, so a popover would need columns inside columns. `center` wraps just what should be centered.
- **Cover beside the text:** more compact, but the cover is the popover's picture. The centered layout was picked.

**Cost:** none measurable. One `container` per centered node while the popover is open.

---

## D-60: Live islands, set on the island marker

**Status:** accepted (2026-10-13). Extends [D-55](#d-55-several-islands-per-zone-collapsible-items-and-dividers-set-in-configtoml) and [D-26](#d-26-squircle-islands-that-squeeze-and-spring-back).

**What:** `live = true` beside `island = true` makes that island a live one. For now that only changes its shape:
- **Snug:** `LIVE_ISLAND_PADDING` (4 px) at its ends instead of `ISLAND_PADDING` (14), and never narrower than it is tall. An island of one icon is about 36 × 30, nearly round. *(Since [D-78](#d-78-live-islands-take-their-corners-from-where-they-sit), the room at its ends is the room above and below its items.)*
- **Rounder:** `LIVE_ISLAND_RADIUS`, half the island's height *(half its own height since [D-76](#d-76-islands-are-as-tall-as-the-notch))*, with the same smoothing, so it's still a squircle. As it grows into a popover the radius eases back to `ISLAND_RADIUS` over the first island height, so its panel looks like any other. *(Superseded by [D-78](#d-78-live-islands-take-their-corners-from-where-they-sit): a live island's corners are any island's where it sits.)*

A marker before the zone's first item sets the first island, so no island has to be empty to be live. `live` that isn't a boolean is warned about and ignored. Every entry of the island carries `live` (`Entry::live`), and the view reads it the way it reads `collapsible`. The island rebuilds its cached shape when it changes.

**Why a property of the island, not a shape flag:** "live" is meant to grow. A live island is one showing something happening now, and it may get more behaviour later. The shape is only the first thing it changes.

**Alternatives:**
- **`live = true` on an item,** making its island live: like `collapsible`. But it describes the island, and with several items it would be unclear which one decides.
- **Settable padding and radius:** more knobs than anyone needs, and it makes the look the config's, not the host's (D-13).

**Cost:** none. One more branch in layout and drawing.

---

## D-61: Black and white, and color only as a signal

**Status:** accepted (2026-10-13). Replaces the blue accent used since the first popovers (D-40). The rules are in [design.md](design.md). *Amended by [D-92](#d-92-light-and-dark-the-palette-mirrors): this is the dark scheme; the light one mirrors it, white islands and black ink.*

**What:** the bar is black and white. Brightness, not hue, says what's current, and color appears only as a signal or when it comes from the content. Code counts as content since [D-65](#d-65-code-is-content-colored-code-diffs-and-timelines).
- **Ink.** `Tone::Normal` is white at 94 % and `Tone::Dim` white at 50 %. Both are neutral; they used to have a blue cast. They're written as the opaque grey they make on black, so overlapping icon strokes don't show seams. `Tone::Accent` is plain white: the thing to notice is the brightest thing there. Plugins keep the same `Tone`s. Only what they look like changed.
- **Current is lit.** The selected list row gets a 10 % white fill, a semibold title and white icons. The workspace mark is a 14 % white pill. Tabs get a 16 % pill on their track, with the current label white and semibold and the others dim. Today in the calendar is a 1.5 px white ring. The toggle is a white pill with a black knob when on. A prominent button is filled white with black text, and a prominent icon button (play/pause) is now drawn as a 36 px white disc; before, `prominent` was ignored for icon buttons. A focused field's edge is 50 % white.
- **Signals are vivid:** good `#30D158`, warning `#FFB020`, critical `#FF453A`, up from pastels. They're now the only color on screen, so they should be seen when they show up.
- **The slider is a line.** It's a new host widget (`slider.rs`) in place of iced's slider: a 2 px line, white up to the value, with no knob. It eases to 4 px under the pointer or while dragged, and back when left. A click eases the fill to where it landed, and a drag follows exactly. It behaves like iced's slider: the change on press and drag, release message, Ctrl+wheel and arrow keys.
- **Content color.** While a cover is shrunk, the host also finds its strongest hue (a 12-bin hue histogram over the 32 px copy, weighted by saturation × value) and lifts it to a lightness that reads on black. A popover that shows a large cover uses that color for its slider fill and prominent button. A cover with little color gives none, and they stay white. No API change: the host infers it from the cover, and plugins still never pick a color (D-11).

**Alternatives** (compared side by side in a style study, with a live preview of each in the full bar):
- **Keep the blue tint** for what's current. Blue meant "selected" in four places and "interactive" in two more, so it signaled nothing in particular, and it fought with the album art and app icons.
- **Solid white with black text** for what's current. It's very clear, but heavy: a white row or pill is the loudest thing in a popover, louder than a warning.
- **Lines and dots only** (an underline for tabs, a dot under today, a check with no fill for rows). It's the quietest option, but the current row is hard to find at a glance in a long list.
- **A knob on the slider, or a thick bar.** The knob is the one round shape in a flat line, and the thick bar looked like a meter. A thin line that grows under the hand fits "thin at rest, alive when touched".
- **iced's slider with a status-based style.** It can switch between 2 and 4 px, but only in one jump: the style function has no time, so it can't ease.
- **A plugin-chosen tint** (`.tint(color)` on a slider). It would break D-11. The cover already carries the color.

**Measured** (Asahi, release, the user's config, no popover open, alternating runs in one session): the binary is 11,693,568 bytes before and after (identical, the change fits in the same pages). RSS was 18.5–18.8 MB on both, and idle CPU 2–4 ticks in 10 s on both. The content color is three floats per cover, computed from the 32 px copy that was being made anyway.

**Cost:** one more host widget, about 250 lines with tests. Tone colors changed for every plugin at once, which is the point.

---

## D-62: Reading and writing in popovers: Markdown, a text area, scrolling, wide panels

**Status:** accepted (2026-10-13). Supersedes the "Reply button first" part of [D-49](#d-49-answering-a-notification-a-field-and-the-keyboard-only-once-asked). The notice's picture and reply field are amended by [D-85](#d-85-notification-cards-fixed-slots-pictures-choices-stacks).

**Why now:** the Claude plugin's popover becomes a chat: a list of sessions, and for each one its conversation, what it's doing, and a field to answer it. Its replies and plans are Markdown, its messages are several lines long, and a conversation is taller than any screen. Its notifications should take an answer without a click on Reply first. None of this is Claude's alone: any messenger, mail or notes plugin needs the same pieces.

**What:**
- **`ui::markdown(text)`.** The host reads it with `pulldown-cmark` (no default features) and builds plain iced widgets (`markdown.rs`): paragraphs as rich text with bold, italic, struck, link and inline-code spans; headings; bullet and numbered lists, nested; quotes with a bar; code blocks in monospace on a tinted box that scrolls sideways; tables with cells sharing the width; rules; task list boxes. Colors and sizes are the theme's, and `.tone(..)` sets the text's color. Links are drawn as links but don't open: the bar starts no programs. Everything built is owned, so nothing borrows from the plugin's view. It's parsed on each view, which runs after a message, not on every frame. Pulldown-cmark reads a long reply in well under a millisecond, so no cache yet.
- **`ui::text_area(placeholder, value, on_input)`**, with `.on_submit` and `.trailing`: several lines, growing to six then scrolling (`text_area.rs`). Enter sends `on_submit`; Shift+Enter breaks the line. Esc stays the bar's: it closes the popover, as with a one-line field (D-41). It's iced's text editor, whose `Content` lives in the widget's tree state instead of the app's: the plugin keeps a plain string, the widget follows every edit and sends the new text, and takes the plugin's text again when it differs (cleared after sending). It goes in the same `Field` box as `text_input`, which now grows with it.
- **`ui::scroll(content)`**, with `.bottom()`: in a popover, its content scrolls once taller than `SCROLL_MAX_HEIGHT` (520 px). `.bottom()` keeps the end in view as content arrives there, unless you've scrolled up to read (iced's `anchor_bottom`). In the bar it's just its content.
- **`Panel::wide()`:** the popover is `PANEL_WIDE_WIDTH` (480 px) instead of 320. The host still picks the number. Every place that took `PANEL_WIDTH` now takes the popover's own width (`island::Popover::width`).
- **Notices:**
  - `.image(url)` shows a picture by URL instead of the app's icon, loaded like a `cover` (D-57). Covers now also take SVG, drawn with resvg, which was already in the build. This is how an app's icon file reaches a notification (eave-notify's `image-path` hint and `app_icon` paths).
  - `.markdown()` sets the body as Markdown.
  - The reply field is a text area too.
- **The reply field shows with the notice; the keyboard waits for a click on it.** `Field::on_focus` tells the host when the field gets the focus or loses it. The host marks that notice engaged, and only then does it take the keyboard (Exclusive, D-36) and stop the clocks. Esc cancels the answer, unfocuses the field and gives the keyboard back. So D-49's promise holds — a notification never takes the keyboard by itself — without the extra Reply click. The pointer works without the keyboard, which is what lets the click reach the field.
- **`Effect::open_popover()` before the item was ever clicked** opens on the bar the notifications show on. Before, it refused with a log line, so a notification's action couldn't open its plugin's popover until you had clicked that plugin's item once.

**Alternatives:**
- **iced's `markdown` feature:** its widget borrows parsed items for the view's lifetime, which plugins' owned nodes can't give without a cache in the app keyed by text. It also brings its own styling and url parsing. Walking pulldown-cmark's events ourselves is ~300 lines, and owned.
- **Plugins rendering Markdown into rows of labels:** every plugin would parse it on its own, and the vocabulary has no spans.
- **A text area as a new widget from scratch:** cursor, selection, IME and clipboard are what iced's editor already does.
- **`KeyboardInteractivity::OnDemand` for the reply field:** with `follow_mouse`, the focus would leave as soon as the pointer left the bar (D-36, D-49).
- **A wider panel for every popover:** the narrow ones are better for lists and switches. Only reading pages ask for room.

**Measured** (aarch64, release, the user's config, 10 s windows after 8 s, run beside the user's own bar): binary 11,693,568 → 12,152,320 bytes (+448 KiB: pulldown-cmark, iced's text editor and the new widgets). RSS 18.8 → 19.3 MB on a quiet run, the code pages. Idle CPU unchanged: 1 and 3 ticks per 10 s. A first, noisier run of each measured 20.1 and 21.0 MB, with 105 and 91 ticks.

**Cost:** +448 KiB of binary for every bar, whether or not a plugin shows Markdown or a text area. One more dependency (pulldown-cmark, pure Rust).

---

## D-63: The battery sends its own notifications

**Status:** accepted (2026-10-13).

**What:** `battery` sends two notifications over the session bus, as D-52 says plugins do:
- **Battery low**, critical urgency, once the discharging battery is at or under `critical` (10%), with the time left. Once per discharge, also when the bar starts on a low battery. It never expires, and it's taken away (`CloseNotification`) as soon as the charger is in.
- **Battery charged**, normal urgency, when it goes from charging to full, or to held at its charge limit. Plugging in a battery that's already full isn't news, so it says nothing.

The icons are theme names (`battery-caution`, `battery-full-charged`). `notify = false` turns both off. The threshold is the one that turns the icon red, so what the bar shows and what it says agree.

**Alternatives:**
- a separate warning threshold for notifications: another knob for what's already `critical`.
- a notice of its own (`notices()`): ruled out by D-52.
- a notification at `warning` (20%) too: the icon already turns orange there. One interruption is enough.

**Why:** a bar that knows the battery is about to die should say so where you'll see it, without needing a separate daemon (D-23 rules out UPower's).

**Cost:** `eave-dbus` (zbus) in the battery plugin. It was already in the default build through media, but a config with `battery` and no D-Bus plugin now links zbus. The session bus connection is only opened when a notification goes out. *Measured* (release, built-in plugins): binary 12,217,856 → 12,611,072 bytes (+384 KiB, the serialization of the `Notify` call). Idle CPU and RSS unchanged, since nothing runs until a notification is sent. With no notification daemon, they're lost, as any app's would be.

## D-64: Sprites: a plugin's little animation, played by the host

**Status:** accepted (2026-10-13).

**What:** `ui::sprite(frames, order)` is a flipbook a plugin ships: PNG or SVG frames compiled in, and the order they play in, by index. Frames keep their shape; the host picks the height, `SPRITE_HEIGHT` (15) in the bar and `SPRITE_HEIGHT_PANEL` (20) in a popover's rows, and scales pixels up with nearest-neighbour filtering, so pixel art shipped at its own size stays sharp (10 rows of art become 30 and 40 screen pixels at scale 2). The host shows a frame for `SPRITE_FRAME` (250 ms). It plays the loop `SPRITE_PLAYS` (2) times when the sprite shows up or changes to another one, then rests on the first frame of `order`. `.looping()` keeps it going for as long as it shows. The first user is eave-claude's mascot, whose mood (planning, coding, asking, done, coffee, asleep) is a sprite in the bar, in front of each chat in its popover, and as its notifications' picture.
- Frames are decoded once, off the UI thread, at their own size (`Icons::frames`, keyed by `FileKey`), not into the 32 px square `image` uses: forced into an icon's square, a 16×10 mascot was 10 px tall and blurred. Each is then enlarged whole pixels at a time to at least 40 rows, the most a sprite shows on a scale-2 screen: iced's tiny-skia renderer places an image at whole pixels *of the image* before scaling it, so a 16×10 frame drawn at 24×15 landed up to 3 screen pixels off, past the area it repaints, and left trails as its island moved. Scaled down instead, it's off by under a pixel.
- The widget asks for a redraw only when the picture changes (`request_redraw_at` the next frame that differs), never at the compositor's rate. At rest it asks for nothing.

**Alternatives:**
- the plugin ticking `time::every(250ms)` and swapping `image`s: works with the old API, but plugins don't animate (D-13), it would keep the plugin's `update` and every view rebuild running too, and the host couldn't stop when nothing shows.
- an animated GIF or APNG decoded by the host: a codec for what a few PNGs already do, and no say over when it rests.
- always looping in the bar: the mascot shows while sessions work, which is most of the day, and every frame redraws the whole surface (D-56). eave-claude's old pulse while busy cost ~10% CPU and was taken out.

**Why:** a mascot that reacts when something happens and then sits still says "something changed" without costing anything at rest. Plugins name the frames; the host owns the timing, like every other motion.

**Cost:** *Measured* (release, a bar with the clock and eave-claude, one Claude session, against the same bar before):
- idle CPU at rest: unchanged, 2–3 ticks in 30 s either way.
- a question looping in the bar: 4 ticks in 30 s (~0.1%). Its loop changes picture twice in two seconds, and only those frames are drawn.
- a change of mood playing out (laptop, 2 loops): 4 ticks over the 4 s it plays, then rest.
- RSS: 15.7 → 16.2 MB, for 19 frames drawn at 32 px and the PNG decoding.
- binary: 10,448,384 → 10,448,544 bytes; the frames are ~380 bytes each.

Those numbers are tiny-skia's. On the GPU, which renders by default since D-67, a loop costs more per frame: see [D-99](#d-99-what-continuous-animation-costs-measured-no-change).

## D-65: Code is content: colored code, diffs and timelines

**Status:** accepted (2026-10-13). Amends [D-61](#d-61-black-and-white-and-color-only-as-a-signal): code joins covers as content that brings its own color. *Amended by [D-92](#d-92-light-and-dark-the-palette-mirrors): the light scheme has its own, deeper code hues.*

**Why now:** eave-claude's chat is mostly code, commands and changes to files, and Claude's steps. Set in one grey, a code block was a wall: keywords, strings and comments all looked alike, an edit showed only as its file's name, and every tool call took a full line with a section header for each speaker. The chat was compared in four prototypes; this is the "rail" one, with muted hues.

**What:**
- **Colored code.** Markdown code blocks keep their fence's language, and the host colors them: keywords, types, calls, strings, numbers, comments and punctuation, in soft hues of about the same lightness (`CODE_*` in `theme.rs`). Comments and punctuation sit back, at 45–55 %. Blocks of four or more lines number their lines. Everywhere Markdown shows, notifications too.
- **The highlighter is ours** (`highlight.rs`): one pass with a table per language (Rust, shell, Python, JS/TS, C/C++, Go, Lua, TOML, JSON, YAML; anything else gets strings, numbers and comments). It doesn't parse, so a keyword used as a name gets the keyword's color now and then.
- **`ui::diff(title, lines)`**: a change to a file, `Change::{Added, Removed, Context, Hunk}` per line. The host counts the `+`/`−` for its header, colors the code by the file's extension, and lays the added and removed lines on faint green and red. Those two are signals (gained, lost), so they're the signal colors, faint. Diff lines wrap, so each line's color reaches the box's edge.
- **`ui::timeline(steps)` and `ui::step(mark, content)`**: steps strung on a thin line (`TRACK`), each marked with where it stands, `Mark::{You, Note, Running, Done, Failed, Waiting}`. Brightness first: you are a white dot, a note a small grey one, done grey, under way a white ring. Only news is colored: failed is critical, waiting for you is a warning ring. `.leading(node)` puts a small picture on the line instead of the mark (the mascot). Nothing pulses: a running mark is a still ring with a faint halo (D-64).
- **`ui::literal(text)`**: text as typed (a path, a command), monospace, one line, its end fading out when it doesn't fit.
- **`ui::divider(text)`**: a separator with a caption in its middle, like the time a turn started.
- **`header(..).leading(node).subtitle(text)`**: a popover page's title can have a small picture before it and a dim line under it, like a chat's mascot and the folder it works in. Both are new fields; `Kind::Header` is matched with `..` everywhere, so nothing breaks.

**Alternatives:**
- **Brightness only** for code: keywords white and bold, strings mid-grey, comments dim. It follows D-61 to the letter, and it was a prototype. But code has more than three kinds of things in it, and brightness ran out: strings, calls and types blurred together. Code isn't a state, but like a cover it's content with structure of its own, and the hues come from it, not from the bar.
- **iced's `highlighter` feature** (syntect): real grammars for hundreds of languages, but megabytes of syntax and theme dumps in every bar (D-5), and its own colors.
- **tree-sitter**: exact, but a C library and a grammar per language.
- **Plugins coloring code** (spans with colors in the API): breaks D-11. The host knows it's code, so the host colors it.
- **New `Label` and `Separator` fields** (`.mono()`, `.label()`) instead of `literal` and `divider`: plugins match on `Kind::Label { text, tone }` and `Kind::Separator` in their tests, so changing those variants would break them. New kinds are additions.

**Why:** reading code and edits is most of what the chat is for, and colors are how people read code everywhere else. The timeline shows the shape of a turn at a glance: what you asked, what it did, what failed, what waits for you.

**Cost:** *Measured* (release, built-in plugins, `cargo build --release`): binary 12,611,072 → 12,676,608 bytes (+64 KiB) for the highlighter, the timeline and diffs. No new dependency. Nothing animates, so nothing asks for frames; idle CPU and RSS are unchanged with no chat open. A code block is lexed on each view, like Markdown is parsed (D-62), after a message, never per frame.


---

## D-66: A patched iced_tiny_skia, and less work per message

**Status:** proposed (2026-10-13). Corrects what D-56 and D-64 say about redrawing "the whole surface".

**Why now:** eave-claude's chat popover felt like 10 fps: opening and closing, scrolling, and while the chat updated. The popover is the largest thing eave has drawn so far (a few megapixels at scale 2, full of text), so costs that didn't show on a 32 px bar now did.

**What a frame cost, as found:**
- iced_tiny_skia already redraws only what changed: it diffs each frame's layers against the ones the buffer last held and rasterizes only inside the damage. D-56 and D-64 overstated it.
- But it **cleared and refilled a surface-sized clip mask** once per layer for every damage rectangle, and twice more per group of primitives, whether or not anything drawn needed clipping. With a popover open that's several MB of memory written per layer, per frame. It's likely also why D-37 found 15 fps and 30 fps cost the same.
- And it **told the compositor the whole surface changed** (`present()`), so Hyprland took the whole buffer every frame.
- Apart from the renderer, every message rebuilds every view. A chat that keeps its latest 200 entries dropped its first one on each new one, so every step's widget state moved to the step below and **all its text was shaped again** on every update. The clock sent one such message a second, and the host built each widget's view several times per message.

**What:**
- `vendor/iced_tiny_skia` is iced_tiny_skia 0.14.1 with three changes, each marked `eave patch`, used through `[patch.crates-io]`. `eave-build` writes the same patch when the eave checkout has it:
  - the clip mask is set lazily: a new clip is only recorded, the mask is filled when a draw needs it, and only the old rectangle's pixels are cleared;
  - `present_with_damage`, with the damaged rectangles in physical pixels. With an older buffer, the change since the frame on screen is added, since that's what the compositor shows;
  - `EAVE_FRAMES=1` prints once a second: frames, draw time, damaged pixels, layers.
- A timeline's steps carry a key, a hash of what they say, and their state follows them when the oldest ones are dropped (`timeline.rs`). No API change.
- A still icon draws through a canvas cache, so it isn't rebuilt and doesn't count as damage when something else moves (`icon.rs`).
- After a widget's message, icons are looked for only in that widget's views, and its popover isn't built twice (`app.rs`).
- The clock checks the time every second off the UI thread, but sends a message only when its text changes: once a minute with the default format.

**Alternatives:**
- **The `gpu` renderer.** Large surfaces are what GPUs are for, and it doesn't touch the per-message costs, which show while the chat updates. It costs ~60 MB of RSS for the whole session (D-4). Compared by hand against this before choosing; see the measurements.
- **Upstreaming the patch**: worth doing. Drop `vendor/iced_tiny_skia` when a release has lazy clipping and damage-aware presents.

**Measured** (Asahi, release, the user's config with eave-claude, scale 2, 120 Hz). Three bars side by side, untouched, three 30 s windows after a 60 s warm-up. The middle window had an animation on every bar (eave-claude's island changing as a session worked):

| Build | Binary | RSS | CPU (quiet, busy, quiet) |
|---|---|---|---|
| `main` | 13,987,728 B | 21.4 MB | 0.33%, 1.27%, 0.33% |
| this, tiny-skia | 13,987,728 B (same pages) | 21.4 MB | 0.10%, 0.73%, 0.10% |
| this, `gpu` | 18,247,680 B | 77.6 MB | 0.10%, 1.00%, 0.13% |

- Idle CPU drops from 0.33% to 0.10%: the clock no longer rebuilds the bar every second.
- During an animation on the bar, CPU drops by about 40% (1.27% to 0.73%). A bar frame draws in under 1 ms.
- With the chat popover open (surface 3024×1560), tiny-skia still took **20–42 ms a frame** while it moved, peaking near 90 ms: 25–50 fps, not 120. What's left is rasterizing a large popover of text on the CPU.
- `gpu` costs +56 MB of RSS and +4.3 MB of binary, as D-4 found. Its idle CPU is the same as tiny-skia's.
- The user compared the two with the chat popover open. The GPU felt much smoother, so it became the default ([D-67](#d-67-the-gpu-renders-by-default-renderer--cpu-in-configtoml)). The patch stays for `renderer = "cpu"`, and for machines where wgpu can't start.

---

## D-67: The GPU renders by default; `renderer = "cpu"` in config.toml

**Status:** accepted (2026-10-13). Supersedes [D-4](#d-4-cpu-renderer-tiny-skia-by-default-gpu-wgpu-optional).

**Why now:** D-4 picked tiny-skia because the bar was a 32 px strip. Since then, popovers have grown to a chat full of text, a surface of 3024×1560 at scale 2. Even with D-66's patch, tiny-skia took 20–42 ms per frame while that popover moved or scrolled, peaking near 90 ms. That's 25–50 fps on a 120 Hz screen, and "smooth" comes first ([vision.md](vision.md)). Side by side, the user found the GPU build much smoother.

**What:**
- `gpu` is a default feature of `crates/eave`, and `eave-build` turns it on in the bars it builds. Both renderers are compiled in. iced tries wgpu first and falls back to tiny-skia if it can't start, e.g. with no Vulkan driver.
- `renderer = "gpu"` (the default) or `renderer = "cpu"`, at the top of config.toml, picks one. eave sets iced's `ICED_BACKEND` from it at startup (`wgpu,tiny-skia` or `tiny-skia`). An `ICED_BACKEND` already in the environment wins. The renderer can't change on a live reload, so a reload that changes it says so, and the bar uses it from its next start.
- A build without the feature (`--no-default-features --features builtin`) is CPU-only and ignores the key.

**Cost** (Asahi, release, the user's config, the same session as D-66):

| Build | Binary | RSS | PSS once a popover has opened | Idle CPU |
|---|---|---|---|---|
| tiny-skia | 13,987,728 B | 21.4 MB | ~50 MB | 0.10% |
| `gpu` | 18,247,680 B | 77.6 MB | ~57 MB | 0.10–0.13% |

- RSS overstates the cost of the GPU. About 40 MB of it is Mesa and Vulkan libraries, shared with Hyprland and every other GPU program.
- Once a popover has opened, the CPU renderer keeps ~15 MB of shared-memory buffers (D-28) that the GPU doesn't need. The real difference is then ~7 MB.
- Idle CPU is the same.

**Alternatives:**
- **Keep tiny-skia and make it faster.** D-66 did the cheap part. What's left would mean scrolling by moving pixels already drawn (deep in iced, and only helps scrolling), or rewriting how text is drawn. Neither gets a full repaint of a 4.7 Mpx surface under 8 ms.
- **The GPU only while a popover is open:** iced uses one compositor for every surface, so it's both or neither.
- **CPU by default, GPU opt-in:** most users would never find out their popovers can be smooth.

**Revisit if:** the GPU's memory grows past the numbers above, or a target's driver turns out unreliable. Then make `cpu` the default there.

---

## D-68: The GPU blends in sRGB, like tiny-skia (`web-colors`)

**Status:** accepted (2026-10-13). Amends [D-5](#d-5-minimal-iced-feature-set).

**Problem:** with the GPU renderer ([D-67](#d-67-the-gpu-renders-by-default-renderer--cpu-in-configtoml)), borders, the tab mark, hover backgrounds and other grays looked brighter than on tiny-skia. They are all translucent white over a black island (`theme.rs`), and D-5 had dropped iced's `web-colors` feature. Without it, iced_wgpu draws to an sRGB surface and blends in linear light. tiny-skia blends in sRGB, which is what the palette was tuned on. White at α 0.08 over black is ~20/255 on the CPU but ~80/255 on the GPU. Opaque colors came out the same, so only the grays (and anti-aliased edges) differed.

**What:** the `gpu` feature turns on `iced/web-colors`. iced_wgpu then packs colors as they are, picks a non-sRGB surface format, and blends in sRGB, the same as tiny-skia and the web. It only touches iced_wgpu, so CPU-only builds don't change.

**Alternative:** a second set of alpha values for the GPU. Two palettes to keep in step, and anti-aliased edges and text would still disagree.

---

## D-69: The bar goes around the notch

**Status:** accepted (2026-10-13).

**Problem:** with `appledrm.show_notch=1` (`apple_dcp.show_notch=1` on older kernels), Asahi gives the MacBook's full panel to the output, and its top center is a hole. A centered island ends up half under the notch.

**What:**
- `notch = "auto"`, the default, finds it.
  - The kernel parameter says whether the notch shows.
  - The display controller's `apple,notch-height` in the device tree says it's there.
  - A table of measured models (`notch.rs`) says how wide and tall it really is. The kernel's 74 px hangs below the notch on a 14", which looks like 66.
  - An unmeasured model gets a width wide enough for any MacBook, and a message asking for `notch = { width = … }`.
  - Only on a built-in panel (`eDP-*`) that isn't rotated.
- `notch = false` ignores it. `notch = { … }` says what it is: see D-71 for the keys and their units.
- The center zone splits in two around it. Its first half of islands goes before the notch, the rest after. The two islands against it reach under it and up to the top edge, so they and the notch read as one black shape. Popovers stay on their island's side.
  - *Superseded in part by D-73:* they fuse only with `attach = "top"`. Without it, they sit a gap away and are rounded like the notch. Always fusing left no way to have islands float beside the notch.

**Alternative:** leaving the center zone empty, as platform.md planned. That wastes the room on both sides of the notch, where the clock fits.

---

## D-70: Islands attach to the screen's edges

**Status:** accepted (2026-10-13).

**What:** an island marker takes `attach = ["top", "left"]` (or `"top"`, or `"right"`). An attached island reaches across the edge gap and on past the edge by `ATTACH_OVERHANG`. Its corners there are out of sight, so it looks as if it grows out of the screen's edge. It reaches past the edge so that a jelly or a drag never shows it coming away. Only the first island on the left can attach left, and only the last on the right can attach right: no other island is at the side.

**Why:** next to the notch, an island floating 6 px below the top edge looks like a mistake. An island hanging from the edge looks like part of the same hardware.

---

## D-71: `eave calibrate` measures the screen's corners and notch

**Status:** accepted (2026-10-13).

**Problem:** the notch's size came from a table fitted by eye: 74, then 64, then 66 px. The bar knew nothing about how rounded the screen's corners are, or whether the bezel hides any rows.

**What:** `eave calibrate` covers the focused output with a model of what the panel shows:
- a 1 px red line just inside its edge;
- blue over what it says is hidden;
- black inside.

The model is right when every red pixel shows and no blue one does. The keys change the screen's four corner radii, their smoothing, the notch (on or off, width, height, lower corners, smoothing, shoulders, and how far below the top it starts), and the four insets. A panel in the middle shows the rows, as an island that morphs like the bar's: the morph spring for its size and the mark on the selected row, the fade spring for its lines. Enter saves to config.toml.

```toml
notch = { width = 370, height = 66, radius = 20, smoothing = 0.6, shoulder = 8, top = 0 }
screen = { corners = [20, 20, 0, 0], smoothing = 0.6, inset = [0, 0, 0, 0] }
```

- **In `eave`, not `eave-build`.** It needs iced, the layer shell, the squircle and the springs, which `eave-build` has none of. `eave::run` checks for the `calibrate` argument, so bars built by `eave-build` have the command too.
- **One curve.** The screen's corners, the notch and the islands are all drawn through `squircle.rs`. Each corner is a circle's at `smoothing = 0` and a squircle's above it. A corner can turn either way, which is how the notch's shoulders round the screen outwards. What the tool measures is the curve the bar draws.
- **Exact pixels.**
  - The pattern is an image at the output's physical size, shown at its logical size without filtering, so each of its pixels is one of the screen's.
  - The outline is flattened to 0.005 px and filled here: a pixel is in when its center is. tiny-skia's fill without anti-aliasing flattens curves so coarsely that a pixel 0.18 px inside a circle came out hidden, so it isn't used.
  - The pattern is redrawn on a blocking thread after each change, never on the UI thread.
  - Each new pattern is allocated (`iced_runtime::image::allocate`) before it replaces the one on screen, so the old one stays until the new one can draw. On the GPU, the upload ends on iced_wgpu's worker thread, and iced_layershell 0.19 drops the redraw it asks for. So the panel asks for frames while a pattern is on its way ([roadmap.md](roadmap.md#known-issues)).
- **Physical pixels** in config.toml, the hardware's unit and the model table's. The bar divides by the output's scale. This changes `notch = { width, height }` from logical pixels, which only this branch ever had.
- **toml_edit** (`parse`, `display`, the version zbus's proc macros already build) saves only `notch` and `screen`, and keeps everything else in the file, comments included. The write goes through a symlink (dotfiles managers link config.toml) and lands by a rename.
- **Red and blue**, a tool-only exception to "no new colors" ([design.md](design.md)): they have to be told apart from each other, from black and from the bezel, one pixel at a time.

**Cost:** release binary 16,936,560 → 17,264,240 B (+328 KB, mostly toml_edit). The bar's RSS and idle CPU are unchanged: none of the calibrator runs in it, and resolving the screen is one Hyprland query when config.toml has measurements.

**Alternatives:**
- **Print a snippet to paste:** no new dependency, but a step to get wrong after a careful measurement.
- **A floating window instead of a layer surface:** it couldn't cover the notch, the corners or the bar's exclusive zone.

**Limits:** a notch is centered on the whole output, insets aside. A bezel that hides more on one side than the other would put it off center in the bar, which centers it between the insets.

*Superseded in part by [D-94](#d-94-screen-settings-per-monitor): saving writes `notch` and `screen` into the monitor's `[monitor."…"]` table, not the top level.*

---

## D-72: The bar keeps off what the bezel hides, and hangs from the top like the notch

**Status:** accepted (2026-10-13).

**What:**
- **Insets** are the bar surface's layer-shell margins. The bar lays itself out on the part of the screen that shows, and the compositor keeps windows below it.
- **A lower or taller notch:** the islands against it reach down as far as it does, `top` included.
- **Shoulders:** an island attached to the top is rounded outwards where its free sides meet the edge, with the notch's `shoulder` and `smoothing` (`squircle::hanging`). Islands then hang from the top the way the notch does, and the notch's fused islands carry its outline on. The free sides are the ones that don't reach on to a screen edge or under the notch. A shoulder is never wider than the room the island's frame keeps around it (14 px).
- With nothing measured, the bar draws as before.

**Not done: concentric corners.** *Superseded by D-73, which does them.* The plan was for the islands nearest the screen's rounded corners to follow them at an even distance, with radius = screen radius − gap. An island is 30 px tall, so its corners already top out at 15 px. Against a MacBook's ~20 px corners, that's what concentric would give anyway, so it would change nothing visible. Revisit if islands get taller, or there's a screen with much rounder corners.

---

## D-73: Islands take their corners from the screen

**Status:** accepted (2026-10-13).

**Problem:** `eave calibrate` measures the screen's corners and the notch, but the bar used only the notch's size and shoulders. Every island had the theme's 14 px corners, whatever the screen looked like, and the screen's `corners` weren't read at all.

**What:** each corner of an island takes its curve, radius and smoothing together, from what it faces:
- **A screen corner:** the top left of the first island on the left, and the top right of the last on the right, follow the screen's corner at an even distance: radius = the screen's − `EDGE_GAP`, with its smoothing. A screen corner no rounder than the gap leaves the island's as it would otherwise be.
- **The top edge:** an island attached there hangs from it with the notch's shoulders, as before (D-72).
- **The notch:** a center island beside the notch that isn't attached to the top sits a gap away from it. On that side, its top corner is rounded like the notch's shoulder and its bottom one like the notch's lower corner, so each corner matches the part of the notch it faces. A notch with no measured shoulder or lower corner leaves that corner to `islands`. With `attach = "top"`, it's fused to the notch, as D-69 had it for every island there.
- **Anything else:** `islands = { radius, smoothing }` in config.toml, in physical pixels. Each is `"auto"` by default: the notch's corners, then the screen's top corners at an even distance, then the theme's 14 px and 0.6. With nothing measured, the bar looks as before. A notch found by `"auto"` has no measured corners, so it's skipped.

A corner that's attached somewhere is out of sight, so whatever it would be doesn't matter. An attached island's items sit in the middle of what shows of it, not of its layout: 3 px toward each edge it reaches, and up again by what a notch shorter than the island leaves below it. A live island is still round until it grows into a popover, and then eases into these corners. *(Since D-78, it has these corners from the start.)* `notch::Screen` resolves all of this once per setting, so drawing reads four `CornerShape`s per island.

**Why concentric:** two curves the same distance apart all the way round read as one shape nested in the other, which is what the islands near the screen's corners should look like. With the MacBook's 19.5 px corners at scale 2, that's 13.5 px. D-72 thought this would be invisible against the theme's 14 px. With the notch setting the rest at 10 px, it isn't.

**Why `attach` decides fusing:** it's the key that already says "this island grows out of the screen", and the island against the notch is no different. A floating one is now possible, and a fused one says so in the config.

**Alternatives:**
- **The same radius as the screen's corner:** 19.5 px, which a 30 px island caps at 15. The gap between them would narrow toward the corner.
- **`islands` inside `screen`:** all the geometry in one table, but `eave calibrate` writes `screen` and has nothing to measure for islands. It's a choice, not a measurement.

**Cost:** no new dependency. The corners are resolved when the setting changes, and an island's shape is cached as before, so nothing new runs per frame.

*Superseded in part by [D-95](#d-95-islands-follow-a-measured-screen-nearly-square-and-very-round-ones-too): on a measured screen, a corner no rounder than the gap makes the island's barely round (4 px), and "auto" follows it there too. The theme's 14 px is only for a screen config.toml doesn't describe.*

---

## D-74: Islands have no border

**Status:** accepted (2026-10-13).

**What:** the 1 px hairline drawn just inside every island's edge (`ISLAND_BORDER`, white at 8 %) is gone, here and on `eave calibrate`'s panel. An island is black on black and nothing else.

**Why:** an island fused to the notch, or hanging from the top, is meant to read as one shape with the hardware around it (D-69, D-72). The notch and the bezel have no border, so the island's hairline drew a line along the bottom under the notch and round the shoulders, right where it should have disappeared. A black border would look the same as none, and none is less to draw.

**Alternative:** no border only where an island touches the notch or an edge. Islands would then look different depending on where they sit, for a line that's hardly visible on a dark wallpaper anyway.

---

## D-75: The bar draws the notch, and it can be grown

**Status:** accepted (2026-10-13).

**What:** the bar fills the notch black, under the islands, with its shoulders and lower corners as measured (D-71, D-72). `notch = { …, grow = [sides, below] }`, in physical pixels and `[0, 0]` by default, makes what the bar draws, and goes around, that much bigger than the hardware's: `sides` on each side and `below` at the bottom. The corners and shoulders keep their measured size. `notch::Notch` holds the grown size, and keeps `grow` so the real one can be found again. `eave calibrate` still models the real notch, and writes `grow` back as it was.

**Why draw it:** the notch should be something the bar can move. A squish that shrinks the notch back toward its real size, or bulges it, needs black pixels of the bar's own to move, beyond the hardware's. Drawing it always also hides a calibration a pixel off, which would otherwise show a sliver of wallpaper between the notch and an island fused to it.

**Why in `notch`:** it's about the notch, it's in the same pixels, and `notch = "auto"` has nothing to grow until the notch is measured anyway.

**Alternative:** only make room for a grown notch, and draw it once a squish exists. The extra pixels would show the wallpaper wherever no island is fused to the notch.

**Cost:** one cached path, drawn when the notch changes. No new dependency.

---

## D-76: Islands are as tall as the notch

**Status:** accepted (2026-10-13).

**What:** with a notch, an island's height plus the gap above it is the notch's bottom, grown (D-75), from the top of the screen that shows: `notch.top + notch.height − inset − EDGE_GAP`, at least `MIN_ISLAND_HEIGHT` (20 px). On the MacBook, with the notch 32 px tall at scale 2, that's 26 px instead of the theme's 30. Without a notch, islands are `ISLAND_HEIGHT`, as before. The bar's exclusive zone, input strip and surface follow (`theme::bar_height`, `surface_height`): they're set to the theme's when the bar opens and change once the screen is found. A live island's corners are half its own height, no longer `LIVE_ISLAND_RADIUS`.

**Why:** islands that ended 4 px below the notch looked like they missed it, whether they were fused to it or floating beside it. Lining every bottom up with it makes the bar one row with the notch.

**Alternative:** keep islands 30 px tall and grow the notch to match. That works with `grow`, but makes it a fix for the bar instead of a choice.

**Cost:** windows move a few pixels up once the screen is found, and again if the notch changes.

---

## D-77: Scrolling glides

**Status:** accepted (2026-10-13).

**Problem:** iced's scrollable moves only while scroll events come in. On a touchpad, a list stopped dead the moment the fingers lifted, and a wheel jumped 60 px a step. Hyprland doesn't fling for its clients: like GTK, each client does it itself.

**What:** the host wraps every scrollable it builds (`ui::scroll`, a long `ui::list`, a code block) in `glide::Glide`. Plugins don't change.
- **Touchpad:** its scroll goes straight to the scrollable, so the content still follows the fingers exactly. Glide keeps the last 100 ms of it. When the fingers lift, the content flies on at the speed they were going, which falls to about a third every `SCROLL_GLIDE_DECAY` (0.325 s), and stops below `SCROLL_GLIDE_MIN`. Fingers that stood still for 50 ms before lifting don't fling. Scrolling again or pressing a button stops it.
- **Wheel:** each step adds 60 px to where the content is going, and frames ease it there, two thirds of the way every `SCROLL_WHEEL_EASE` (40 ms). Steps add up without overshooting. A scrollable that can't go further that way leaves the wheel to whatever is around it.
- **Frames:** a fling or a wheel moves the scrollable on frame callbacks, through iced's `scroll_by` operation, and stops asking for them when it settles or reaches the end. At rest, nothing changes.

**Telling the lift apart:** the compositor says the fingers lifted with `axis_stop`, which iced_layershell 0.19 turns into a scroll of zero pixels. `axis_source`, sent before each touchpad scroll, becomes one too. So a zero counts as the lift only if no scroll follows within 10 ms. Where no zero ever comes, 80 ms without a scroll counts as the lift.

**Alternatives:**
- **Patch iced_layershell to pass `ScrollStop` on.** It has the event, but only sends it when both axes stop together, which never happens, and iced has no event to put it in. Another vendored crate, for what the zero already says.
- **A fixed-length fling, eased:** simpler, but a fast flick and a slow push would travel for the same time. A decay keeps the speed the hand gave it.
- **A spring:** nothing pulls a flung list anywhere. It runs out.

**Cost:** no new dependency. Frames only while a fling or a wheel step moves.

---

## D-78: Live islands take their corners from where they sit

**Status:** accepted (2026-10-13). Supersedes D-60's "rounder".

**Problem:** since D-73, every island takes its corners from what it faces. Live islands were the exception: they were round, with a radius of half their height, wherever they sat. Beside the notch, a round island didn't match the notch's shoulder, which every island near it now did.

**What:** a live island's corners are any island's in the same place: the screen's corner at an even distance, the notch's shoulder and lower corner beside it, `islands` elsewhere. It's still snug. Its ends have as much room as there is above and below its tallest item, in what's seen of the island (`shows`, D-73), so the gap around an item is even. On a 26 px island, a 16 px icon has 5 px all round. A live island with no items keeps `LIVE_ISLAND_PADDING`, and it's still never narrower than it is tall.

**Why:** a live island is about what it shows, not about a shape of its own. Matching its neighbors keeps the bar one family of shapes, and the even padding is what still makes it read as snug.

**Alternative:** keep a fixed 4 px at its ends. It was close to the even gap on a 30 px island, but not on the notch's 26 px.

**Cost:** nothing at run time. The easing from round into the panel's corners is gone.


---

## D-79: Fingers landing on the touchpad stop a fling

**Status:** accepted (2026-10-13).

**Problem:** with D-77, putting two fingers back on the touchpad didn't stop a fling until they moved. Fingers that land without moving send no scroll at all. libinput reports them as a hold gesture, which clients like GTK use to stop flings, but layershellev didn't listen for it and iced has no event for it.

**What:** `vendor/layershellev` is layershellev 0.19.1 with one change, marked `eave patch`. It binds `zwp_pointer_gestures_v1` version 3, if the compositor has it (Hyprland does), and asks for a hold gesture on each pointer. When a hold begins, it adds one to a counter, `layershellev::holds()`, and sends a scroll of zero pixels so the bar wakes. On any zero scroll, `Glide` compares the counter with the last value it saw. If it changed, whatever is moving stops where it is, wherever the pointer is. The workspace uses the crate through `[patch.crates-io]`, and `eave-build` writes the same patch when the eave checkout has it.

**Why a counter, not an event:** iced's `Event` is closed, so a hold would have to be dressed as something else on its way through iced_layershell. That means a second vendored crate. A touch event would start a touch-scroll and press buttons. A zero scroll already means "look at the touchpad" to `Glide` and nothing to anyone else (D-77), so the counter just says what kind of zero it was.

**Alternatives:**
- **Patch iced_layershell too, for a proper event:** two vendored crates, and the event still has to become an existing iced one.
- **Stop only when the fingers move again:** what D-77 did. A touch that doesn't move went unnoticed.
- **Upstream first:** worth doing. Drop `vendor/layershellev` once a release has hold gestures.

**Cost:** one more global and one gesture object per pointer, and no new dependency (wayland-protocols already has the protocol). A hold only wakes the bar when fingers land.

## D-80: Brightness through logind and uevents

**Status:** accepted (2026-10-13).

**What:** `plugins/brightness` reads the display's backlight from `/sys/class/backlight` (firmware ones first, then platform, then raw, or the `device` named) and the keyboard's from the first `*kbd_backlight` in `/sys/class/leds`. One thread listens for the kernel's `backlight` and `leds` uevents, as battery does for `power_supply` (D-23), and re-reads when one comes. Levels are set with logind's `Session.SetBrightness(subsystem, name, raw)` on the system bus, through `eave-dbus`. The percent shown is perceptual: raw = max × (percent / 100)², and a reading of the raw level we set keeps the percent we set it to, so a coarse backlight doesn't jump between percents that share a raw level. Scrolling never goes below `min` (default 1), since 0 turns some panels off. Right click does nothing. Two icons join `Icon`: `Brightness { level }`, a sun whose eight rays light up clockwise, and `KeyboardLight`.

**Why:**
- **logind** may write the backlights of the user's own session without root, a udev rule or a group, and it's on every systemd machine. Writing sysfs directly needs one of those set up, and `brightnessctl` is a binary to run (D-23).
- **uevents:** a backlight's core sends one on every write, whoever writes it (checked on the MacBook: a `change` with `SOURCE=sysfs` per `SetBrightness`). So brightness keys bound to anything show and nudge at once, with no polling. LEDs send none, so the keyboard's level is re-read every 2 s, and only while the popover is open.
- **Perceptual:** a backlight is linear in light, so linear steps rush through the dark end and crawl through the bright one. Squaring gives steps that look even. Showing the perceptual percent keeps the number and the slider in step with what the eye sees.

**Alternative:** sysfs writes with a shipped udev rule (one more thing to install); `brightnessctl` (a binary); linear percents (matches sysfs, looks uneven); polling the backlight (wakes up for nothing).

**Cost:** one thread blocked on a netlink socket, one small dependency set already in the tree (rustix, zbus through eave-dbus). External monitors (DDC/CI over `/dev/i2c-*`, which needs `i2c-dev` and group access) are left for later; night light is the compositor's job and out of scope.

---

## D-81: The top edge can be grown too

**Status:** accepted (2026-10-13).

**What:** `screen = { …, grow = N }`, in physical pixels and 0 by default, makes the bar draw the screen's top edge N pixels thicker than it is: a black band from the top of what shows, under the islands, whose lower ends curve down into the sides with the screen's top `corners` and `smoothing`. What shows below it looks like the same screen, a little shorter. Everything that hung from the top edge hangs from the grown one:
- the islands start a gap below it, and the shoulders of those attached to the top (D-72) are where they meet it;
- a notch hanging from the top (`top = 0`) hangs from it too, with its own height, so its depth below the edge, its shoulders and the islands' height (D-76) are as before. Its own `grow` (D-75) adds on top. A cut-out notch (`top` above 0) is a hole in the panel and stays where it is, so the islands beside it get shorter;
- the exclusive zone, the input strip and the surface are N taller (`notch::Screen::bar_height`, `surface_height`), so windows move down that much;
- islands facing the screen's corners keep their concentric corners (D-73): the band's corners are the screen's, moved down.

`eave calibrate` models the real screen and writes `grow` back as it was, like the notch's.

**Why:** the same as D-75's. A squish of the islands hanging from the top, or of the edge itself, needs black pixels of the bar's own to move into, beyond the bezel's.

**Why its own key, not the notch's `grow`:** the top edge is there without a notch (any desktop monitor), so it can't live in `notch`. It's about the screen's edge, next to `inset`, the rows the bezel hides: `grow` is rows the bar adds to it. And the edge and the notch may want different room to squish into. Sharing `notch.grow[1]` would tie the two together and leave a screen without a notch with nothing to set.

**Alternative:** a bigger `inset`. It moves the bar down the same way, but draws nothing: the rows above the bar would show the wallpaper, and the bar would have no pixels there to move.

**Cost:** one cached path, redrawn when the grow, the corners or the bar's width change. No new dependency.

---

## D-82: A count badge for unread notifications

**Status:** accepted (2026-10-13).

**What:** `ui::counted(content, count)` puts a number in a small red badge on its content's top right corner, overlapping it a little: a `Tone::Critical` circle for one digit and a pill for two, with the number in white, capped at 99. The host grows it in from nothing and back on the morph spring, counts through the numbers between like a `counter`, and springs its width from circle to pill. While there's a count, the item is wider by what of the badge sticks out past the content, and the island springs to it; the badge itself is drawn over the content and the item's own padding, so the bell doesn't move as the badge grows. eave-notifications uses it on the bell, and hides it in do not disturb.

**Why:**
- **A host primitive:** plugins don't pick colors or draw (D-11, D-13), so a red badge has to be something the host draws. `badge(text)` was already the outlined code box (a keyboard layout's `US`), hence the name.
- **Red:** unread notifications are news, which is what a signal color is for (design rule 2). It's the one red thing on a black bar, so it gets seen.
- **On the corner, not beside:** the count next to the bell read as another item and made the island jump in width with every digit. On the corner it reads as belonging to the bell, the way phones and desktops show it.

**Alternatives:** keep the counter beside the bell (what it was); a dot without a number (says less); reserve the badge's room always (an island wider for nothing at rest).

**On top:** the badge draws in a layer of its own. Within one layer the renderer draws by kind (shapes, then pictures, then text), not in the order things were drawn, so the bell could show through it. A nested layer isn't clipped by the island's, so the island hands down what it shows (`island::clip`, ambient like the squish) and the badge keeps to it.

**Cost:** one small widget and one layer while a badge shows, no dependency.

---

## D-83: One motion language

**Status:** accepted (2026-10-14). Supersedes the springs and kick speeds of [D-26](#d-26-squircle-islands-that-squeeze-and-spring-back), [D-32](#d-32-plugins-can-nudge-their-island), [D-44](#d-44-popovers-squish-opening-and-splat-closing) and [D-45](#d-45-drags-stretch-near-the-pointer-popovers-too).

**What:** every spring an interaction plays on is one of three feels, and every kick says how far it goes.
- **Feels** (`spring.rs`, numbers in `theme.rs`) are made with `Feel::new(response, damping)`: how long one swing would take undamped, in seconds, and the damping ratio. `HAND` (0.26 s, 0.75) is what the hand moves: the press's squeeze and the drag's stretch. `REACT` (0.20 s, 0.92) is what an event kicks out and back: a nudge's push and lift, the nudged item's kick, every poke of the jelly, a popover's squish and splat. `MORPH` (0.28 s, 0.95) is shapes going somewhere. `FADE` is written the same way but isn't part of the language: it's a value.
- **Kicks by peak:** `Feel::kick(peak)` is the speed that sends a spring at rest out `peak` px at its furthest. It steps a unit kick the way `Spring::advance` does and scales it, because the stepped spring falls a few percent short of the exact solution, more the more damped. The jelly's `poke(peak)` adds what a poke loses to its neighbors (×1.3, measured). So `theme.rs` says `NUDGE_PUSH` 5 px, `JELLY_BUMP` 2 px, `SPLAT_WIDEN` 3.5 px instead of 300, 150 and 125 px/s, and the sizes are the ones those speeds had.
- **Shared knobs:** `REACT_SIZE` scales every reaction at once. `JELLY_SPREAD` is the jelly's coupling as a share of `REACT`'s stiffness (2.33, so 1400 at 600), so a quicker or slower `REACT` spreads a poke the same way. `BULGE` (0.5) is how much stretching along one axis pulls the other in, for the popover's shape and the kicked item alike (it was `SQUISH_BULGE` and a `/ 2.0` by hand).
- **Splat timing:** a closing popover splatted when its bottom edge came within `SQUISH_LAND` (5 px) of its island's, which only peaks as the shape arrives for one pair of tempos. Now it looks ahead: it splats when the morph's bottom edge, stepped forward by `REACT.peak_time()`, will be within `SQUISH_LAND` (now 1 px). A quicker or slower `MORPH` or `REACT` still splats as it lands. *(Tried on the way: extrapolating the edge's speed. The morph slows down a lot as it arrives, so that splatted far too early, while the shape was still tall.)*
- **Pace:** a paced nudge divides its duration by `REACT.settle_time()`, how long a kick takes to stay within 5 % of its peak (196 ms now), instead of the hand-picked `NUDGE_LASTING` (190 ms). A different `REACT` keeps paced nudges right.

**Why:** each motion was tuned on its own, so there were five tempos (press 600, drag 220, nudge and jelly 600, squish 250, morph 380) and four damping ratios, and the amplitudes were launch speeds whose size only a comment gave. Next to each other they didn't feel like the same material. With a shared tempo, the whole bar's feel can be tried and changed in a handful of numbers.

**The feel: crisp.** With the language in place, four feels were built on branches and run side by side on the user's screen. The user picked crisp:

| Feel | REACT | HAND | MORPH | `REACT_SIZE` |
|---|---|---|---|---|
| reference: the old sizes on one shared tempo | 0.26 s / 0.85 | 0.32 s / 0.6 | 0.32 s / 0.9 | 1.0 |
| **crisp** (taken): quick and precise, barely any rebound, like iOS controls | 0.20 s / 0.92 | 0.26 s / 0.75 | 0.28 s / 0.95 | 0.8 |
| bouncy: playful rubber, one rebound of about 20 % | 0.32 s / 0.45 | 0.36 s / 0.4 | 0.34 s / 0.88 | 1.15 |
| liquid: soft and heavy, no rebound, the jelly spreading wider (`JELLY_SPREAD` 3.7) | 0.42 s / 1.0 | 0.45 s / 0.8 | 0.40 s / 1.0 | 1.2 |

Compared with before D-83, everything moves quicker and settles sooner (a kick peaks at about 31 ms and is back within 5 % in about 165 ms, against 192 ms for the old nudge), and every reaction goes 0.8 as far: a push 4 px, a bump 1.6 px. The squish and splat lost their slower spring (250 / 0.75), the press its springy one (600 / 0.5, 16 % rebound, now about 2 %), and the drag its soft one (220 / 0.7). Popovers open and close a little quicker. Even bouncy kept the morph near critical, because of D-44's reason against an underdamped morph.

**Alternatives:**
- keep a feel per motion and only retune them closer: it drifts apart again with the next motion, and there's no one place to change the feel.
- one feel for everything: a drag would follow the hand as quickly as a nudge comes back, and a morph would rebound like a press, which D-44 ruled out for the edges (the overshoot grows with the distance).

**Tests** compare against the language, not literal numbers: a push peaks within 20 % of `NUDGE_PUSH × REACT_SIZE`, and swings back past rest no more than `REACT`'s `overshoot()` says. So retuning the feels keeps them meaningful.

**Measured** (on the reference feel; the feels only change numbers. Asahi, release, GPU renderer, the user's config, both binaries from the same directory, 12 s after start, two runs each): binary 17,788,544 bytes before and after. RSS about 62 MB of anonymous and file pages either way (RssAnon 22.2 MB, RssFile 40.0 MB on both). Idle CPU 0–2 ticks per 10 s on both. No dependencies, nothing allocated. *Gotcha:* a binary run from tmpfs shows about 2 MB more RSS than the same one run from disk, so compare binaries from the same place.

## D-84: The dormer: actions widgets declare, found by typing

**Status:** accepted (2026-10-14).

**What:** a keybind opens the dormer, a search dialog that drops out of the top center of the screen into its upper middle. It lists **actions**, and typing narrows them down: up and down (or Tab) pick a row, Enter or a click runs it, and Esc, any other key the field doesn't take, or a click elsewhere closes it, like a popover.
- **Widgets declare their actions** with `Widget::actions()`, a list of `ui::action(title, message)` with `.subtitle`, `.keywords` and `.leading` (a small picture). Like `notices()`, the method is pure, and the host reads it, not the widget. It reads it only while the dormer is open: once when it opens, then after each of a widget's messages, so titles can follow state ("Mute" while sound plays, "Unmute" while muted). Running an action sends its message to its widget, as a click would, so an action that returns `open_popover()` opens that popover.
- **Every widget with a popover gets a row of its own**, titled with its name, that opens it. So the dormer is useful before any widget declares an action.
- **The dialog:** the host matches what's typed against titles, then keywords and subtitles: every character in order, scoring more for a start, a word start or a run, plus a little for recent picks. It shows the best eight, so nothing scrolls. With nothing typed, it lists the recent picks first, then the rest. The picks are kept in memory only, like the notification history.
- **Its island:** a host island at the top center: the notch's width when there is one, otherwise a dot under the top edge, attached to the top. It is only there while the dormer is. The panel uses the popover machinery, with a new `Placement::Centered { top }`: centered across the surface, its top a fifth of the way down the focused screen (from Hyprland's `monitors`). It moves in the motion language like any popover ([D-83](#d-83-one-motion-language)), with nothing of its own: every edge rides `MORPH`, so the shape glides down whole and never passes where it goes, the opening squish and the landing splat ride `REACT`, and the splat's look-ahead lands it against the top, not on the way up. *(Superseded by [D-86](#d-86-the-dormer-comes-out-as-a-drop): its island is a dot where it comes out, and it comes out as a drop.)* The surface grows to fit it, and the bar takes the keyboard for it, as it does for a popover. The dormer and popovers take turns: opening either closes the other.
- **The keybind** is a Hyprland custom event, `eave-dormer`: `bind = SUPER, SPACE, event, eave-dormer`, or `eave dormer` from anywhere, which dispatches that event. The bar's socket2 reader now runs all the time to catch it. Every other event stops in its filter, before `update`. *(Amended by [D-91](#d-91-shortcuts-plugins-name-them-users-rebind-them-hyprland-owns-the-global-ones): `eave-popover` and `eave-action` events too, and actions can have ids and keys.)*
- **`[[dormer]]` in config.toml** holds widgets that are only there for their actions. Their items aren't drawn and their popovers never open. eave-build compiles them in like any other.
- **The `apps` plugin** is the first of those. It turns each desktop entry into an action that launches it through Hyprland's `exec`, so the app isn't a child of the bar. `Terminal=true` entries need `terminal = "foot"`. It reads the entries off the UI thread, then again whenever an `applications` directory changes (inotify, on a thread of its own).
- **Built-in actions:** volume (mute, up, down, the mic, switch devices), media (play or pause, next, previous, follow another player), brightness (brighter, dimmer), workspaces (every window to switch to, every workspace, previous, next), keyboard (next layout, each other layout).

**Why:**
- **The host reads the actions; no plugin reads another's.** Plugins have no channel to each other, and the host owns every surface (D-11). The dormer as a plugin would need both. Declaring keeps one source of truth, as with `notices()` (D-47): the widget's state, not a copy the host keeps in step.
- **A keyboard door to every plugin.** Popovers are reached with the pointer. Muting, skipping a track or finding a window by name are quicker typed, and a widget gets this by listing what it can already do.
- **A launcher is just a plugin.** The roadmap's "launcher" needs nothing of its own: apps are actions too.
- **Only while open.** Nothing is asked of the widgets while the dormer is closed, so the bar at rest does no more work than before.
- **`actions()` sees no query.** A static list the host filters covers every built-in and the app launcher. A second reader (eave-claude handing actions to Claude as tools, say) can use the same list.

**Alternatives:**
- The dormer as a plugin: needs a way for plugins to read each other, and to own a surface.
- Actions over D-Bus, so other programs could add theirs: a serialization layer between widgets and their own messages, for no reader yet.
- A query-aware API (`actions(query)`, async results) for calculators and file search: more API before it's needed. It can be added beside this one.
- A separate layer surface for the dialog: the bar surface already grows for popovers, and morphing out of an island is the motion eave uses everywhere.
- Arrow keys through `ui` (an `on_key` primitive): the dormer is the host's own, so a host-only `Popover::on_key` does it without widening the vocabulary.

**Cost:** no new dependency in the host. The apps plugin uses rustix's inotify and tokio's channel, both already in the tree. One always-on socket2 subscription, shared with every Hyprland widget. The `apps` plugin keeps its desktop entries in memory (a few KB) and one blocked thread.

**Not yet:** network actions. The Wi-Fi state is read from iwd only while its popover is open (D-42), and the dormer closes that popover, so for now there is only the row that opens it. eave-notifications (do not disturb, clear) and eave-claude (focus a session) wait for this API on crates.io.

---

## D-85: Notification cards: fixed slots, pictures, choices, stacks

**Status:** accepted (2026-10-14). Amends [D-47](#d-47-notices-plugins-list-notifications-the-right-island-turns-into-them) and [D-62](#d-62-reading-and-writing-in-popovers-markdown-a-text-area-scrolling-wide-panels).

**What:** a notification card has one layout with fixed slots, which a notice fills or leaves out, and the panel shows the cards in a set way. Designed side by side with today's cards first (the artifact "eave Notification Cards").
- **Slots, top to bottom:** the countdown; where it's from (the app's icon, which may now be a `file://` URL, its name, the thread's name, step dots, and when it came, `Notice::time`); what it says (summary and body, with the picture 40 px beside them); a wide picture under them; the earlier notices of its thread, a line each; progress (`Notice::progress`, a meter and its percent); then what to answer.
- **The corner:** the time at rest, the close button under the pointer, swapped by an `Alternate` that the card's `Slot` tells about the pointer. Thin at rest, it grows under the hand (design rule 5).
- **Pictures:** wider than 4:3, the picture is drawn whole under the text, as wide as the card and at most 180 px tall, cropped from the middle past that (`Cover::wide`, made only for wide ones). Squarer ones stay 40 px beside the text: a large square avatar is still an avatar, so it's the shape that decides, not the size. A picture can come as pixels (`Notice::pixels`, eave-notify reading `image-data`), drawn off the UI thread like a file and kept by where its bytes are; the host holds the bytes while it keeps the picture, so no other picture can take that address.
- **Answers:** options (`Notice::choices`, one or several, with descriptions and a recommended one) as rows lit like a list row in use, with check boxes for several and "Send N"; then the reply field and up to three buttons sharing the card's width, the first filled. A reply with two buttons or fewer shares their row. The reply's send button shows only once there's text, and `Reply::sent` says Sent in the field's place. "Show all" opens a cut body in place.
- **Steps:** a notice can be question *n* of *m* (`Notice::step`), with Back. When the step changes, the old content slides away and the new one in, like tabs' pages (`steps.rs`); the host keeps the old notice for as long as that takes.
- **Stacks:** notices of the same widget, app and thread (`Notice::thread`) that come one after another share a card: the newest in full, up to three earlier ones a line each. The card's clock is its newest's, and when it runs out every notice on it expires; its close button dismisses them all.
- **Three at most:** critical cards come first, then the newest. Past three, the rest wait with their clocks stopped, under a line saying how many, which opens their popover.
- **Keyed cards:** the panel is a keyed column, so a card's state (what's typed into it) stays with it as cards come and go above it.
- **Over the bus:** eave-notify reads `image-data`, `value`, `transient`, `resident` and eave's own `x-eave-thread`, `x-eave-thread-name`, `x-eave-icon`, `x-eave-choices`, `x-eave-multiple` and `x-eave-step`, and advertises them. A single pick is the action with the option's key; several picks go back as the `EaveChoicesPicked` signal.

**Why:** apps send more than the cards showed: avatars as pixels, progress, screenshots. A burst of messages from one chat filled the screen edge, card after card. And a question with three options, or several, had to open the chat. Fixed slots keep "plugins describe, the host draws": a notice still says what it has, never where it goes.

**Alternatives:**
- the picture's size deciding (≥ 256 px wide): large square avatars would fill the card.
- stacking every notice of an app, not just one thread's: a chat's messages from two people would read as one conversation.
- grouping runs in the popover rather than by app: as soon as two apps interleave there's little to group (eave-notifications groups by app, then thread).
- actions right-aligned as before: one button looked lost, three cramped.
- the close button always there: one more thing at rest on every card.

**Measured** (CachyOS x86_64, release, the user's config, both renderers built in, run on tiny-skia on a session bus of its own, two alternating runs against main): binary +128 KiB (18.40 → 18.53 MiB); RSS unchanged within noise, ~23.4 MB idle and ~27.7 MB with one notification shown, against ~23.6 and ~27.3 on main. No dependency. The time in a card's corner moves on with a one-minute subscription, only while the panel shows a notice with a time.

---

## D-86: The dormer comes out as a drop

**Status:** accepted (2026-10-14). Supersedes how [D-84](#d-84-the-dormer-actions-widgets-declare-found-by-typing)'s island moves.

**What:** the dormer no longer comes out of the top as a notch-wide shape. It comes out as a drop, and what it comes out of squishes.
- **Its island is a dot** where it comes out. With a notch, that's the bottom middle of the notch as the bar draws it, grown (D-75). Without a notch, it's the bottom of the grown top edge (D-81). `island::Popover::drop` says how far down the surface that is.
- **Stops on the way:** the shape's edges ride `MORPH` from the dot through two stops: first a ball (`DROP_BALL`) swelling half out of where it comes out, then the same ball fallen `DROP_FALL` below it. After that, they head for the panel. They head on to the next stop once they're `DROP_WAYPOINT` (0.4) of the way to one, while the shape is still quick, so it never slows down on the way and moves as one motion. *(It was 0.7 at first: the shape all but stopped at each stop, and next to the other morphs it felt halting.)* Closing runs through the same stops backwards. The corners are a circle's while it's a ball, and blend into the panel's as it unfolds. The content waits for the unfolding, as it waits for any popover's morph.
- **The neck** (`neck.rs`) is the smooth union of the line along the bottom of where it comes out and the drawn shape: the polynomial smooth minimum of their distances, blending over `NECK_BLEND` (56 px; 36 at first, widened at the user's ask for a rounder start). Both are symmetric about the middle, and the field only grows away from it along a row. So each half-pixel row is one span, found by bisection, no wider than `NECK_SPREAD`, and the outline is the spans' ends. A drop just under the notch sits in a wide, smooth bell, about four and a half times its width at the root. The bell narrows as the drop falls, thins to a waist and pinches off (about half `NECK_BLEND` away), leaving a drip on the notch that draws back in. On the way back, it catches the drop again. The blend shrinks with the drop while it's smaller than its ball, so a dot has none. *(The first version bridged two circles with the classic metaball curves. The bridge was narrow from the start, so the ball seemed to slide out of a slot instead of growing out of the notch.)*
- **Bounces** that the user liked on workspace switches: a lurch on `REACT` (`DROP_BOUNCE`, 3 px), with the sides leaning and the side it goes toward bulging, on the jelly. It's paced like a nudge that says how long it lasts, to `DROP_BOUNCE_LASTING` (420 ms). *(At first it played at `REACT`'s own pace, about 200 ms, which the user found too fast.)*
  - Down, as the neck lets go of it.
  - Down again, as it falls into place as the panel. A look-ahead, as with the splat, makes it peak on arrival.
  - Up, as the neck catches it on the way back.

  The neck is drawn where the drop is, moved, but flows from where it comes out, which stays put.
- **The neck follows the notch.** `Zones` writes where it draws the middle of the notch's bottom (or the edge's) to a shared cell (`zones::Sag`), each time it draws, and the island reads it: the neck flows from there, and the drop's dot rests there. Zones draw before the dormer's island, so the two agree on every frame. *(At first the neck flowed from where the bottom rests: as the notch flattened on the drop's return, a 2 px strip of background showed between them.)*
- **What it comes out of squishes,** within what it's grown by, so it never gets smaller than the hardware's:
  - As the dormer opens, the notch pinches in from each side (up to `NOTCH_PINCH`, a little taller by `BULGE`).
  - As the drop lands back, the notch flattens (up to `NOTCH_FLATTEN`, a little wider).
  - Without a notch, the grown edge bumps down in a narrow bump where the drop leaves (`EDGE_PULL`, `EDGE_PULL_WIDTH`), and dents up in a wide one where it lands (`EDGE_DENT`, `EDGE_DENT_WIDTH`).
  - With nothing grown, nothing moves.

  Each rides `REACT`. The host numbers each kick (`zones::Kick`), as it numbers nudges. The notch reacts at the same moment the drop bounces: the island says when the neck lets go of the drop or catches it (`on_neck`), and the next frame squishes the notch. *(At first the notch pinched as soon as the dormer was asked to open, before the drop had come out, and flattened only once the drop had shrunk away into it. Both read as late or early next to the drop. The user asked for them to happen as the ball leaves and touches.)*
- The ball is squeezed by `DROP_SQUISH` as it sets off to unfold. The island's own splat is left out, since it vanishes as it lands.

**Why:** the notch-wide shape read as a copy of the notch sliding out of it, not as something coming out of it. A drop reads as made of the same black stuff as the notch, which is the point of drawing the notch at all. D-75 and D-81 grew the notch and the edge to have pixels to squish, and nothing squished them until now.

**Alternatives:**
- a clean ball, without the neck: cheaper, but it pops out instead of pulling away.
- tracing the union's outline with marching squares: no need, since each row is one span.
- squishing the notch from the island, drawing a notch of its own while the dormer is there: two shapes in one island, and the notch drawn twice.
- one waypoint (straight from the dot to a fallen ball): the ball left the notch as it swelled, so the neck never had a ball to hold.

**Cost:** no dependency. Built and drawn only while it moves:
- the neck: about 150 rows of 18 field samples and one path a frame, while the drop is within two blends of the notch;
- the notch or edge path: rebuilt each frame while it squishes, cached again at rest;
- the stops: no allocation.

**Measured** (Asahi aarch64, release, the user's config, the default GPU renderer, both binaries from the same directory, 12 s after start, two alternating runs): binary 18,050,688 bytes before and after. RSS 74.6 and 76.5 MB before, 74.7 and 74.9 MB after: unchanged within noise. Idle CPU unchanged within noise: 130 and 48 ticks over 10 s before, 63 and 47 after (something in the config animates at rest on both). Only aarch64 was built here: no target for x86_64 is installed, and nothing in this change is arch-specific.

---

## D-87: Every notification has an icon; workspaces' app icons are optional

**Status:** accepted (2026-10-14). Amends [D-38](#d-38-app-icons-found-and-drawn-by-the-host) and [D-85](#d-85-notification-cards-fixed-slots-pictures-choices-stacks).

**What:**
- `Notice::icon(Icon)` (eave-api 0.1.7) gives a notice a status symbol as its icon, drawn in the text's tone. It wins over `app_icon`.
- The card's icon is the symbol, else the app's icon, else a bell: a notice with no app, or whose app's icon the host looked for and didn't find, gets the bell instead of the id's first letter. One still loading keeps its place empty. Elsewhere `app_icon` still falls back to the letter: in a workspace, the letter tells apps apart.
- eave-notifications maps icon names from the freedesktop naming spec that are about the device to eave's symbols: `battery-*` (filled by `caution`, `low`, `good`, `full`, charging for `charging`/`charged`), `audio-volume-*`, microphones, `network-wireless-*`, `network-wired`, brightness, media controls, headphones, speakers, displays, `dialog-information` and bells, with or without `-symbolic`. The battery plugin keeps sending `battery-caution` over the bus, so mako and dunst still find it in their theme.
- The workspaces widget shows only names unless `icons = true`.

**Why:** the battery's notification showed a "B": the name it sends is in Adwaita and Breeze, not in `hicolor`, which is all a system without a GTK theme set has. A status symbol is what the bar draws for the battery anyway, so the card matches the bar. The mapping lives in the notification engine, not the host, because it's about a bus convention, and the host only learns of symbols (D-13). As for workspaces, names alone are what most bars show, and quieter; the icons stay one key away.

**Alternatives:**
- falling back to `Adwaita` in the host's lookup: color, raster art for device states that eave draws as symbols, and only if Adwaita is installed.
- a `-symbolic` lookup tinted like `symbol`: still depends on the theme having it.
- the battery plugin sending an `x-eave-*` hint: other apps' device notifications (a volume OSD, a VPN) would still show letters.


---

## D-88: The bar shows on hover or hides over fullscreen windows, and notifications wait

**Status:** accepted (2026-10-14). Amends roadmap step 0 (the bar always on the `top` layer, always keeping its exclusive zone) and [D-47](#d-47-notices-plugins-list-notifications-the-right-island-turns-into-them) (notices show as soon as they come).

**What:**
- Two new tables in config.toml, one per thing they're about, in the style of `notch`, `screen` and `islands`:

  ```toml
  bar = { show = "always", fullscreen = "hide", peek = true }
  notifications = { fullscreen = "critical" }
  ```

  - `bar.show`, outside fullscreen, is `"always"` or `"hover"`.
  - `bar.fullscreen` is `"hide"`, `"hover"` or `"show"`.
  - `bar.peek` brings a bar shown on hover out for `REVEAL_SHOWN` on a workspace switch.
  - `notifications.fullscreen` is `"show"`, `"critical"` or `"hold"`.
- **Fullscreen** means a client on the focused monitor's active workspace whose Hyprland fullscreen mode has bit 2. Bit 1 is maximized, which respects the exclusive zone. Hyprland's `fullscreen>>` event fires for both and only for the focused workspace, so the host doesn't trust it on its own. On that event, a workspace or monitor change, and a window opening, closing or moving, it queries `monitors` and `clients` once (`Message::CheckFullscreen`). eave-hyprland gains `Event::Fullscreen`, `Client::fullscreen` with `Client::covers()`, and `Monitor::active_workspace`.
- **Out or hidden.** The bar is out when its mode is `show`, or `hover` and the pointer is on it, or while it peeks, or while a popover, the notification panel or the dormer is on it, until they have folded back.
  - Hidden, the islands are drawn lifted off the screen by a `Lift` widget on `MORPH`. The layout doesn't move.
  - The input region becomes a `REVEAL_EDGE` (2 px) strip on hover, else nothing.
  - On hover, the pointer gate (`popover_event`) stays open, so `CursorEntered` and `CursorLeft` arrive. Leaving hides it after `HIDE_AFTER` (400 ms).
- **Exclusive zone.** It is 0 when `bar.show = "hover"`, even while the bar is out, so windows never move as it comes and goes. In fullscreen it doesn't matter.
- **Layer.** The surfaces go on `overlay` over a fullscreen window while the bar is out, or while `bar.fullscreen = "hover"` waits for the pointer. Otherwise they stay on `top`.
  - Going back to `top` waits for the slide away (`lift::slide_time`, the time `MORPH` takes to settle from a step).
  - Hidden on purpose in fullscreen, the bar is under the window, not just transparent above it.
- **Peeks** come from a workspace switch (with `peek = true`) and from any nudge, and only for a bar shown on hover. With `fullscreen = "hide"`, the bar stays hidden for those, and comes out only for a notification or the dormer.
- **Held notifications.** In fullscreen, `Bar::held` keeps them out of `cards()`. A panel opens only for those that aren't held. A held notice keeps its place in `toasts`, its clock stopped, since only shown cards run. When fullscreen ends, it shows with its whole time left. Going fullscreen with uncritical ones on screen folds them away until it ends.

**Why:** the bar was on the `top` layer, which Hyprland draws under fullscreen windows. Playing a video fullscreen hid the bar, and every notification with it, since they grow out of the bar's own surface (D-28, D-47). They even ran out there, unseen. Putting notifications on `overlay` all the time would have drawn every one over a film. So it's a setting, and the default lets only critical ones through and keeps the rest for later, the way phones do. The bar's own visibility was the same problem from the other side: once there's a mode that hides it over fullscreen, hiding it on hover elsewhere is the same machinery.

The settings are grouped by the thing they're about, not by situation, because each key then reads on its own (`bar.fullscreen = "hover"`). The vision rules out a config language, and this needs no conditions. Timings stay in `theme.rs`, like every other motion.

**Alternatives:**
- **Always `overlay` when fullscreen.** A transparent overlay surface over a fullscreen video can keep Hyprland from scanning the video out directly, which costs power for nothing while the bar is hidden.
- **A separate surface for notifications.** Rejected in D-28 for popovers; the panel is the right island's morph.
- **Trusting `fullscreen>>1` alone.** That flags maximized windows, which don't cover the bar.
- **Polling `activeworkspace.hasfullscreen`.** There's an event, so no polling (AGENTS.md). `hasfullscreen` is also true for maximized windows.
- **Changing the exclusive zone as the bar comes and goes on hover.** Windows would jump every time the pointer touched the top.
- **Settings grouped by situation** (`fullscreen = { bar = "hover", notifications = "critical" }`): one table would mix two subjects, and the normal case would need a table of its own.

**Cost:** no dependency. While the bar is shown on hover, every pointer event on the bar's surface runs the subscription's filter, which returns nothing except for enter and leave. One `clients` and one `monitors` query per window or workspace event.

**Measured** (Asahi aarch64, release, the user's config with every plugin, the default GPU renderer, each alone on a session bus of its own, 12 s after start, two alternating runs): binary 19,427,344 bytes before and after (stripped, page-aligned). RSS 77.8 and 77.8 MB on main, 77.6 and 77.5 MB on this branch, 77.7 and 77.7 MB with `bar.show = "hover"`: unchanged within noise. Idle CPU 1, 1, 1, 1, 1 and 0 ticks over 10 s. Only aarch64 was built: nothing here is arch-specific.

---

## D-89: A solid bar, with islands that flow out of it

**Status:** accepted (2026-10-14). Amends [D-24](#d-24-floating-islands-on-one-transparent-surface): a full-width strip is optional again. Builds on [D-81](#d-81-the-top-edge-can-be-grown-too) and [D-72](#d-72-the-bar-keeps-off-what-the-bezel-hides-and-hangs-from-the-top-like-the-notch).

**What:**
- **Settings.** `bar.style = "solid"` draws the whole bar black. `"islands"`, the default, keeps the floating islands. `bar.corners` is `"round"` (the default) or `"square"`: whether a solid bar's lower ends curve into the screen's sides like its top corners, or meet them square.
- **The bar is the grown edge, made thicker.** `Zones::edge` takes `grow + EDGE_GAP + island_height` (`band_bottom`), which reaches the islands' bottom, so the islands rest flush with the bar's bottom. `edge_outline` already rounds the band's lower ends by the screen's corners, or leaves them square. `Zones::solid` stops drawing the notch, which is under the bar now. The dormer's kick then always dips and bumps the bar's bottom, as it does the edge without a notch.
- **Islands stay islands.** They are the bar's black, so a resting one can't be seen, but its items, hover, morphs, popovers, nudges and drags are untouched. Each gets `Island::band(Some(band_bottom))`, the islands' own corner shape as its shoulders, and a top bleed up into the bar. A dragged or jellied island then never comes apart from it.
- **Flowing out.** `squircle::flowing` is `hanging` with the shoulders at the bar's bottom instead of the surface's top, and with one change. `hanging` gives the lower corner its share of the side below the edge first, and the shoulder gets what's left. `flowing` shares that side between them in proportion. So as a popover starts to grow below the bar, the shoulder and the corner under it grow together from nothing, and the outline is an S from the first pixel, never a corner meeting the bar's flat bottom at a cusp. Once deep enough, each is what it wants. The frame has room for a shoulder as round as the island: `room.x` grows to its reach. The shape is drawn again when the bar's bottom moves on it (`shape_band`).
- **The dormer** drops out of the bar's bottom (`band_bottom`), not the notch's.
- **What shows differently.** A nudge's or a drag's downward bump bulges the bar's bottom. An upward one, and the workspace push sideways, happen in black and can't be seen.
- **The rounded ends hang below the bar**, by the screen corners' reach (about 64 px on a MacBook). The surface is made tall enough to draw them whole (`Bar::surface_height`). Only the bar's strip takes the pointer, so they lie over the windows below like the drag room does (D-45). Square ends need no more room.
- **The exclusive zone** is unchanged: the bar ends above `RESERVED_ROOM`. A solid bar hidden on hover (D-88) slides away whole, since `Lift` lifts the zones, edge included.

**Why:** the user wanted a classic full-width bar without giving up the islands' behavior. The pieces were already there: D-81's grown edge is a full-width black band that rounds into the screen's corners, and D-72's `hanging` draws concave shoulders at any height. Keeping the islands as the things that morph means a popover is still the island growing, now out of the bar instead of out of nothing.

**Alternatives:**
- **A separate full-width rectangle under the islands, with no shoulders.** A popover would meet the bar's bottom at a square inner corner, which is what the user asked to avoid.
- **Shoulders from `hanging` as it is.** The lower corner takes its share first, so for the first 20-odd pixels of a growing popover there is a cusp where the corner meets the bar, and only then does the shoulder start.
- **The neck from D-86** (a smooth union, row by row). Made for a ball leaving a point, and capped in width. A popover is a box as wide as it likes.

**Cost:** no dependency. The bar is the edge's cached path, so it's drawn once until it changes. An island's shape is rebuilt when the bar's bottom moves on it, which is while it morphs, as it already was.

**Measured** (Asahi aarch64, release, the user's config with every plugin, the default GPU renderer, each alone on a session bus of its own, 12 s after start, two alternating runs): binary 19,427,344 bytes before and after. RSS 77.6 and 77.5 MB before, 77.7 and 77.6 MB after with islands, 77.5 and 77.7 MB solid: unchanged within noise. Idle CPU 9 and 8 ticks over 10 s before, 7 and 9 after, 8 and 10 solid. Something in the config animates at rest during these runs (a playing track), for all three alike.

---

## D-90: Widgets' dialogs: they come out of what opened them

**Status:** accepted (2026-10-14).

**What:** a widget can have a dialog: a panel in the middle of the screen, for what needs more room than a popover. eave-claude's chat is the first.
- **The API.** `Widget::dialog()` returns its `Panel`, like `popover()`, and `Panel::on_open`/`on_close` work the same. `Effect::open_dialog()` and `close_dialog()` ask for it. A click never opens it. Returning `None` while it's open closes it. To keep `Effect::into_parts` the same shape for the plugins that test with it, the host reads the request through a hidden `Effect::dialog_request()`.
- **In the middle of the screen, and always on it.** It's `DIALOG_WIDTH` (640) wide, and centered on the focused screen both ways (`Placement::Middle`), never over the bar. A `scroll` in it grows to `DIALOG_SCROLL_MAX_HEIGHT` (600) before it scrolls, less on a short screen: the screen's height less `DIALOG_CHROME` (260) for the rows around the scroll and a margin, so the whole dialog fits (`kit::dialog`). A dialog that grows re-centers, springing there like any panel that changes. A centered panel takes no room in its island's row, so the islands beside it stay put. A dialog takes the keyboard while it's open, and closes on what closes the dormer: Esc, a key its field doesn't keep, or a click elsewhere. It also closes when its widget asks. A dialog, the dormer and popovers take turns.
- **What was acted on turns into it.** The host notes what was last touched, and when (`Touched`): an item (`Touched`), a message from a popover's content or a notification card (`Acted`), or a dormer pick. A dialog asked for within `ORIGIN_WINDOW` (1 s) of that comes out of it:
  - **An island:** the dialog comes out of the island's shape as it's drawn at that moment, and the island stays on the bar. Every island puts its drawn shape, popover included, in a cell of its own (`Island::shape`). The dialog, drawn by the island in the middle, starts as that shape and springs on to its panel (`Popover::home`). If the popover was open, it's gone at once, under the dialog's shape, and the island is back to its items. Closing springs the dialog back onto the island, wherever it's drawn by then, with the popover's splat, and it's gone.
  - **A notification:** the same, from the island the notifications show in. That island is back meanwhile, and so are its notifications. The click goes over the bus to eave-notifications, then to the widget that sent it, which asks for its dialog. The time window covers that round trip.
  - **The dormer:** a pick that asks for a dialog turns the dormer into it. The island in the middle takes the dialog's key, the shape springs on to the new panel, the new content fades in, and it goes back into the notch.
  - **Anything later or unprompted:** the island in the middle drops it out of the notch, or the top edge, as it drops the dormer (D-86). The same island holds both (`center_view`).
  - **Its own popover, open** *(amended 2026-10-15)*: the dialog always comes out of it, whatever was touched last and however long ago, and a popover row's shortcut counts as acting on the popover, like a click on the row. Before, the shortcuts weren't counted, so pressing `1` in eave-claude's popover depended on the clock: within a second of clicking the item the chat came out of the popover, after that it dropped out of the notch like the dormer. The user saw it as random. A dialog that takes in another one, or one still going back, now also goes back to where the new one came from. One out of a popover while the dormer still folds back starts at the popover's shape, not the dormer's.
**Why:** a chat in a 480 px popover hung from a small island reads as squeezed, and covers the bar's side. The middle of the screen is where the dormer already is. What you clicked turning into it keeps the cause and the effect one motion, the way a popover grows out of its island. A dialog out of the notch would look like the dormer for no reason.

**Alternatives:**
- **the island turning into the dialog** (the second version: the island acted on took the dialog's popover key, so its popover's shape sprang straight on to the dialog): one motion, but the island was gone from the bar for as long as the dialog was open. The user wanted it back. The dialog's top was also a fifth of the way down, as the dormer's is, and a chat as tall as it can get ran off the bottom of the screen.
- **dropping out of the popover as it folds back** (the first version, with the drop's neck flowing from the island's bottom as drawn): the user tried it and found that the popover went back into its island first, and only then did something come out. It read as two motions where they had asked for one: the popover becoming the dialog.
- **the popover staying open behind:** two panels at once compete, and Esc would have to close one and not the other.
- **always out of the notch:** one path, but it breaks the link with what was clicked.
- **a host-side link from the notification to the dialog** (the engine passing on where the click was): the click goes through two plugins and the session bus, so the host can't see the link without a protocol of its own. The time window is enough, and harmless when it misses: then the dialog comes out of the top.
- **a separate layer surface for the dialog:** the same reasons as for popovers (D-28). It's drawn on the bar surface, which grows to fit, with an input region.
- **changing `Effect::into_parts`** to return a struct: cleaner, but it breaks plugins' tests that read the tuple, in a patch release.

**Measured** (Asahi aarch64, release, the user's config with eave-claude and eave-notifications, both renderers built in, run on tiny-skia on a session bus of its own, three alternating runs against main): binary unchanged to the byte at 19,427,344 B (the code fits in the padding of the page-aligned sections); RSS ~23.3 MB both, within noise; idle CPU the same on both (~3.7%, from the mascot's question loop with a session asking). RSS and CPU were measured with the first version, which drew more (a neck out of any island); the later ones take code away from it. The third (centered, out of the island's shape) against the second, same setup: binary unchanged at 19,427,344 B; RSS ~22.8 MB both; idle CPU the same. It adds one shared cell per island, written as it's drawn. Nothing runs while no dialog is open. No dependency.

---

## D-91: Shortcuts: plugins name them, users rebind them, Hyprland owns the global ones

**Status:** accepted (2026-10-14). Amends [D-36](#d-36-popovers-take-the-keyboard-while-open), [D-41](#d-41-text-fields-in-popovers) and [D-84](#d-84-the-dormer-actions-widgets-declare-found-by-typing).

**What:** keys come in three kinds, each with its own owner.
- **Popover keys** press a popover's rows while it's open. eave-api 0.1.8 adds `ui::Key` (a character, Enter, Space, Tab, the arrows; no Escape), `ui::Shortcut` built with `shortcut(key)` and `.ctrl()`, `.alt()`, `.shift()`, `.logo()`, `list_item(..).shortcut(id, key)` and a `keys(shortcut)` node that only draws one.
  - The host draws a row's key as keycaps at its end, after its trailing node, and binds it to the row's `on_press`. Popovers only have the keyboard while open (D-36), so that's all "only when focused" takes.
  - `popover_view` resolves and then collects the keys of the panel's rows, skipping rows inside `tabs` (only one page shows), and gives the island an `on_key` that matches the key and exactly the modifiers held. A letter matches in either case. A dialog's rows ([D-90](#d-90-widgets-dialogs-they-come-out-of-what-opened-them)) get theirs the same way (`panel_keys`, `rows_on_key`).
  - A key with no Ctrl, Alt or Super gives way while a text field in the panel has the focus, so typing `1` in a field types it. The island now finds out whether a field has the focus before asking `on_key`, and tells it.
  - A modifier pressed alone no longer closes a popover: it may start `Ctrl+K`.
  - The keys are the widget's alone, so two plugins never clash.
- **Dormer keys** run an action while the dormer is open: `ui::action(..).id("mute").shortcut(shortcut('m').ctrl())`. The keycaps show on its row, and the key works whether the row shows or not.
  - It needs Ctrl, Alt or Super, since the dormer's field takes the keys that type, and an id. Otherwise the host drops it and says so.
  - Every widget's actions share the dormer, so keys can clash. The user's keys come first, then the plugins', each in config.toml's order; a key that loses is dropped, and the host says so once.
- **Global keys** open a popover or run an action from anywhere. eave takes none of them. `eave popover <widget>` and `eave action <widget> <id>` send the custom events `eave-popover <widget>` and `eave-action <widget> <id>`, like `eave dormer`, and the user binds them in hyprland.conf (`bind = SUPER, C, event, eave-popover claude`) or runs the commands from any other shortcut tool.
  - `<widget>` is the name `widget = "…"` gives, and the first such widget in config.toml is the one.
  - `eave popover` opens the popover or closes it, like a click on the item. `eave action` runs the action as the dormer would, so one that opens its popover does that too.
  - The built-in widgets give their fixed actions ids: `volume` `mute`, `up`, `down`, `mic-mute`; `media` `play-pause`, `next`, `previous`; `brightness` `up`, `down`; `workspaces` `previous`, `next`; `keyboard` `next-layout`; `network` `wifi-on`, `wifi-off`. Actions that come and go, like a window to switch to, have none. They set no default keys.
- **The user rebinds** popover and dormer keys in config.toml: `keys = { chat-1 = "alt+1", chat-10 = false }` in the widget's entry, by id. Rows and actions share the ids. A key is modifiers (`ctrl`, `alt`, `shift`, `super`) and one key, joined with `+`; `false` takes the key away. The host takes `keys` out like `width_group`, so the widget never sees it, and a change reloads live without starting the widget again. Within a popover, a key the user set wins over a plugin's, then the first row wins.
- The first user is eave-claude: the first ten sessions get `chat-1` to `chat-10`, keyed `1` to `9` and `0`.

**Why:**
- The plugin names the key once, and the host shows it, binds it and lets the user change it, so the hint can't say one key while another works. Plugins never see key events, which keeps the vocabulary closed (D-13). Esc stays the host's: it always closes.
- Ids make keys the user's: a default that clashes with their habits is one line away from another key, and the plugin needn't know.
- Clashes only happen where plugins share keys. In a popover only its widget's rows are there, so first-wins is enough; the dormer gathers everyone's actions, so config.toml's order decides, the same order the user already arranges.
- Global keys belong to the compositor. Hyprland already knows every bind, warns about clashes and is where the user looks for them. Installing a plugin should never take a key across the whole system.

**Alternatives:**
- a `Panel::on_key` that hands plugins raw keys: they'd draw their own hints, or none, and every plugin would match keys its own way.
- `iced::keyboard::Key` in the API: ties eave-api to iced's types and lets plugins bind keys the host can't draw.
- keys on buttons too: no use for it yet; a row is what the keyboard picks among. *(Superseded by [D-100](#d-100-buttons-take-shortcuts-too): eave-claude's chat has a use.)*
- unnamed shortcuts: they can't be rebound.
- eave adding its global binds to Hyprland over IPC (`keyword bind`), checked against `binds`: it writes into the user's compositor config behind their back, and `hyprctl reload` wipes them, so eave would have to put them back.
- `hyprland-global-shortcuts-v1`: one more protocol for the same result as a custom event, and the user still binds the keys in hyprland.conf.
- plugins giving global keys by default: a plugin would take a key everywhere just by being installed.

**Cost:** no dependency. Popover keys are resolved only while one is open, and dormer keys when it asks the widgets, so the bar at rest does nothing more. One small list per open popover or dormer.

---

## D-92: Light and dark: the palette mirrors

**Status:** accepted (2026-10-14). Amends [D-61](#d-61-black-and-white-and-color-only-as-a-signal) and [D-65](#d-65-code-is-content-colored-code-diffs-and-timelines), which now describe the dark scheme.

**What:** `theme` in config.toml is `"system"` (the default), `"light"` or `"dark"`, and applies live.
- **Two palettes.** Every color in `theme.rs` moved from a `const` into a `Palette`: `DARK` holds the values D-61 and D-65 set, unchanged, and `LIGHT` is its mirror. White islands; ink black at 88 % (`Normal`) and 50 % (`Dim`), written as the grey it makes on white; `Accent` black; every white overlay black at about the same alpha (fills a little lighter, since black on white reads stronger); the accent fill black with white on it. Signals and code hues go deeper so they read on white: good `#248A3D`, warning `#C25E00`, critical `#D70015`. A cover's color is lifted to a lightness of 0.62–0.74 on black, and held to 0.34–0.44 on white, so the white on a prominent button still reads. Sizes, springs and durations stay `const`s.
- **How it reaches the pixels.** The bar sets iced's theme (`.theme(app::theme)`): `Theme::Dark` or `Theme::Light`. Every widget that draws itself, and every style function, already gets that theme and now asks `Palette::of(theme)`. What kit builds at view time (text colors, the background a `Clip` fades into) takes the palette from `Ctx`, which `app::view` fills from the bar's scheme, next to the icons. Caches that fill with the island's color (the island's shape, the notch's and edge's band, a still icon with a slash) count the scheme in what they were drawn from, so a switch redraws them.
- **The notch and the grown top edge stay black.** They stand for the hardware around the picture, which is black whatever the theme, like a light menu bar's notch on a Mac. A solid bar's band ([D-89](#d-89-a-solid-bar-with-islands-that-flow-out-of-it)) is the islands' color. `eave calibrate` stays black too: it measures the screen against black. The dormer, and a dialog that drops from the top, come out of that black: the drop is black while it's a ball on its neck, turns the islands' color as it unfolds into the panel, and goes black again as it folds back up, before it rises into the notch (`Popover::drop_color`, following the drop's roundness).
- **`"system"`** follows the settings portal's `org.freedesktop.appearance` `color-scheme`: read once, then on every `SettingChanged`, with no polling (`eave_dbus::portal`, new in eave-dbus). Light only when the desktop prefers light: no preference, or no portal, is dark, the bar's own look. The host depends on eave-dbus for this; it was already in the build through the plugins that use D-Bus.
- **The `appearance` plugin** switches the desktop's mode, not the bar's: a ringed sun or a moon (`Icon::Appearance`, new in eave-api; the sun is drawn apart from brightness's), a click to switch, a popover with both, and `light` and `dark` actions. It sets GNOME's `color-scheme` key through dconf's own D-Bus writer (`ca.desrt.dconf.Writer.Change`, a one-key changeset serialized as GVariant by hand, checked byte for byte against GLib's), which portal backends hand to every app that asks. So the bar (with `"system"`), GTK and libadwaita apps all follow one switch. It reads the mode back from the portal, so a change made elsewhere shows too, and nudges up for light, down for dark.

**Why:**
- Some desktops are light, and a black bar on them is the one dark thing on screen. Mirroring keeps every rule of D-61 (brightness for hierarchy, color only as a signal or content) with the roles of black and white swapped, so plugins change nothing: they still name a `Tone`.
- iced already threads its theme through every draw and style call, and kit already threads a context: two existing paths, no global state.
- `"system"` as the default makes the bar match the desktop out of the box. With no preference it stays dark, which is what it was.
- The switch belongs to the desktop. A plugin that only flipped the bar would leave apps the other way; flipping the desktop's setting flips both.

**Alternatives:**
- A new eave-api request (`Effect::theme`) for a plugin to set the bar's theme, written to config.toml like `eave calibrate` does. It works without dconf, but switches only the bar, and grows the API for one plugin. Rejected by the user in favor of the desktop's switch.
- A global (an atomic scheme read by `theme::palette()`). Fewer signatures change, but it's state the view doesn't own, and iced's theme already carries it.
- A custom iced `Theme` holding the whole palette. More than two values need, and every widget would downcast it.
- Encoding the changeset with zvariant's `gvariant` feature: a feature flag and more code for 30 lines of bytes.
- The `gsettings` binary: shelling out (D-23).

**What we give up:** the appearance plugin needs dconf on the session bus, which bends [D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel). It's opt-in, nothing else depends on it, and without dconf it hides. The bar following the desktop needs a portal backend that has the Settings interface (xdg-desktop-portal-gtk or -gnome; -hyprland has none and falls back to them); without one, `"system"` is dark.

**Measured** (Asahi, release, two clocks on tiny-skia, alternating runs in one session): the binary is 18,181,760 → 18,968,192 bytes (+0.79 MB, text +816 KB). The palettes themselves add 7 KB; the rest is zbus and zvariant code instantiated anew, about half for the host's portal subscription (connection setup, match rules, reading a `Value`) and half for the appearance plugin. RSS about 17.9 → 18.3–18.7 MB in the quiet runs (some runs of both landed near 34 MB, noise), from the session bus connection `"system"` opens. Idle CPU 0–1 ticks in 10 s on both.

---

## D-93: `update` reinstalls eave-build before the bar, not after

*Amended by [D-97](#d-97-eave-build-check-and-the-update-units): the handover passes `--summary` on, and what had moved in `EAVE_BUILD_MOVED`.*

**Decision:** when `eave-build update` moves eave, it reinstalls eave-build from the new checkout right away, before reading plugins, and hands the rest of the run to the new binary (`exec`, with `EAVE_BUILD_HANDED_OVER` set so it doesn't do it again). The new binary fetches the plugins, builds and installs the bar, rewrites the units and reloads systemd. A marker (`$XDG_DATA_HOME/eave/eave-build-outdated`) is written before the install and removed after it succeeds, so a failed install is retried by the next `update` even though eave no longer moves. Packaged and non-cargo eave-builds are unchanged ([D-54](#d-54-eave-on-the-aur-shipping-eave-build-from-release-tags)).

**Why:** D-31 updated eave-build after the bar, so a failure there would never leave the bar half-updated. But the build recipe lives in eave-build: when eave vendored a patched layershellev (D-79), the old eave-build generated a manifest without that patch, cargo used crates.io's layershellev, and the build failed before the self-update could run. Every commit that changes both the recipe and code depending on it would break `update` the same way. Building the bar with the eave-build of the same checkout makes the recipe and the code always match.

**Alternatives:**
- Keep the order and have users run `cargo install` by hand when a build fails: the failure says nothing about why.
- Reinstall eave-build on every `update`: no stuck state without a marker, but a cargo build of eave-build every time, for nothing most times.
- Retry the bar build after updating eave-build when the first build fails: two long builds, and a failure for another reason looks the same.

**What we give up:** if the new eave-build fails to install, the bar isn't rebuilt at all, while eave's checkout has already moved. A plain `eave-build` (the rebuild on save) then still builds with the old binary until `update` succeeds.

---

## D-94: Screen settings per monitor

**Status:** accepted (2026-10-14). Amends [D-71](#d-71-eave-calibrate-measures-the-screens-corners-and-notch): where `eave calibrate` saves.

**Problem:** `notch`, `screen` and `islands` describe one screen, but one config.toml often serves several: a dotfiles repository shared by a MacBook and a desktop, or a laptop that's docked. Each panel has its own corners, bezel and notch, and calibrating one overwrote the other.

**What:**
- `[monitor."<name>"]` tables hold `notch`, `screen` and `islands` for the monitors they match. The top-level keys stay, as the default for every other monitor.
- **Matching:** a table named after the connector (`eDP-1`) wins. Otherwise the table whose name Hyprland's `description` ("make model serial") starts with, the longest one. `eave_hyprland::types::Monitor` gained `description`, `make` and `model` to read them.
- **Merging:** each key a table has replaces the top-level one whole. A key it leaves out is the top level's.
- **Which monitor:** the one the bar opened on, the focused one at its first look into the screen (`Bar::output`). A reload looks that monitor up by name, not whichever has focus then. Resolving the screen now always asks Hyprland for the monitor, one query per change of these settings, since matching needs it.
- **`eave calibrate`** starts from the setting that applies to the focused monitor. It saves into the table that matches it, or a new `[monitor."<make> <model>"]` (the connector, when Hyprland knows neither), and leaves the top level alone.

**Why:**
- **The description, and a prefix of it.** Connector names follow the port, not the monitor: the same external screen is `DP-1` on one port or dock and `DP-3` on another. The description follows the monitor, and Hyprland's own `desc:` monitor rules match it the same way. Leaving the serial out covers every unit of a model, which has the same panel. The connector still works, and wins, for the built-in panel (`eDP-1`), which never moves.
- **Whole keys, not a deep merge.** A `screen` that's half one monitor's and half the top level's is hard to read off the file, and calibrate measures all of `notch` and `screen` together anyway.
- **Calibrate per monitor.** A measurement is of one panel. Writing it to the top level would make it every other monitor's too.

**Alternatives:**
- **`[[monitor]]` entries with a `match = "…"` key:** room for more ways to match, but longer to write, and two entries could match equally.
- **Matching on connector names only:** simplest, but an external monitor's settings would follow the port.
- **A separate state file per monitor**, written by calibrate: config.toml is the only user file.

**What we give up:**
- The bar doesn't follow a monitor plugged in or out while it runs. It stays where it opened, with that monitor's settings, until it restarts. Following hotplug comes with one bar per monitor ([roadmap.md](roadmap.md)).
- Top-level `notch` and `screen` written by an older `eave calibrate` stay where they are. The monitor's table now overrides them, and they still apply to other monitors.

**Cost:** no dependency. One more Hyprland query when config.toml has no measurements, only when these settings change.

---

## D-95: Islands follow a measured screen, nearly square and very round ones too

**Status:** accepted (2026-10-14). Amends [D-73](#d-73-islands-take-their-corners-from-the-screen).

**Problem:** a monitor calibrated to `corners = 1` still had round 14 px islands, nothing like its screen. D-73 took the island's corner as the screen's minus the 6 px gap, and below zero fell back to the theme's 14 px, the same as for a screen nobody measured. And a screen corner much rounder than an island can follow put the island into it: an island's corner reaches at most halfway down it, so with a 30 px island the corner is capped at 15 px. Against a 39 px screen corner at scale 1, the gap was 0 to 1.5 px instead of 6.

**What:**
- **Measured or not.** `screen` in config.toml (top level or the monitor's table, D-94) means the screen is measured; `Config::screen` is `None` without it. Only an unmeasured screen gives islands the theme's corners.
- **Barely round on a square screen.** On a measured screen the concentric radius is the screen's minus the gap, but at least `MIN_ISLAND_RADIUS` (4 px, with the theme's 0.6 smoothing). That's the corner facing the screen's, and the "auto" `islands` corners when there's no notch to take them from: a screen with corners of 10 px or less has islands with a hint of a squircle. Fully square ones looked wrong, like a bar with no shape of its own. `islands = { radius = … }` still sets them, 0 included.
- **In from a corner too round to follow.** The first island on the left and the last on the right move in (`Screen::shift`, more padding on that side) until no point of their outline comes closer to the screen's than a quarter pixel short of the gap. Their outlines are flattened (`squircle::Polygons`, moved from the calibrator) and the shift found by bisection, once per setting. An island attached to that side doesn't move; it's flush by design (D-70).

**How precise the even gap is:** measured with the island's and the screen's outlines flattened to 0.005 px.
- **Circles** (`smoothing = 0`): exactly the gap. The arcs share a center.
- **Squircles:** the arcs still share a center, but the easing curves on either side of them don't stay parallel, since a Figma squircle one gap in isn't a Figma squircle with the same smoothing. The gap dips to 5.91 px at smoothing 0.65 and 5.80 at 1.0. That's under a fifth of a pixel, so it's kept rather than drawing the island's corner as a true offset curve, which every morph and popover would have to carry.
- **Squeezed into the island's height:** 5.77 px for a 20 px screen corner on a 30 px island, still within the quarter pixel. From 21 px (logical) on, the island moves in: 0.6 px for 21, 5 for 25, 11 for 30, 22 for 39.

**Alternatives:**
- **The theme's corners below the gap, as D-73 had it:** a square screen with round islands is what started this.
- **Square, as the concentric radius says:** tried first. It's exact, but it looked harsh, so there's a floor.
- **Only the corner facing the screen's follows it:** the others would keep 14 px. The user picked all of them, as "auto" says.
- **Lowering the island instead of moving it in:** it would break the row of islands at the top, and the notch's island height.
- **Making the island's corner as round as the screen's:** capped by the island's height, which is the problem.

**Cost:** no dependency. Finding the shift takes up to 3 ms (release) when the screen setting changes, and nothing per frame.


---

## D-96: `eave doctor` says what's wrong with the setup

**Status:** accepted (2026-10-14).

**Problem:** what goes wrong with a bar is mostly outside it: config.toml doesn't parse and the bar keeps the old one, a widget isn't in the build yet, two actions want the same key and one silently loses it (D-91), a MacBook's notch was never measured, eave.service can't reach the compositor because systemd lacks its environment. Each of these is printed to stderr once, if at all, and read only by someone who knows to look in the journal.

**What:** `eave doctor`, a subcommand of the bar's binary like `eave calibrate`, runs the checks and prints one line each, `good`, `warning` or `problem`, with what to do under it. It exits with 1 when it found a problem. `--report <path>` also writes them as `[[check]]` tables, for the `essential` plugin's Check health ([D-98](#d-98-the-essential-and-update-plugins-the-bars-own-actions-in-the-dormer)).
- **config.toml:** it's there and parses (`config::load_from`), and every warning the bar would print (`Config.warnings`).
- **Widgets:** each one config.toml names is in this binary's registry and takes its options. It makes each widget, as the bar would, and drops the task `new` returns: the widget never runs.
- **Keys:** the made widgets' actions, settled by `shortcut::resolve_actions`, the bar's own rule (D-91). Its warnings are the checks.
- **Screen:** Hyprland answers. The focused monitor, the one the bar opens on, has `notch` measured when the kernel shows a notch on it (D-71, D-94).
- **systemd:** `eave.service` and `eave-rebuild.path` are enabled, the manager's environment has `WAYLAND_DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE` (when the service is enabled), and the update units are installed ([D-97](#d-97-eave-build-check-and-the-update-units)). Asked over D-Bus through a new `eave_dbus::systemd` module, never `systemctl`. Without a user manager, one note says so.

**Why:**
- **In the bar's binary.** It must check against the widgets *this* binary has, read config.toml with the bar's own parser and settle keys with the bar's own rule, or it would drift from what the bar does. And it must run when the bar won't start, which a check inside the running bar can't.
- **A report file, not output parsing.** The plugin can't run it as a child (plugins don't spawn processes), so Hyprland's exec runs it, and the file is how it answers. Written beside and renamed into place, so it's never read half-written.

**Alternatives:**
- **Checks in the plugin:** it can't see the registry or the host's config parser, and a second copy would drift.
- **In `eave-build`:** it doesn't know the widgets' options or keys, only their names.
- **A health page inside the running bar:** useless exactly when it's needed most.

**What we give up:** what it can't know without running the bar: which renderer iced actually picked, and keys on popover rows (only the dormer's are checked).

**Cost:** no new dependency (`toml_edit` writes the report, already there for calibrate). The binary grows 75 KB (21,778,352 → 21,853,744 B with every built-in but the two new plugins). Nothing runs in the bar.

---

## D-97: `eave-build check`, and the update units

**Status:** accepted (2026-10-14). Amends [D-93](#d-93-update-reinstalls-eave-build-before-the-bar-not-after): the handover carries `--summary` and what moved.

**Problem:** `eave-build update` moves everything at once, with no way to see what it would move first. And to update from the dormer, something must run it outside the bar: an update restarts the bar, so a child of the bar would be killed halfway (systemd stops the whole cgroup).

**What:**
- **`eave-build check`** fetches eave and every git plugin config.toml uses, and prints what `update` would move: `weather: 1111111 → 2222222 (3 new commits)`. It checks nothing out and builds nothing. `Sources::checkout` is split, so `check` and the move share `wanted`, which fetches and resolves. Paths and built-ins are skipped; a source never fetched is mentioned and left alone.
- **`--summary`** leaves `~/.cache/eave/<command>.toml`: `ok`, `error`, and `[[moved]]` with `label`, `from`, `to`, `commits`. Every move is now recorded, not only eave's. A handover to a newly installed eave-build (D-93) passes `--summary` on, and what had moved in `EAVE_BUILD_MOVED`.
- **Two units**, written by `eave-build init`: `eave-update.service` runs `eave-build update --summary`, `eave-update-check.service` runs `eave-build check --summary`. Oneshots at idle priority, like `eave-rebuild.service`, without its failure `notify-send`: the plugin tells.
- **`init` reloads systemd** after writing the units. Before, only an update that moved eave did.

**Why:**
- **A summary file:** the bar that started the update may be gone when it ends, and the one that replaced it needs to know what happened. A file outlives both, and taking it (read, then delete) tells it once.
- **systemd units:** a run outside the bar's cgroup, with its output in the journal, started over D-Bus with no process spawned by the bar. systemd is already what runs the bar and the rebuild.
- **Fetching moves nothing:** `check` only updates the checkouts' remote-tracking refs, so nothing changes version without `update` ([D-31](#d-31-updates-are-explicit-installs-only-when-the-binary-changed)).

**Alternatives:**
- **`systemd-run --user`** from the plugin: a process the bar spawns, and the unit would be anonymous.
- **eave-build sending the notification:** it would need zbus, or `notify-send`, a binary not everyone has.
- **A periodic check:** polling the network, and something automatic. Nothing is checked unless asked.

**Cost:** no new dependency. Two unit files.

---

## D-98: The `essential` and `update` plugins: the bar's own actions in the dormer

**Status:** accepted (2026-10-14).

**Problem:** calibrating the screen, restarting the bar after a `renderer` change, checking the setup and updating eave all need a terminal and the right command.

**What:** two built-in plugins, with nothing in the bar, for `[[dormer]]`. The starter config has both, and they can be removed like any widget.
- **`essential`:**
  - **Calibrate screen** (`calibrate`) has Hyprland run `<the bar's binary> calibrate`.
  - **Restart bar** (`restart`) calls systemd's `RestartUnit("eave.service")`. It's listed only when this bar is that service's current run: `$INVOCATION_ID` equals the unit's `InvocationID`.
  - **Check health** (`health`) has Hyprland run `<the bar's binary> doctor --report $XDG_RUNTIME_DIR/eave/doctor.toml`, waits for the file with inotify (10 s at most), and opens a dialog with the checks, problems first ([D-96](#d-96-eave-doctor-says-whats-wrong-with-the-setup)). While it runs, the action reads "Checking health…".
- **`update`:**
  - **Check for updates** (`check`) and **Update eave** (`update`) start `eave-update-check.service` and `eave-update.service` over D-Bus ([D-97](#d-97-eave-build-check-and-the-update-units)).
  - It follows both units through systemd's `PropertiesChanged` signals. When one finishes, it takes the summary and tells it in a notification: what moved or would move, or the error, critical for a failed update, with the journal to read.
  - A check that found updates offers **Update** on its notification. The plugin listens for the notification's `ActionInvoked` only while one offers it.
  - While either unit runs, the only action is "Checking for updates…" or "Updating eave…". Without the units (an older `init`) or without systemd, it lists nothing.
- The bar's binary is `current_exe()`, without the " (deleted)" Linux adds once eave-build has replaced it: the new binary at the same path is the one to run.

**Why:**
- **Plugins, not the host's actions:** everything is a plugin, and these use only the public API. A bar that doesn't want them leaves them out.
- **Hyprland's exec and systemd, never `std::process`:** core plugins don't spawn processes ([D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel)). The `apps` plugin launches apps the same way. Processes started this way aren't the bar's children, so a restart doesn't take them along.
- **Restart only under the service:** a bar started by hand, from a worktree, or from Hyprland's config would otherwise restart some other bar, or nothing.
- **Two plugins, not one:** updating is about eave-build and its units, and depends on systemd. The bar's own actions don't.
- **No icons:** they're commands, not things on screen.

**Alternatives:**
- **One plugin that can't be removed:** against "everything is a plugin" and the empty bar without a config.
- **The host's own dormer actions:** they'd work without a config, but they'd be a second kind of action, outside the plugin API.

**What we give up:** both bend D-23 a little: `update` needs systemd, and Restart bar does too. Without systemd they leave those actions out.

**Cost:** no new dependency (rustix's `event` and inotify were already built). Release binary with every built-in, measured on CachyOS x86_64:

| | Binary | RSS, two clocks, tiny-skia | Idle CPU |
|---|---|---|---|
| main | 21,778,352 B | ~17.9 MB | 0 ticks in 2 s |
| this, two clocks | 23,049,392 B | ~18.2 MB | 0 |
| this, + `essential` and `update` under `[[dormer]]` | | ~18.4–18.5 MB | 0 |

The binary grows 1.27 MB: 75 KB for `eave doctor`, about 565 KB for `essential` and 630 KB for `update`, mostly zbus, serde and iced code instantiated anew for each plugin, as with `appearance` (D-92). A bar built by eave-build has only what config.toml uses. The plugins' RSS is the session bus connection and two unit subscriptions.

---

## D-99: What continuous animation costs (measured, no change)

**Status:** measurement (2026-10-14). Nothing changed; it informs D-64 and D-67.

**Question:** could eave-claude's mascot, or an icon, animate all the time? D-64 kept the mascot still at rest because "every frame redraws the whole surface". D-66 found that overstated for tiny-skia, which repaints only what changed. But iced_wgpu, the default since D-67, has no damage tracking. Every frame clears, renders and presents the whole surface.

**How:** a bench-only branch (deleted) added two env knobs, `EAVE_BENCH_SPRITE_FPS` (replacing `SPRITE_FRAME`) and `EAVE_BENCH_GULP_FPS` (capping `gulp`'s rate). It ran a second bar from the user's config, without eave-claude and notifications, plus a bench widget: the mascot's question sprite looping with every frame different, or `gulp(icon(Battery))`. Each run had 10 s of warm-up, then 60 s measured: eave's and Hyprland's CPU from `/proc/<pid>/stat`. On tiny-skia, `EAVE_FRAMES` confirmed each run reached its rate. Asahi, M1 Pro (8 cores), 120 Hz, scale 2, the user working meanwhile. Battery power was measured too but swamped by that work (13–22 W), so it's left out.

**Measured** (% of one core):

| fps | GPU sprite: eave / Hyprland | GPU icon: eave / Hyprland | CPU sprite: eave / Hyprland | CPU icon: eave / Hyprland |
|---|---|---|---|---|
| none | 0.2–0.4 / 1.8–2.1 | | 0.2–0.7 / 1.1–3.4 | |
| 4 | 0.9 / 0.9 | 1.0 / 1.7 | 0.5 / 4.6 | 0.3 / 3.1 |
| 10 | 2.3 / 3.4 | 2.0 / 2.5 | 1.6 / 6.2 | 0.9 / 3.5 |
| 30 | 5.1 / 4.6 | 5.3 / 5.2 | 1.0 / 4.8 | 3.0 / 5.9 |
| 60 | 9.9 / 9.6 | 10.3 / 9.5 | 0.8 / 4.8 | 4.3 / 8.9 |
| 120 | 18.4 / 15.3 | 18.3 / 14.9 | 1.3 / 5.0 | 8.6 / 14.5 |

**What it says:**
- On the GPU a frame costs the same whatever moves: ~1.5 ms of eave's CPU and ~1.3 ms of Hyprland's. The cost is the frame rate, not the size of what animates.
- On tiny-skia a small sprite is nearly free at any rate (~0.1 ms a frame). A swelling icon costs ~0.7 ms a frame at 120 fps, because its paths are redrawn every frame. Hyprland stayed flat for the sprite but rose with the icon, which isn't explained. It may be noise from the user's work.
- **A loop at the sprite's 4 fps is cheap on both renderers:** under 1% of one core. The mascot could loop all day.
- **30 fps or more for good is not:** ~10% of one core on the GPU, 50–100× the bar at rest, and it wakes the CPU and GPU 30–120 times a second, which this didn't measure on battery. Keep fast motion to bursts.

**Damage for the GPU?** Not pursued. eave's ~1.5 ms likely isn't pixels (0.3 Mpx is nothing to a GPU, and tiny-skia walks the same tree for ~0.1 ms). It's probably fixed per-frame work: iced_wgpu preparing layers and text, encoding, submitting, acquiring a frame. Damage would mostly relieve Hyprland, and wgpu exposes neither buffer age nor present-with-damage, so it means patching wgpu too. **Next step if this comes back:** `perf record` the GPU bar at 60 fps to see where eave's 1.5 ms goes. If it's iced_wgpu redoing text or layers every frame, caching that is a far smaller patch.

---

## D-100: Buttons take shortcuts too

**Status:** accepted (2026-10-15). Amends [D-91](#d-91-shortcuts-plugins-name-them-users-rebind-them-hyprland-owns-the-global-ones).

**What:** `button(..).shortcut(id, key)`, as on a `list_item`. In a popover or dialog the host binds the key to the button's `on_press` and draws its keycaps on the button, after what it says. It resolves with the panel's rows: the user's key from `keys` by id first, then the first to want it. Buttons in a `header`'s leading or trailing node count, which rows never needed. In the bar, and on icon-only buttons, the key works but isn't drawn.

**Why:** eave-claude's chat has a Show button in its header, to go to the session's terminal. That's an action on the page, not one of a list to pick among, so it isn't a row; and the chat's field has the keyboard, so it needs a key with a modifier (`Ctrl+T`), which only a shortcut gives. D-91 turned buttons down for having no use yet.

**Alternatives:**
- a row for Show: it's the header's action, not an entry in the conversation.
- a dormer action with a key: works only while the dormer is open, not in the chat.

**Cost:** one optional field on `Kind::Button`. eave-api grows, compatibly: plugins using it need the next eave-api.

---

## D-101: Pausing and muting show for a moment

**Status:** accepted (2026-10-15).

**What:** the media item hands its cover over to a pause or play sign for 1.5 s after the followed player pauses or plays, whether from the bar, its window, a headphone button or a media key. The volume item nudges down on mute, putting the struck speaker back at once if the percent was showing, and nudges up on unmute, showing the percent it came back to for 1.5 s. Both go off the service's own readings, so a click and a key look the same, and each change shows once: a change we made ourselves already shows, and its reading only confirms it.

**Why:** a pause showed only as the item dimming, and a mute only as the stroke through a speaker that might be collapsed or on a hidden bar, so a key press had no answer you'd notice. The flash is the same move the volume and brightness already make for a level (`LEVEL_SHOWN`): an `alternate` that hands over and back, guarded by a change counter, with no new motion or primitive. The nudge brings a collapsed item or a hidden bar out to show it (D-58).

**Alternatives:**
- an OSD (still a maybe on the roadmap): a new surface and host feature for what the items can already say in place.
- flashing the mute in the media item: the sound's state belongs to the volume item.

---

## D-102: The battery says when the charger goes in

**Status:** accepted (2026-10-15). Amends [D-63](#d-63-the-battery-sends-its-own-notifications).

**What:** plugging in the charger sends a notification, like D-63's: "Charging" with the percent and, when the kernel says, how long until full; "Plugged in" for a battery that's full or held where it is. It expires after 4 s, unlike the low and charged ones, and a new one takes the place of one still up. Unplugging sends nothing, and takes it away if it's still there. `charging = false` turns it off, `notify = false` all of them. Only the change counts: a bar starting with the charger in says nothing.

**How it tells:** from the charger's `online` when the machine has one, since the battery's `status` can still read discharging for a moment after the plug goes in; otherwise from the battery's status.

**Why:** a loose cable or a dead socket charges nothing, and the bar's green battery is easy to miss. A short notification answers the plug the way a phone does.

**Alternatives:**
- a nudge on the battery item: easy to miss, and nothing else in the bar answers a plug.
- one on unplugging too: you know you unplugged; asked for only on plugging in.

---

## D-103: Bluetooth through BlueZ

**Status:** accepted (2026-10-15). Amends [D-23](#d-23-core-plugins-depend-only-on-hyprland-and-the-kernel) for Bluetooth, the way [D-42](#d-42-wi-fi-controls-through-iwd) did for Wi-Fi.

**What:** a `bluetooth` widget and a `crates/eave-bluez` service, on `eave-dbus`'s shared system connection. The bar shows the Bluetooth rune (a new `Icon::Bluetooth { connected }`, with a dot on each side while a device is connected), dimmed while off. *(It first showed the connected device and its battery when the pointer rested on it; taken out on 2026-10-15 at the user's request: the rune alone, and the battery in the popover.)* The popover turns Bluetooth on and off, connects and disconnects paired devices, looks for devices while it's open and pairs them, and has a page per paired device to forget it. The dormer turns Bluetooth on and off (`bluetooth-on`, `bluetooth-off`) and connects or disconnects each paired device. `show` keeps the item to when Bluetooth is on, or a device is connected.

**Why BlueZ, against D-23:** on Linux, Bluetooth is BlueZ. The kernel's HCI sockets would mean reimplementing pairing, profiles and the audio hand-off to PipeWire, all of which go through bluetoothd anyway. Its API is small (adapter, device, battery, agent manager) and on the system bus eave already shares. Without it, the popover says Bluetooth needs BlueZ; turned away by its bus policy, it says that.

**How:**
- `eave_dbus::watch_all` re-reads on any signal from `org.bluez`, one `GetManagedObjects` folded into the first adapter, its devices (paired first by name, then the strongest) and their `Battery1`. Devices come and go while looking, so it's every signal, not only `PropertiesChanged` on known paths.
- **Watched all the time, unlike iwd (D-42):** the bar item shows what's connected and its battery, and BlueZ is quiet unless something changes. Looking for devices, the chatty part, runs only while the popover is open, and stops when it closes (BlueZ also stops it if eave leaves).
- **Pairing** goes through an `org.bluez.Agent1` served on the shared connection at `/eave/bluez/agent`, as `KeyboardDisplay` so BlueZ can pick whatever the device can do. It's registered before each pairing and never made the default agent: BlueZ asks the agent of whoever called `Pair`, so it only ever hears about devices eave pairs, and it turns down any other. A question (confirm a passkey, type a PIN or passkey, show a code to type on a keyboard) goes out on `requests()`, which the widget watches only while pairing, and waits up to a minute for `answer`. A paired device is trusted and connected right away, so it reconnects on its own later.
- Errors become short reasons under the device (`reason()`), like iwd's.

**Alternatives:**
- the `bluer` crate: BlueZ's own Rust bindings, but on its own D-Bus stack (dbus-rs and libdbus), a second D-Bus library beside zbus (D-7).
- making eave the default agent: it would answer every device that tries to pair, also when no popover is open to ask.
- unblocking rfkill from the popover: the kernel's `/dev/rfkill` could do it, but a blocked adapter is rare and deliberate. The popover says it's blocked.

**Measured** (Asahi, release, both renderers built in, run on tiny-skia, BlueZ running with one paired device): +0.69 MiB of binary (20,082,304 → 20,803,248 bytes), most of it the agent's `interface` code and the second plugin's popover. A clock alone ~18.8 MB RSS; with `bluetooth` ~19.1–19.5 MB. Idle CPU unchanged: one tick in 20 s either way.

**Cost:** `Icon::Bluetooth` grows eave-api, compatibly: plugins using it need the next eave-api. `eave-bluez` is a new service crate, to publish beside the others (D-50) once third-party plugins want it.

---

## D-106: Pages hand over

**Status:** accepted (2026-10-15).

**What:** `ui::page(order, content)` says that `content` is one page of a sequence the plugin moves through, and `order` says which one. When `order` changes, the host hands over the way tabs do (D-43). The old page slides `TAB_SLIDE` away and fades out, then the new one slides and fades in from the side it's on, over `TAB_FADE` in all. A higher order comes from the right and a lower one from the left. While `order` stays the same, the content just changes. In the bar, a page is just its content.
- **The old page is kept by the host** (`page.rs`). The plugin only sends the page it's on. Each view's content moves into the widget state with its own tree as it's diffed, and a new order keeps the old content there until it's gone. The same order is diffed in place, so a page keeps its state, like a scrolled list or a field.
- **Turning around** mid-way goes back from what's on screen, as tabs do. A third page while the old one is still leaving lets it finish leaving.
- **Used by** network (the list at 0; a network's details, its password and the connection at 1), bluetooth (the list at 0; a device's details and pairing at 1), and the clock's calendar (each month numbered `year * 12 + month`, both its title and its grid).
- **The content no longer flashes black when a popover changes size.** The content of a popover waits until the shape is halfway out of the island, and that rule ran on every change of size too. It measured from the island, so a page that changed the panel's height by more than about half covered the content with the island's color until the shape got there. Once the content has shown, a change of size now only springs the shape (`PopoverMotion::out`).

**Why:** a page change was the one change in a popover that didn't move: the shape sprang to the new size while the text changed in a frame, and often went black first. Only the plugin knows which page comes after which, so it says so with a number. The host turns that number into a direction.

**Alternatives:**
- **the host spotting a new page by itself**, from a fingerprint of the content or a changed `header`: it can't tell a new page from the same page changing, like the details filling in as iwd answers, and it can't tell forward from back.
- **a key without an order**: it can say "another page" but not which way it went, and a calendar going back a month would slide the wrong way.
- **a crossfade**: iced has no group opacity, and the host fades by covering with the island's color, so two pages crossing would cover each other where they overlap. Fading out and then in is what tabs and alternates already do.
- **keeping the old node and drawing it again**: drawing a node needs the host's icons and palette at view time, and a fresh widget state for it would lose the old page's scroll position as it leaves. Moving the built content into the widget state keeps both.

**Measured** (Asahi aarch64, release, default GPU build, the user's config, two alternating runs against main): binary 20,803,248 → 20,868,784 B (+64 KiB, one page, for D-106 and D-107 together); RSS ~79.7 MB both, within noise; CPU 6 s after start the same within noise (1.6–2.1 %, both still settling). No dependency is added, and nothing runs while no page changes. Only a page that's leaving is held, until it's gone.

---

## D-107: Items that come or go together, one at a time

**Status:** accepted (2026-10-15). Extends [D-55](#d-55-several-islands-per-zone-collapsible-items-and-dividers-set-in-configtoml) and [D-27](#d-27-islands-morph-when-their-content-changes).

**What:** when several items come into an island at once, like the collapsible items shown by the chevron, each fades in `STAGGER` (45 ms) after the one before. The one nearest the chevron comes first. Hiding them goes the other way: the furthest goes first, and the island keeps its width until the last is gone. Then it springs to its new width.
- **In:** `Motion::arrange` sees several new keys at once on an island that's already there, and gives each a `wait` before its fade sets off, in order from the toggle's end (`Motion::from`). A single new item, like a reveal (D-58) or a widget's item appearing, comes in at once as before. A new island grows out with all its items, as before.
- **Out:** the items have to stay laid out while they fade, so the host keeps them. Pressing the chevron moves the island from `Bar::expanded` to `Bar::collapsing`. The view still lays out its collapsible items and hands their keys (dividers' too) to the island as `Toggle::leaving`. The island fades them out in turn and sends `Toggle::on_gone` (`Collapsed(island)`) once, when the last is out. The host then leaves them out, and the island springs narrower.
- **Changing your mind:** pressing the chevron again while they leave brings them back, each from where its fade got to. A reload forgets an island's collapsing like its expanding.
- The squash on pressing (`COLLAPSE_SQUASH`) is unchanged. The fades ride `MORPH`. `STAGGER` is a gap, not a feel.

**Why:** the user asked for the icons to come out one at a time, in order. Coming out of the chevron in a line says where they came from, the way a popover grows out of its island. All at once, they just appear. Going back in the reverse order makes the round trip one motion, and keeping the width until they're gone means nothing slides under an item that's still fading.

**Alternatives:**
- **delaying each item's fade by its distance from the toggle in pixels:** wide items would wait longer for no reason. Their order is what the eye follows.
- **staggering every new item, a new island's too:** the whole bar would ripple in at start, and a new island already grows out of a sliver.
- **dropping the items at once and fading copies of them in the island:** iced can't draw an element it no longer has, so the host has to keep them laid out anyway.
- **the island shrinking as each one goes:** the items left would slide under the ones still fading out.

**Measured** (Asahi aarch64, release, default GPU build, the user's config, two alternating runs against main): binary 20,803,248 → 20,868,784 B (+64 KiB, one page, for D-106 and D-107 together); RSS ~79.7 MB both, within noise; CPU 6 s after start the same within noise (1.6–2.1 %, both still settling). Nothing runs while no item waits.

