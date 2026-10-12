# Roadmap

The order follows two ideas: **prove the hardest property early** (smooth animation, in step 2) and **save the most annoying protocol for last** (the tray). Each step should leave the bar usable, and its numbers measured ([development.md](development.md#measuring)).

## Done

- [x] **0. Skeleton.** Layer-shell surface anchored top, 32px exclusive zone, no keyboard focus. Three-part layout (left / center / right). Clock and date on a 1 s tick. CPU renderer by default, `gpu` feature optional. Baseline measured.
- [x] **0.6 Reload and restart.** `config.toml` hot reload through inotify, keeping unchanged widgets' state. `eave-build --restart`. systemd user units for the bar and for rebuilding when `plugins.toml` changes ([D-20–D-22](decisions.md), [running.md](running.md)).
- [x] **0.5 Plugin foundation.** Workspace split into `eave-api` (SDK), `eave` (host) and `eave-build`. Closed UI vocabulary, type-erased widget registry, `config.toml` zones (empty bar without it), the clock rewritten as a plugin with a calendar popover, host-owned popover layer surfaces in daemon mode, and third-party plugins compiled in from git ([D-11–D-15](decisions.md)).

- [x] **5.5 Floating islands.** Left, center and right zones drawn as rounded islands that fit their items and vanish when empty, with a gap from the screen edges ([D-23](decisions.md)).
- [x] **5.6 Islands you can touch.** Squircle corners. A press squeezes the island, a drag pulls it with a rubber band, and a spring snaps it back, all on the frame clock ([D-25](decisions.md)).
- [x] **1. Hyprland IPC client.** `crates/eave-hyprland`: one shared socket2 reader, socket1 queries and dispatches, and `watch(snapshot)` so a widget gets its initial state and then every later event ([D-6](decisions.md), [D-17](decisions.md)).
- [x] **2. Workspaces widget + animated indicator.** `plugins/workspaces` on the new `ui::switcher`: the host slides the mark between workspaces, one step per frame callback, and stops asking for frames when it arrives ([D-16](decisions.md)). Scroll moves between workspaces. *Still to do:* measure CPU during a switch with the debug overlay.
- [x] **3. Active window title.** `plugins/window`, from the same event stream.
- [x] **3.5 Keyboard layout.** `plugins/keyboard`: `activelayout` events, click to switch, hidden with a single layout.
- [x] **4. Battery.** `plugins/battery`: UPower's display device over zbus, re-read only on `PropertiesChanged` ([D-18](decisions.md)).
- [x] **4.5 Network.** `plugins/network`: NetworkManager's primary connection, with Wi-Fi strength, band, address and gateway in the popover. iwd-only systems (no NetworkManager) aren't covered yet.
- [x] **5. Volume.** `plugins/volume` on `crates/eave-pulse`: the PulseAudio protocol served by pipewire-pulse ([D-19](decisions.md)). Scroll changes it, right click mutes, and the popover has −/+, mute and output switching.

## Next

- [ ] **6. Media.** MPRIS over zbus.
- [ ] **7. Bluetooth.** BlueZ over zbus, on `eave-dbus`.
- [ ] **8. System tray.** StatusNotifierItem watcher and host, with menus.

Every item from here on is a plugin, written against `eave-api` like a third-party one. Anything a built-in widget needs goes into the API or the kit, not into a private shortcut.

## Plugin system follow-ups

- [ ] Close popovers on an outside click ([D-14](decisions.md) open issue): test `OnDemand` focus loss, then measure a catcher surface.
- [ ] Popover open/close animation, owned by the host.
- [x] Shared services crate (with step 1), so widgets on the same source share one connection. Hyprland is done ([D-17](decisions.md)); D-Bus is done too (`eave-dbus`, [D-18](decisions.md)).
- [ ] Publish `eave-api` on crates.io, so plugins build standalone without a `[patch]`.
- [ ] Kit primitives as widgets need them: icon, meter, slider, toggle. Switcher and item scroll are done ([D-16](decisions.md)).
- [ ] Per-monitor workspaces once there's one bar per monitor (the widget shows every monitor's workspaces today).

## Later

- [ ] One bar per monitor, following monitor hotplug. The host already runs as an `iced_layershell::daemon` (0.5), so this needs `StartMode::AllScreens` plus per-output state.
- [ ] Notch-aware layout on Asahi when `apple_dcp.show_notch=1` ([platform.md](platform.md#asahi-specifics)).
- [ ] `PKGBUILD` for Asahi and CachyOS.
- [ ] Maybe: OSD for volume and brightness, notification daemon, launcher, each as its own layer surface built on the same foundations.
