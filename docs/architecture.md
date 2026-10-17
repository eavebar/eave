# Architecture

## The two halves of a bar

A bar does two separate jobs:

1. **Being a bar-shaped surface.** On Wayland a normal window gets tiled by the compositor. A bar instead uses the **`wlr-layer-shell`** protocol to anchor to a screen edge, sit on a layer (`background`, `bottom`, `top` or `overlay`), and reserve an **exclusive zone** so tiled windows stay out of its space. `iced_layershell` handles this for us.
2. **Knowing what to show.** Every widget reads a different system interface. This is where most of the work is.

eave keeps these apart. The **host** does the first job and owns everything visual. **Widgets** (plugins) do the second and only describe content. With no widgets configured, the bar draws nothing (it still reserves its space).

## Three layers

```
┌──────────────────────────────── host (crates/eave) ────────────────────────────────┐
│ surfaces: bars (popovers grow out of islands) · zones · theme · kit (nodes → pixels)│
│ click routing · popover lifecycle · animation                                      │
└───────────────▲───────────────────────────────────────────────▲────────────────────┘
                │ Item / Panel (closed vocabulary)               │ Effect (task, open/close popover)
┌───────────────┴───────────── widgets (plugins) ────────────────┴────────────────────┐
│ clock · battery · workspaces · …   each: Config, state, Message, update, views      │
└───────────────▲────────────────────────────────────────────────────────────────────┘
                │ Subscription
┌───────────────┴───────────── services ─────────────────────────────────────────────┐
│ Hyprland IPC · kernel (netlink, sysfs) · D-Bus (MPRIS, SNI) · PipeWire · timers    │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- **Host** ([D-11](decisions.md)): owns every surface, the layout, the look and the motion. It is not pluggable.
- **Widgets** implement `eave_api::Widget`. They can't pick colors or sizes, can't build raw iced elements, and can't move other items or the bar ([D-13](decisions.md)).
- **Services** are shared event sources, as ordinary public crates that plugins depend on. `crates/eave-hyprland` is the first: every widget that watches Hyprland shares one socket2 connection ([D-17](decisions.md)). `crates/eave-net` holds one netlink listener for links, addresses, routes and Wi-Fi ([D-23](decisions.md)). `crates/eave-dbus` shares one system bus connection and one session bus connection ([D-18](decisions.md)). `crates/eave-pulse` holds the one audio event subscription ([D-19](decisions.md)). `crates/eave-iwd` controls Wi-Fi through iwd, watched only while the network popover is open ([D-42](decisions.md)). `crates/eave-mpris` follows media players on the session bus ([D-56](decisions.md)). `crates/eave-bluez` watches and controls Bluetooth through BlueZ, with a pairing agent ([D-103](decisions.md)).

Core plugins depend only on Hyprland and the kernel, plus an audio server for volume. They don't need UPower, NetworkManager or any other daemon or binary to be installed ([D-23](decisions.md)).

## Data flow

```
 event sources                       host (iced daemon)                                 compositor
┌───────────────────┐  AnyMessage   ┌─────────────────────────┐ view(window) ┌────────┐ frame cb ┌──────────┐
│ widget            │─────────────▶ │ update: route to widget │────────────▶ │ layout │ ───────▶ │ Hyprland │
│ subscriptions     │  (id, msg)    │ instance, apply Effect  │              │ render │ ◀─────── │          │
│ pointer, windows, │─────────────▶ │                         │              └────────┘          └──────────┘
│ config.toml       │               │                         │
└───────────────────┘               └─────────────────────────┘
```

- Each widget instance's `subscription()` is tagged with the instance's stable id and mapped to `Message::Widget(id, AnyMessage)` ([D-15](decisions.md)). `update` hands the message to that instance only. Messages for an id that no longer exists (removed by a reload) are dropped.
- `config.toml` is watched with inotify. When it changes, the host re-reads it and rebuilds only the instances whose entry changed ([D-21](decisions.md)).
- A widget's `update` returns an `Effect`: a `Task` for async work, plus optional requests to open or close its popover or its dialog, or to nudge its island. `update` never does I/O.
- `view(window)` is pure. For the bar it lays out the left, center and right zones from `config.toml`, each as one or more squircle islands side by side, each only as wide as its items. The center zone stays centered on screen unless a side's islands reach it, and is then pushed aside. An island with nothing to show isn't drawn ([D-24](decisions.md)). Items marked `collapsible` are left out while their island is collapsed, and `divider` entries put a line between two items that show ([D-55](decisions.md)). When items come or go, the island morphs: its edges spring to the new size, items slide to their new places and new ones fade in ([D-27](decisions.md)). Items the chevron shows come out of it as the island's edge passes them. Collapsing keeps the items (`Bar::collapsing`, `Toggle::leaving`), taking no room, so the island closes at once and each goes back into the chevron as it reaches it, until the island says they're gone (`Collapsed`) ([D-108](decisions.md), [D-116](decisions.md)). With no window focused, the bar takes the keyboard on demand, so Hyprland keeps a drag's pointer on the bar ([D-117](decisions.md)). Pressing an island squeezes it; dragging stretches it toward the pointer with a rubber band, most where it was grabbed, and it springs back on release ([D-26](decisions.md), [D-45](decisions.md)). An open popover stretches the same way when a press on it isn't a control's. A widget's nudge pushes the island a little that way and pokes its outline, a jelly of springs that smooths the poke out by itself: the island swells on the side it's going toward ([D-32](decisions.md)). Up or down, the edge over the item holds a flat shelf out for as long as nudges keep coming, and lets it go once they stop ([D-105](decisions.md)). The screen's sides and top are walls: whatever moves an island keeps it `WALL_GAP` from them, round the screen's corners too, and a push that gets there bounces off (`wall.rs`, [D-104](decisions.md)). The notch and the grown top edge are drawn under the islands by `Bezel` (`bezel.rs`), which stretches them under a drag and turns a click on them into `Dormer(Click)` ([D-106](decisions.md)). With hypr-eave loaded, the pointer goes under them, in every theme ([D-113](decisions.md)). Each item is wrapped in a `Slot`, which turns unhandled clicks and scroll steps into messages. An open popover's `Panel` is drawn by the island it grew out of.
- Nothing is drawn when no message arrives. Pointer tracking is only subscribed while a popover is open, so hovering over the bar sends the app no messages. The pointer entering or leaving an item does redraw it: its `Slot` tells the alternates inside, which fade to their hover option and back ([D-35](decisions.md)). Moving within an item, or resting on it, draws nothing. An island with collapsible items does the same for itself: the pointer entering it makes room for its chevron, and leaving gives the room back, with no message to the app ([D-55](decisions.md)).

## Popovers

A popover isn't a surface of its own: the island holding the item turns into it ([D-28](decisions.md)). A click on an item that has a popover and no `on_press`:

1. The host records which bar was clicked and marks the popover open. `view` hands the panel to that zone's island.
2. The island lays the panel out at its natural size in an iced overlay and places it: top edge on the island, growing from the island's left edge, right edge or center (by zone), never narrower than the island, `EDGE_GAP` from the screen sides. It reports the place (`PopoverPlaced`).
3. The bar surface is already a little taller than its exclusive zone, for drags ([D-45](decisions.md)). The host grows it to fit (`SizeChange`, exclusive zone unchanged, so windows don't move) and sets an input region of the bar strip plus the panel, so the pointer still reaches windows beside and below it.
4. Once the surface is tall enough, the island morphs into the panel: its edges spring to the panel's, its items make way, and the panel's content fades in once the shape is halfway there, dropping into place on `MORPH` ([D-121](decisions.md)). Once it has shown, a change of the panel's size only springs the shape, and the content stays (`PopoverMotion::out`). Its outline is a skin over the morph: it stretches the way the shape grows and draws in across it, then settles ([D-111](decisions.md)). It pours: a tongue runs on ahead of it, down, joined to it by a fillet, and on the way back hangs under it ([D-114](decisions.md), `pour.rs`).
5. It closes on a second click on the item, when the widget returns `close_popover()` or stops offering a popover, on a click anywhere else, or on any key press but a modifier or one of its rows' shortcuts: the bar takes the keyboard only while a popover is open ([D-36](decisions.md)). `popover_view` gives the panel's rows the keys config.toml sets for them (`shortcut::resolve_panel`), then collects them and hands them to the island as its `on_key`, so a shortcut presses its row instead ([D-91](decisions.md)). While a popover is open, a non-consuming Hyprland bind reports every left click on socket2, and the host ignores the ones that land on the bar. Clicks on the bar are handled by the bar itself, and the island captures presses on its panel ([D-14](decisions.md)). Closing runs the morph backwards and lands on the island, stopping dead, and the skin carries on: each side the way it was going, splatting into the top and bulging at the sides. Any change of an island's size lands the same way ([D-111](decisions.md)). The content fades and lifts back toward the island as the shape shrinks. The island takes clicks again as soon as it starts closing: only an open popover keeps it to itself. While anything is out of the bar, the surface is as tall as the screen, so a morph never waits on a resize. When the island is back, it reports `PopoverClosed` and the host shrinks the surface again ([D-128](decisions.md)). A dialog or the dormer coming out of an island on the bar joins it while they're close: the dialog tells that island its outline each frame (`join::Join`), and the island draws both as one shape, the smooth union of the two, before its own items, while the dialog draws only its content ([D-122](decisions.md)).

## Bays

A widget's popover or dialog can hold other widgets' parts, in the bays it leaves for them ([D-109](decisions.md)). A part is an instance like any other, with an `owner`: it isn't drawn on the bar and has no popover of its own.

1. Wherever the host builds a panel (`popover_view`, `dialog_popover`, icon loading, the open and close hooks), `Bar::fill_bays` walks it. For each `ui::bay`, it asks the owner's parts, in config order, for `part(&bay)`. Their nodes take the bay's place, or nothing does.
2. Each part's nodes send `Routed(part, message)`. `route` unwraps it where the panel's messages and its shortcuts become `Message::Widget`, so they reach the part. `keys_of` gives each row the `keys` of whoever sends it, so `shortcut::resolve_panel` settles keys across the owner and its parts in one pass.
3. A part's `open_popover` and nudges go to its owner, and a message to a part reloads its owner's icons, since the part shows in the owner's view.

## Notifications

Notifications travel only over the session bus ([D-52](decisions.md)). The plugin that serves `org.freedesktop.Notifications` (eave-notifications) lists what it received as notices, and no other plugin lists any. While there are notices, the right island turns into a panel of them (with several islands on the right, the last one, at the screen's edge; [D-55](decisions.md)), the same way it turns into a popover ([D-47](decisions.md)):

1. After each of a widget's messages, the host compares its notices with the ones it keeps (`Bar::toasts`), by key. New ones go on top, changed ones are updated in place, and missing ones go.
2. While there are any, and no popover is open on the right island, the host keeps a `NoticePanel`, and `view` hands it to the right island of the toast bar (the first bar surface, for now the only one) as its popover, with the key `NOTICES`. A popover still turning back there gives way. Placing, growing the surface, the input region and the morph are a popover's (`NoticesPlaced`). Unlike a popover's, nobody opens it, it doesn't take the keyboard, and clicks elsewhere don't close it.
3. As notices come and go, the panel springs to its new size. When the last one goes, it folds back into the island, still showing them while they fade, and reports `NoticesClosed`.
4. The panel shows them as cards (`Bar::cards`): critical ones first, then the rest, newest first, notices of the same thread that come one after another sharing a card. Only the first three cards show; a line under them says how many more wait and opens their popover ([D-85](decisions.md)).
5. The host runs the clock of each shown card's newest notice while the panel is on screen and not pointed at (`NoticesHovered`), with one timer set for the next to expire (`ToastTimer`). When it runs out, the widget gets the `expires` message of every notice on the card. Cards that wait don't count down.
6. An open popover on the right island takes the island over, and the notices wait with their clocks stopped. The island's popover key changes, so it morphs straight from one to the other, both ways.

## The dormer

The dormer is the host's search dialog for widgets' actions ([D-84](decisions.md)). A Hyprland bind sends `eave-dormer` on socket2 (`bind = SUPER, SPACE, event, eave-dormer`, or `eave dormer`), and the bar's always-on socket2 filter turns it into `Dormer(Toggle)`. A click on the notch, or the middle of the top without one, sends `Dormer(Click)`, which does the same and tells the host the pointer is on the bar, so Hyprland's report of the next click there isn't taken for one outside ([D-106](decisions.md)):

1. The host asks Hyprland how tall the focused screen is, then opens the dormer a fifth of the way down (`Opened(top)`). Opening it closes any popover, the same way opening a popover closes it.
2. It asks every widget for `actions()`, and adds a row for each widget with a popover. `dormer.rs` ranks them for what's typed and keeps the best eight. After each of a widget's messages while it's open, the host asks again, and the selection stays on its row.
3. `view` stacks a host island at the top center over the bar's islands: a dot where the dormer comes out of the notch or the top edge, which comes out as a drop (`Popover::drop`, [D-86](decisions.md)). `Bezel` squishes the notch or the edge as the drop's neck lets go of it and catches it back (`Neck`). Without a notch, with an island in the center zone, it comes out of that island's shape instead, as a dialog does (`Bar::middle_island`, `Popover::home`, [D-106](decisions.md)). Its popover is the dormer's panel, a `text_input` over a `list`, placed with `Placement::Centered { top }`. Placing, growing the surface, the input region and the morph are a popover's (`Placed`). Arrow keys reach the host through the popover's `on_key`. Every other key stays in the field.
4. A pick (`Run`) closes the dormer, remembers the pick, and sends the action's message to its widget through the usual `Message::Widget` route, or opens the widget's popover.
5. Actions' keys ([D-91](decisions.md)): when it asks the widgets, `shortcut::resolve_actions` gives each action its key from the entry's `keys` in config.toml or the widget's default, settling clashes in config order. The rows draw them, and the popover's `on_key` runs any of them (`Key`), shown or not. `eave popover` and `eave action` arrive the same way as the dormer's keybind, as `eave-popover` and `eave-action` custom events, and open a popover or run an action by id without it.
5. It closes like a popover: Esc, a key the field doesn't take, a click elsewhere, or the keybind again. When its island is back, it reports `Closed` and the surface shrinks.

## Showing and hiding

The bar can show on hover, and over fullscreen windows it shows, hides or shows on hover, as `bar` in config.toml says. In fullscreen, notifications can wait ([D-88](decisions.md)):

1. The host's socket2 filter asks Hyprland whether a window covers the focused screen on the way in, and whenever windows open, close or move, go fullscreen or the workspace changes (`CheckFullscreen`). The answer (`Fullscreen`) is a client on the focused monitor's workspace with fullscreen mode 2. Mode 1 is maximized, which keeps clear of the bar.
2. `Bar::out` says whether the bar is out: always, or while hovered, peeking, or with a popover, the notification panel, the dormer or a dialog on it, until they've folded back. `view` wraps the islands in a `Lift` (`lift.rs`), which slides them up off the screen on `MORPH` when it isn't. Only the drawing moves, not the layout. Lifted and at rest, they aren't drawn and get no events. The notch and the grown edge (`Bezel`) aren't lifted at all: they're the screen's border, not the bar, so they stay even over a fullscreen window. A solid bar's band is the bar: it's a second `Bezel`, lifted with the islands, that slides away over them ([D-106](decisions.md), [D-130](decisions.md)).
3. `sync_shown` runs after every message. When `out` changes, it refits the surfaces. A hidden bar's input region is a `REVEAL_EDGE` strip at the top if it shows on hover, else nothing, plus the notch and the grown edge, which stay (`Bar::bezel_rects`, from the width the bezel reports, `Width`). Popovers, notifications and the dormer add their rects as before. On hover, the bar listens to the pointer: the top strip brings it out (`PointerAtTop`), coming back onto a bar that's still out keeps it (`PointerEntered`), and leaving hides it after `HIDE_AFTER` unless it comes back (`Unhover`). Coming onto the notch or the edge doesn't bring it out.
4. Over a fullscreen window the surfaces go on the `overlay` layer while the bar shows or waits for the pointer (`LayerChange`). When it's hidden on purpose, they go back to `top` once it has slid away (`Sink`, after `lift::slide_time`). The fullscreen window is then alone above them.
5. A bar shown on hover keeps an exclusive zone of 0, so windows don't move when it comes out.
6. In fullscreen, `Bar::held` keeps notifications out of the cards. They don't count down, since only shown cards do, and the panel opens only for the ones that aren't held.

## Dialogs

A widget's dialog is its panel in the middle of the screen ([D-90](decisions.md)):

1. The widget returns `open_dialog()`. If the widget's own popover is open, the dialog comes out of it, however it was asked for. Otherwise the host looks at what was last acted on (`Bar::touched`), set by an item press (`Touched`), a message or a row's shortcut from a popover or a notification card (`Acted`), or a dormer pick. If that was within a second, the dialog comes out of it. Otherwise it comes out of the top.
2. Picked in the dormer, the dormer turns into the dialog on the spot. Otherwise the host looks up how tall the focused screen is, as it does for the dormer (`Dialog(Opened(room))`). Opening it closes any popover. One open in the island acted on is dropped at once, without folding: the dialog's shape covers it as it leaves.
3. A dialog on screen, or one going back, takes the new one in where it is. It then goes back to where the new one came from, and the popover it came out of is dropped the same way (`Bar::take_popover`).
4. The island in the middle draws every dialog, with the key `DIALOG` and `Placement::Middle`, centered on the screen. Out of an island, its home is that island's shape as last drawn (`Bar::shapes`, `Island::shape`, `Popover::home`): it starts there, popover and all, and springs back onto it when it closes, while the island itself stays on the bar. If the dormer is still folding back in that island, the dialog doesn't start from the dormer's shape: it starts at its home (`Motion::rehome`). Out of the top, it comes with a drop, as the dormer does, and the notch squishes as the neck lets go and catches it (`Neck`). Without a notch, "the top" is the island in the middle, as for the dormer.
5. Placing, growing the surface, the input region, the keyboard and closing are the dormer's (`Placed`, `Closed`). It also closes when the widget returns `close_dialog()` or stops offering a dialog.

## Glass

With `theme = "glass"` and the `hypr-eave` Hyprland plugin loaded, the islands are glass ([D-112](decisions.md)). The plugin draws it; eave says where:

1. eave draws every island opaque, in its fill, as it would without the plugin.
2. The vendored layershellev publishes its layer surfaces' `wl_surface` (`layer_surfaces`). At the first frame, `glass::client` binds `eave_glass_v1` on the bar's connection, on an event queue of its own that a small thread dispatches.
3. As a frame is drawn, `glass::frame` gathers what it says:
   - Each island that may turn to glass records its shape, fill and which way its ink goes (`draw_island`, from `Motion::ink`). That's one not attached to an edge and not in a solid bar, and not a drop hanging from the notch.
   - Every other island records itself as opaque. Pictures record themselves with their rounded shape and opacity (`Picture::draw`, sent as `set_pictures`; solid records for a plugin before version 5, [D-129](decisions.md)).
   - The offsets of the island's motion, of `Lift` and of scrollables (`glass::Scrolled`) are added on the way.
   - The notch and the grown edge come from `Bar::bezel_rects`: a margin wider as solid, and exactly as bezel records, which the pointer goes under. Those are sent in every theme, glass or not ([D-113](decisions.md)).
   - The controls on an island record themselves as reliefs: wells and beads, with how far they're pressed, how milky, and the light under the finger ([D-126](decisions.md)). `Press`, the slider, tabs and the switcher do it when `glass::frame::on()`, and skip their own fills then. `frame::squeezed` follows the island's squeeze, as `translated` follows its motion.
4. `glass::Present`, last in the view, sends the records from an overlay, the last thing drawn (popovers draw in overlays), before iced commits the frame. The protocol applies them with that commit. `frame::fit` keeps them to 64, so the request fits in one Wayland message, and only the first bar's are sent. The reliefs go in `set_reliefs` (version 4), kept to 64 on an island by `frame::fit_reliefs`.
5. The plugin answers with `active` and `backdrop`, which arrive as `Message::Glass`. With glass, `Bar::darker_ink` (and `center_darker`, for the dormer's and dialogs' island) holds the islands over something bright, whose ink turns dark (`Island::ink`). The palette stays the dark one.
6. The plugin going away (unloaded, or turned off) sends `active(0)` or removes its global. The islands go back to dark, and nothing is sent.

## Data sources by widget

| Widget | Source | Mechanism |
|---|---|---|
| Workspaces, active window, monitors | Hyprland | `.socket2.sock` events + `.socket.sock` queries (our own client) |
| Clock | system time | 1 s timer (`eave_api::time::every`) |
| Battery | kernel (`/sys/class/power_supply`) | `power_supply` uevents over netlink, plus a 30 s re-read while a battery exists ([D-23](decisions.md)) |
| Volume | PipeWire (through pipewire-pulse) | PulseAudio native protocol, pure Rust, subscribed to sink/server events ([D-19](decisions.md)) |
| Brightness | kernel (`/sys/class/backlight`, `/sys/class/leds`) | `backlight` uevents over netlink; the keyboard's LED re-read every 2 s only while the popover is open; set through logind's `SetBrightness` ([D-80](decisions.md)) |
| Media | MPRIS players | D-Bus on the session bus: name changes, `PropertiesChanged` and `Seeked` from every player ([D-56](decisions.md)) |
| Network | kernel (rtnetlink, nl80211) | link/address/route multicast events, plus a 10 s signal re-read while on Wi-Fi ([D-23](decisions.md)) |
| Bluetooth | BlueZ | D-Bus on the system bus: every signal from `org.bluez` re-reads its objects ([D-103](decisions.md)) |
| Tray | StatusNotifierItem | D-Bus. We must act as the watcher/host. The hardest widget. |
| Notifications | apps, through the freedesktop notification spec | D-Bus: eave serves `org.freedesktop.Notifications` on the session bus, from the plugin in [eaveshell/eave-notifications](https://github.com/eaveshell/eave-notifications) ([D-48](decisions.md), [D-50](decisions.md)) |
| CPU / RAM / temps | `/proc`, `/sys` | timer (the only legitimate polling) |

So far: clock, workspaces, active window, keyboard layout, battery, network, volume, brightness, media, notifications and Bluetooth. The tray is next. See [roadmap.md](roadmap.md).

## Code layout

```
Cargo.toml                 workspace, release profile
crates/eave-api/           the plugin SDK: what plugin authors depend on
  src/widget.rs            the Widget trait
  src/ui.rs                the closed vocabulary: Node, Item, Panel, Notice, Action, Tone
  src/effect.rs            Effect: a Task plus popover and dialog requests
  src/registry.rs          Registry: widget name → constructor
  src/erased.rs            type erasure for the host (doc-hidden)
crates/eave/               the host, as a library (`eave::run`) plus the default binary
  src/main.rs              default binary: host + built-in plugins
  src/lib.rs               run(): registry, layer-shell settings, daemon
  src/app.rs               state, Message, update/view/subscription, popover lifecycle, the notifications panel and its clocks, surface fit, reload
  src/config.rs            config.toml → zone entries, with their island, divider and collapsible
  src/watch.rs             inotify watch on the config directory
  src/doctor/              eave doctor: checks config.toml, widgets, keys, the screen, hypr-eave, Hyprland's layer animation and systemd; text and TOML report
  src/kit.rs               ui nodes → iced elements, styled
  src/icon.rs              status icons (volume, Wi-Fi, wired, battery, brightness, media) as vector paths
  src/apps.rs              app icons: desktop entry → icon theme → 32 px bitmap, off the UI thread
  src/cover.rs             covers: a file or https URL → cropped, rounded 32 and 256 px bitmaps, off the UI thread
  src/dialog.rs            a widget's dialog: what turns into it, and what was last acted on
  src/dormer.rs            the dormer's list: what widgets offer, ranked for what's typed, the selection and recent picks
  src/pour.rs              how the dormer's drop flows out of the notch's bottom: the smooth union of the two, row by row
  src/theme.rs             every color, size and font
  src/island.rs            island: item row, morph springs, press squeeze and drag, popover overlay, switching popovers mid-morph, the collapse chevron on hover
  src/squish.rs            how squished the items being drawn are, set by their island
  src/label.rs             a label that can squish (outlines while squished)
  src/squircle.rs          the squircle path
  src/jelly.rs             an island's outline, its skin: a ring of coupled springs that keeps its area, poked by nudges, and the shelves nudges hold out
  src/join.rs              two shapes as one while they're close, when asked: a dialog and the island it comes out of (D-122)
  src/body.rs              an island's outline while it morphs: a ring of points that pours ahead, hangs on, bounces and lands (D-118)
  src/wall.rs              the screen's edges as walls: how far an island can go toward each before it's too close
  src/hand.rs              what a drag does to a shape: the rubber band, and the stretch around the grab
  src/glass/               glass with the hypr-eave plugin: the protocol client, what each frame says, which way the ink goes (D-112)
  src/lift.rs              the bar sliding up off the screen and back, drawn lifted on a spring
  src/zones.rs             the three zones across the bar: sides at the edges, the center pushed aside when they reach it, or split around the notch
  src/bezel.rs             the notch and the grown edge (the whole bar when it's solid): squished by the dormer's drop, stretched by a drag, a click brings the dormer out
  src/slot.rs              item wrapper: clicks, scroll steps, and the pointer for alternates
  src/alternate.rs         alternate: one option at a time with fades; hover, and growing (expand)
  src/handover.rs          one thing on screen at a time: fade (and slide) out, then the next in; alternates and tabs
  src/snapshot.rs          pictures of what widgets draw, taken offscreen, sharp and blurred: a popover's content crossing its island's (D-124)
  src/switcher.rs          switcher: row of options with a sliding, frame-paced mark
  src/tabs.rs              tabs: a pill strip of names over the current page, which the host owns; pages hand over
  src/toggle.rs            toggle: a pill whose knob slides over
  src/meter.rs             meter: a thin bar eased to its value, with an optional limit tick
  src/slider.rs            slider: a line with no knob that thickens under the pointer
  src/field.rs             text field box, its focus on appearing, and the focus query popovers use
  src/clip.rs              a line of text cut at its room, fading at the end
  src/ease.rs              a number eased to each new value on the frame clock
crates/eave-build/         builds the widgets config.toml uses, installs and restarts; `update`, `check`, `init`
crates/eave-dbus/          service: shared system and session buses, snapshot-on-PropertiesChanged watcher; systemd units
crates/eave-mpris/         service: MPRIS media players on the session bus, and their controls
crates/eave-iwd/           service: Wi-Fi networks and controls through iwd's D-Bus API, and a passphrase agent
crates/eave-bluez/         service: Bluetooth adapter and devices through BlueZ's D-Bus API, and a pairing agent
crates/eave-net/           service: network state from the kernel over netlink (one event thread)
crates/eave-pulse/         service: audio over the PulseAudio protocol (one event thread)
crates/eave-hyprland/      service: shared socket2 events, socket1 queries and dispatches
eave-collection.toml       lists plugins/*: the built-ins, as one collection (D-46)
plugins/apps/              installed apps as actions in the dormer, from desktop entries (eave-apps)
plugins/clock/             the clock plugin (crate eave-clock) + its eave-plugin.toml
plugins/essential/         the bar's own actions in the dormer: calibrate, restart, health (eave-essential)
plugins/update/            check for updates and update, through eave-build's units (eave-update)
plugins/workspaces/        Hyprland workspaces (eave-workspaces)
plugins/window/            focused window title (eave-window)
plugins/keyboard/          keyboard layout (eave-keyboard)
plugins/media/             now playing over MPRIS, with cover art (eave-media)
plugins/battery/           battery from sysfs (eave-battery)
plugins/bluetooth/         Bluetooth devices, connecting and pairing, via eave-bluez (eave-bluetooth)
plugins/brightness/        display and keyboard backlights from sysfs, set through logind (eave-brightness)
plugins/network/           primary connection, via eave-net; Wi-Fi to join via eave-iwd (eave-network)
plugins/volume/            output volume and switching (eave-volume)
hypr-eave/                the Hyprland plugin that draws glass and puts the pointer under the notch (C++), and its smoke test (D-112, D-113)
protocol/                 eave-glass-v1.xml, the protocol between eave and hypr-eave (D-112)
vendor/softbuffer/         softbuffer 0.4.8 with an alpha-carrying Wayland buffer (D-25)
vendor/iced_tiny_skia/     iced_tiny_skia 0.14.1: lazy clip mask, damage-aware present, EAVE_FRAMES (D-66)
vendor/wgpu-hal/           wgpu-hal 27.0.4: the GL resolve blit ignores the scissor (D-120)
dist/systemd/              user units: the bar, the rebuild-on-config.toml pair, and the update and check
dist/config.toml           the starter config `eave-build init` writes
install.sh                 the curl | sh installer
```

The rule: **split a file when it gets a second responsibility, not before.** Each service is its own crate (`eave-hyprland`, later a D-Bus one), so a plugin only compiles in the sources it uses.

## Things to keep in mind

- `#[to_layer_message(multi)]` on `Message` adds the variants `iced_layershell` uses to create and control surfaces (`SizeChange`, `SetInputRegion`, `NewLayerShell`, `RemoveWindow`, …). `update` must handle them with a catch-all arm.
- Margin tuples are `(top, right, bottom, left)`, despite the macro's doc comment.
- Surfaces are transparent. The islands (popovers included) draw their own backgrounds, which is what gives them rounded corners and lets the wallpaper show around the islands. With the CPU renderer that needs our patched softbuffer ([D-25](decisions.md)).
- `KeyboardInteractivity::None`, except while a popover is open: then its bar takes the keyboard (`Exclusive`), any key closes the popover, and keys stay with a focused text field in it ([D-36](decisions.md), [D-41](decisions.md)). A row's shortcut presses it instead, and a modifier alone keeps it open ([D-91](decisions.md)).
- The bar opens on the active output (`StartMode` default). In daemon mode, every window is drawn as a bar, and popovers remember which bar they grew on, so one bar per monitor (`StartMode::AllScreens`) mostly needs per-output state.
