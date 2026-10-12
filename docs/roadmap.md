# Roadmap

The order follows two ideas: **prove the hardest property early** (smooth animation, in step 2) and **save the most annoying protocol for last** (the tray). Each step should leave the bar usable, and its numbers measured ([development.md](development.md#measuring)).

## Done

- [x] **0. Skeleton.** Layer-shell surface anchored top, 32px exclusive zone, no keyboard focus. Three-part layout (left / center / right). Clock and date on a 1 s tick. CPU renderer by default, `gpu` feature optional. Baseline measured.
- [x] **0.5 Plugin foundation.** Workspace split into `eave-api` (SDK), `eave` (host) and `eave-build`. Closed UI vocabulary, type-erased widget registry, `config.toml` zones (empty bar without it), the clock rewritten as a plugin with a calendar popover, host-owned popover layer surfaces in daemon mode, and third-party plugins compiled in from git ([D-11–D-15](decisions.md)).

## Next

- [ ] **1. Hyprland IPC client.** Our own reader for the `.socket2.sock` event stream and `.socket.sock` queries, exposed as an iced subscription ([D-6](decisions.md)). Initial state comes from `j/workspaces` and `j/activeworkspace`, then events keep it current.
- [ ] **2. Workspaces widget + animated indicator.** The smoothness test: the active-workspace highlight slides between positions, paced by frame callbacks. Measure CPU during and after the animation.
- [ ] **3. Active window title.** Comes from the same event stream.
- [ ] **4. Battery.** UPower over zbus ([D-7](decisions.md)), driven by signals.
- [ ] **5. Volume.** PipeWire listener. Scroll to change it, click to mute.
- [ ] **6. Media.** MPRIS over zbus.
- [ ] **7. Network / Bluetooth.** NetworkManager or iwd, plus BlueZ, over zbus.
- [ ] **8. System tray.** StatusNotifierItem watcher and host, with menus.

Every item from here on is a plugin, written against `eave-api` like a third-party one. Anything a built-in widget needs goes into the API or the kit, not into a private shortcut.

## Plugin system follow-ups

- [ ] Close popovers on an outside click ([D-14](decisions.md) open issue): test `OnDemand` focus loss, then measure a catcher surface.
- [ ] Popover open/close animation, owned by the host.
- [ ] Shared services crate (with step 1), so widgets on the same source share one connection.
- [ ] Publish `eave-api` on crates.io, so plugins build standalone without a `[patch]`.
- [ ] Kit primitives as widgets need them: icon, meter, slider, toggle.

## Later

- [ ] Hot reload of `config.toml` (the file itself exists since 0.5).
- [ ] One bar per monitor, following monitor hotplug. The host already runs as an `iced_layershell::daemon` (0.5), so this needs `StartMode::AllScreens` plus per-output state.
- [ ] Notch-aware layout on Asahi when `apple_dcp.show_notch=1` ([platform.md](platform.md#asahi-specifics)).
- [ ] `PKGBUILD` for Asahi and CachyOS.
- [ ] Maybe: OSD for volume and brightness, notification daemon, launcher, each as its own layer surface built on the same foundations.
