# Development

## Requirements

- Rust (stable, edition 2024). Developed with 1.97.
- A running Hyprland session (the bar needs a Wayland compositor with layer-shell).
- System libraries: `wayland`, `libxkbcommon`. For the `gpu` feature, also a Vulkan loader plus drivers (Mesa on Asahi).

## Build and run

```fish
cargo run                          # debug, default binary (host + built-in plugins)
cargo run --release                # what you'd actually use
cargo run --release --features gpu # wgpu renderer instead of tiny-skia
cargo test --workspace             # every crate
cargo run --release -p eave-build -- --install   # build with the plugins in plugins.toml
```

Without `~/.config/eave/config.toml` the bar is empty. A minimal config:

```toml
[[center]]
widget = "clock"
```

To try a config without touching your own, point `XDG_CONFIG_HOME` at another directory. See [plugins.md](plugins.md) for plugins and the config format.

Stop with Ctrl+C. To check that the surface exists and where it is:

```fish
hyprctl layers | grep eave
```

To watch the Hyprland event stream the workspace module will consume:

```fish
socat -U - UNIX-CONNECT:$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock
```

## Measuring

"Lightweight" is checked with numbers ([vision.md](vision.md)). Before and after any change to dependencies, renderer, animation or polling:

```fish
cargo build --release
ls -lh target/release/eave                     # binary size
./target/release/eave &
sleep 5
ps -o rss,pcpu -p (pgrep -x eave)              # memory (KB) and CPU
kill (pgrep -x eave)
```

Current baseline (Asahi, aarch64, release, tiny-skia, default binary). Measured 2026-10-12 after the plugin foundation (0.5):

| Config | Binary | RSS | Idle CPU |
|---|---|---|---|
| empty bar (no config) | 5.8 MiB | ~9.7 MB | 0% |
| two clocks | 5.8 MiB | ~11.8 MB | 0% |
| two clocks, popover open | | ~14.5 MB | |

Before the plugin split (skeleton, clock + date hard-coded): binary 5.6 MiB, RSS ~11.5 MB. The +0.2 MiB of binary and +0.3 MB of RSS are the price of the registry, config parsing (`toml`) and popover surfaces ([D-12](decisions.md), [D-14](decisions.md)).

The wgpu renderer measured 9.5 MB binary, ~72 MB RSS and ~1% idle CPU at the skeleton stage ([D-4](decisions.md)).

Update this table when the baseline moves for a good reason, and record that reason in [decisions.md](decisions.md).

For smoothness: watch animations with Hyprland's debug overlay (`hyprctl keyword debug:overlay true`) and check that the bar's CPU only rises *while* something is animating.

## Code rules

- **Events over polling.** If the data source can notify us, subscribe. Polling needs a comment explaining why no event source exists.
- **`update` does no I/O.** It mutates state and returns `Task`s. Blocking calls belong in subscriptions or tasks.
- **`view` is pure.** No side effects, no expensive computation. Precompute in `update` if needed.
- **One place for visual constants:** colors, sizes and fonts all live in `crates/eave/src/theme.rs`. Plugins never see them ([D-13](decisions.md)).
- **Dependencies:** `default-features = false` unless there's a reason. Every new crate gets one line in the PR or commit explaining why. Non-obvious ones get a `decisions.md` entry.
- **Tolerant parsing** of anything external (Hyprland events, D-Bus properties). Unknown is ignored and logged at debug level, never a panic.
- **Both arches build.** If you touch anything platform-dependent, note whether it was tested on x86_64 too.
- Match the surrounding code style. `cargo fmt` and `cargo clippy` should be clean.

## Docs

`AGENTS.md` is the entry point. `docs/` holds the reasoning. When behavior or a decision changes, update the relevant doc in the same change.
