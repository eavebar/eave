# Roadmap

The order follows two ideas: **prove the hardest property early** (smooth animation, in step 2) and **save the most annoying protocol for last** (the tray). Each step should leave the bar usable, and its numbers measured ([development.md](development.md#measuring)).

## Done

- [x] **0. Skeleton.** Layer-shell surface anchored top, 32px exclusive zone, no keyboard focus. Three-part layout (left / center / right). Clock and date on a 1 s tick. CPU renderer by default, `gpu` feature optional. Baseline measured.

## Next

- [ ] **1. Hyprland IPC client.** Our own reader for the `.socket2.sock` event stream and `.socket.sock` queries, exposed as an iced subscription ([D-6](decisions.md)). Initial state comes from `j/workspaces` and `j/activeworkspace`, then events keep it current.
- [ ] **2. Workspaces module + animated indicator.** The smoothness test: the active-workspace highlight slides between positions, paced by frame callbacks. Measure CPU during and after the animation.
- [ ] **3. Active window title.** Comes from the same event stream.
- [ ] **4. Battery.** UPower over zbus ([D-7](decisions.md)), driven by signals.
- [ ] **5. Volume.** PipeWire listener. Scroll to change it, click to mute.
- [ ] **6. Media.** MPRIS over zbus.
- [ ] **7. Network / Bluetooth.** NetworkManager or iwd, plus BlueZ, over zbus.
- [ ] **8. System tray.** StatusNotifierItem watcher and host, with menus.

## Later

- [ ] Config file (TOML) with hot reload.
- [ ] One bar per monitor (`iced_layershell::daemon`), following monitor hotplug.
- [ ] Notch-aware layout on Asahi when `apple_dcp.show_notch=1` ([platform.md](platform.md#asahi-specifics)).
- [ ] `PKGBUILD` for Asahi and CachyOS.
- [ ] Maybe: OSD for volume and brightness, notification daemon, launcher, each as its own layer surface built on the same foundations.
