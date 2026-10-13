# Roadmap

The order follows two ideas: **prove the hardest property early** (smooth animation, in step 2) and **save the most annoying protocol for last** (the tray). Each step should leave the bar usable, and its numbers measured ([development.md](development.md#measuring)).

## Done

- [x] **0. Skeleton.** Layer-shell surface anchored top, 32px exclusive zone, no keyboard focus. Three-part layout (left / center / right). Clock and date on a 1 s tick. CPU renderer by default, `gpu` feature optional. Baseline measured.
- [x] **0.6 Reload and restart.** `config.toml` hot reload through inotify, keeping unchanged widgets' state. `eave-build --restart`. systemd user units for the bar and for rebuilding when `plugins.toml` changes ([D-20–D-22](decisions.md), [running.md](running.md)).
- [x] **0.5 Plugin foundation.** Workspace split into `eave-api` (SDK), `eave` (host) and `eave-build`. Closed UI vocabulary, type-erased widget registry, `config.toml` zones (empty bar without it), the clock rewritten as a plugin with a calendar popover, host-owned popover layer surfaces in daemon mode, and third-party plugins compiled in from git ([D-11–D-15](decisions.md)).

- [x] **5.5 Floating islands.** Left, center and right zones drawn as rounded islands that fit their items and vanish when empty, with a gap from the screen edges ([D-24](decisions.md)).
- [x] **5.6 Islands you can touch.** Squircle corners. A press squeezes the island, a drag pulls it with a rubber band, and a spring snaps it back, all on the frame clock ([D-26](decisions.md)). Drags stretch it near the pointer, popovers too ([D-45](decisions.md)).
- [x] **5.7 Islands that morph.** Islands spring to their new shape when items come or go, items slide and fade in, and a popover grows out of its island instead of opening its own surface ([D-27](decisions.md), [D-28](decisions.md)).
- [x] **5.9 Nudges.** A plugin can nudge its island: volume bumps the edge above or below its icon, and switching workspace pushes the island the way it went ([D-32](decisions.md)).
- [x] **5.10 Hover.** An alternate can show another option while its item is hovered: volume and battery show their percent, the clock its date ([D-35](decisions.md)).
- [x] **1. Hyprland IPC client.** `crates/eave-hyprland`: one shared socket2 reader, socket1 queries and dispatches, and `watch(snapshot)` so a widget gets its initial state and then every later event ([D-6](decisions.md), [D-17](decisions.md)).
- [x] **2. Workspaces widget + animated indicator.** `plugins/workspaces` on the new `ui::switcher`: the host slides the mark between workspaces, one step per frame callback, and stops asking for frames when it arrives ([D-16](decisions.md)). Scroll moves between workspaces. *Still to do:* measure CPU during a switch with the debug overlay.
- [x] **2.5 App icons on workspaces.** Each workspace shows its apps' icons (two, then `+n`), every window while the pointer rests on it, and a window list on right click. Workspaces you aren't on are dimmed instead of marked. New primitives: `app_icon`, `expand`, button `on_secondary` ([D-38](decisions.md), [D-39](decisions.md)).
- [x] **3. Active window title.** `plugins/window`, from the same event stream.
- [x] **3.5 Keyboard layout.** `plugins/keyboard`: `activelayout` events, click to switch, hidden with a single layout.
- [x] **4. Battery.** `plugins/battery`: `/sys/class/power_supply`, re-read on the kernel's `power_supply` uevents and every 30 s while there's a battery. No UPower ([D-23](decisions.md), first done over UPower in [D-18](decisions.md)).
- [x] **4.5 Network.** `plugins/network` on `crates/eave-net`: the interface with the default route, from rtnetlink, with SSID, signal and band from nl80211, and address and gateway in the popover. It works with any network manager (NetworkManager, iwd, systemd-networkd, dhcpcd) ([D-23](decisions.md)).
- [x] **5.8 Install and update.** `install.sh` (curl | sh), `eave-build init`, plugins named in config.toml as `owner/repo[@rev]` instead of a separate plugins.toml, only used widgets compiled in, and updates only through `eave-build update` ([D-30](decisions.md), [D-31](decisions.md), [running.md](running.md)).
- [x] **5.12 Layout options.** Several islands in one zone, dividers between items, and collapsible items behind a chevron that shows on hover ([D-55](decisions.md)).
- [x] **5.11 Notifications.** The notification engine lists notices and the right island morphs into a panel of them ([D-47](decisions.md)). Plugins send theirs over the bus, like any app ([D-52](decisions.md)). eave's own notification server and its plugin, with the latest ones in its popover, history, inline replies and do not disturb ([D-48](decisions.md), [D-49](decisions.md)), now in [eaveshell/eave-notifications](https://github.com/eaveshell/eave-notifications) ([D-50](decisions.md)).
- [x] **5. Volume.** `plugins/volume` on `crates/eave-pulse`: the PulseAudio protocol served by pipewire-pulse ([D-19](decisions.md)). Scroll changes it, right click mutes, and the popover has −/+, mute and output switching.

- [x] **6. Media.** `plugins/media` on `crates/eave-mpris`: any MPRIS player over the session bus. The track's cover in the bar, title and artist, right click to play or pause, scroll to skip; cover, seek bar, controls and players in the popover ([D-56](decisions.md)). Covers through a new `ui::cover`, from files or https ([D-57](decisions.md)).
- [x] **6.5 Style.** Black and white, with color only as a signal or from the content. A soft white fill marks what's current, the slider is a line that thickens under the pointer, and a media popover takes its cover's color ([D-61](decisions.md), [design.md](design.md)).

## Next

- [ ] **7. Bluetooth.** BlueZ over zbus, on `eave-dbus`.
- [ ] **8. System tray.** StatusNotifierItem watcher and host, with menus.

Every item from here on is a plugin, written against `eave-api` like a third-party one. Anything a built-in widget needs goes into the API or the kit, not into a private shortcut.

## Plugin system follow-ups

- [x] Close popovers on an outside click, via a Hyprland click bind ([D-14](decisions.md)).
- [x] Close popovers on any key press, Esc included ([D-36](decisions.md)).
- [x] Popover open/close animation, owned by the host: the island morphs into its popover and back ([D-28](decisions.md)).
- [x] Shared services crate (with step 1), so widgets on the same source share one connection. Hyprland is done ([D-17](decisions.md)); D-Bus is done too (`eave-dbus`, [D-18](decisions.md)), and so is the kernel's network state (`eave-net`, [D-23](decisions.md)).
- [x] Publish `eave-api` on crates.io, so plugins build standalone without a `[patch]`. The service crates too ([D-50](decisions.md)).
- [x] Packages (AUR first): `eave` on the AUR, from release tags, shipping `eave-build` ([D-54](decisions.md), [dist/aur](../dist/aur/README.md)).
- [x] Kit primitives as widgets need them: meter, slider, toggle, lists, sections, details, text fields ([D-40](decisions.md), [D-41](decisions.md)). Switcher and item scroll are done ([D-16](decisions.md)), and so are status icons ([D-29](decisions.md)).
- [x] Real popovers: Wi-Fi networks to join through iwd ([D-42](decisions.md)), sound devices, inputs and apps, battery health, a six-week calendar, workspace windows, keyboard layouts.
- [ ] Per-monitor workspaces once there's one bar per monitor (the widget shows every monitor's workspaces today).

## Later

- [ ] One bar per monitor, following monitor hotplug. The host already runs as an `iced_layershell::daemon` (0.5), so this needs `StartMode::AllScreens` plus per-output state.
- [ ] Notch-aware layout on Asahi when `apple_dcp.show_notch=1` ([platform.md](platform.md#asahi-specifics)).
- [x] `PKGBUILD` for Asahi and CachyOS: the AUR package builds on aarch64 and x86_64 ([D-54](decisions.md)).
- [ ] Maybe: OSD for volume and brightness, launcher, each built on the same foundations. (Notifications are done: 5.11.)
- [ ] Notification images (`image-data`, `image-path`): a new primitive for a picture sent by the app, not found in the icon theme.
