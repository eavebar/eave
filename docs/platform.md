# Platform targets

## Must support

| Target | Arch | Notes |
|---|---|---|
| **Arch Linux on Asahi** (Apple Silicon MacBook) | aarch64 | Primary dev machine |
| **CachyOS** | x86_64 (v3/v4-optimized repos) | Desktop |
| **Hyprland** | — | The only compositor we target |

Both are Arch-based, but nothing in the bar depends on Arch. It uses Wayland protocols, Hyprland IPC and freedesktop D-Bus APIs, which are the same on every distro. Being "Arch only" would only affect packaging, so we don't restrict anything in the code.

## Why these constraints shape the code

### Two architectures
Everything must build from source on both aarch64 and x86_64. In practice:
- No dependencies that only ship prebuilt x86 binaries.
- When adding an AUR or system dependency, check that its `arch=()` includes `aarch64`.
- No inline assembly or arch-specific intrinsics without a portable fallback.
- Test any change to renderer or GPU code on both machines. The GPU stacks are very different: Asahi's Mesa drivers versus Intel/AMD/NVIDIA.

### Asahi specifics
- **GPU:** Asahi's Mesa drivers provide OpenGL and Vulkan, so wgpu works. The GPU renderer is the default, with the CPU one as a fallback and as `renderer = "cpu"` (see [decisions.md](decisions.md), D-67).
- **HiDPI / fractional scaling:** the MacBook panel runs at a non-1 scale, so the bar must render crisp text at fractional scales. If text ever looks blurry, check whether the fractional-scale and viewporter protocols are being used.
- **The notch:** by default Asahi hides the notch area, so the usable screen is just shorter. If it's enabled with the `apple_dcp.show_notch=1` kernel parameter, the top-center of the display is physically missing. The bar should then leave the center empty on that output. This is planned, not implemented yet.
- **Battery and backlight** are read from `/sys/class/power_supply` and `/sys/class/backlight`. No Apple-specific code. `macsmc-battery` reports `charge_*` and `energy_*`, and its `capacity` is the firmware's rounded-up display value, so the percentage comes from the energy ratio instead ([D-23](decisions.md)).

### CachyOS specifics
- Usually a desktop with one or more external monitors, so multi-monitor support (one bar per output) matters more there.
- Repos are built for x86-64-v3/v4. That doesn't affect us, since we build from source with the system toolchain.

## Runtime requirements

The bar must run on any Hyprland machine without checking what else is installed ([D-23](decisions.md)). What it needs at runtime:

- **Hyprland** (its IPC sockets).
- **A font.** Text is shaped with whatever fonts the system has (fontdb reads fontconfig's directories).
- **For volume only:** a PulseAudio-protocol server, meaning `pipewire-pulse` or PulseAudio. If none is running, the widget hides ([D-19](decisions.md)).

That's all. Battery and network read the kernel directly. The binary links only libc, and no plugin runs other programs. `eave-build` needs `cargo` and `git`, and uses `systemctl` only when systemd is there.

## Hyprland interface we rely on

- **`wlr-layer-shell`** for the bar surface itself (anchor, layer, exclusive zone).
- **Hyprland IPC sockets** in `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`:
  - `.socket.sock`: request/response (same as `hyprctl -j ...`)
  - `.socket2.sock`: newline-separated event stream, `EVENT>>DATA`

- **`eave-build`** relaunches a bar started from the Hyprland config by writing to `.socket.sock` itself (no `hyprctl`). Hyprland 0.56 moved to Lua configs, where `dispatch <args>` is evaluated as Lua (`hl.dispatch(<args>)`), so it sends `eval hl.exec_cmd(…)` first and falls back to `dispatch exec` on older releases ([D-22](decisions.md)).

Hyprland's IPC changes between releases more often than freedesktop APIs do. Keep parsing tolerant: ignore unknown events, and don't crash on extra fields.
