# Development

## Requirements

- Rust (stable, edition 2024). Developed with 1.97.
- A running Hyprland session (the bar needs a Wayland compositor with layer-shell).
- System libraries: `wayland`, `libxkbcommon`. For the `gpu` feature, also a Vulkan loader plus drivers (Mesa on Asahi).

## Build and run

```fish
cargo run                          # debug
cargo run --release                # what you'd actually use
cargo run --release --features gpu # wgpu renderer instead of tiny-skia
```

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

Current baseline (Asahi, aarch64, release, clock only):

| | Binary | RSS | Idle CPU |
|---|---|---|---|
| tiny-skia | 5.6 MB | ~11.5 MB | ~0.2% |
| wgpu | 9.5 MB | ~72 MB | ~1% |

Update this table when the baseline moves for a good reason, and record that reason in [decisions.md](decisions.md).

For smoothness: watch animations with Hyprland's debug overlay (`hyprctl keyword debug:overlay true`) and check that the bar's CPU only rises *while* something is animating.

## Code rules

- **Events over polling.** If the data source can notify us, subscribe. Polling needs a comment explaining why no event source exists.
- **`update` does no I/O.** It mutates state and returns `Task`s. Blocking calls belong in subscriptions or tasks.
- **`view` is pure.** No side effects, no expensive computation. Precompute in `update` if needed.
- **One place for visual constants:** colors, sizes and fonts. Today that's the constants at the top of `app.rs`, and later `theme.rs`.
- **Dependencies:** `default-features = false` unless there's a reason. Every new crate gets one line in the PR or commit explaining why. Non-obvious ones get a `decisions.md` entry.
- **Tolerant parsing** of anything external (Hyprland events, D-Bus properties). Unknown is ignored and logged at debug level, never a panic.
- **Both arches build.** If you touch anything platform-dependent, note whether it was tested on x86_64 too.
- Match the surrounding code style. `cargo fmt` and `cargo clippy` should be clean.

## Docs

`AGENTS.md` is the entry point. `docs/` holds the reasoning. When behavior or a decision changes, update the relevant doc in the same change.
