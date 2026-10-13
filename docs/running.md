# Running eave

How to install the bar, start it with your session, update it, and what each kind of edit needs. The default is a systemd user service ([D-20](decisions.md)). Starting it from the Hyprland config works too.

## Install

### From the AUR (Arch, CachyOS)

```sh
paru -S eave
```

The package holds only `eave-build`, in `/usr/bin`, and pulls in `rust`, `git` and `pkgconf` to build the bar with ([D-54](decisions.md)). Then, as your user, the same steps as below from step 2:

```sh
eave-build init
eave-build
systemctl --user enable --now eave-rebuild.path eave.service
```

### With install.sh

You need Rust (`cargo`) and `git`:

```sh
curl -fsSL https://raw.githubusercontent.com/eaveshell/eave/main/install.sh | sh
```

[`install.sh`](../install.sh) does these steps, and you can do them by hand too:

1. `cargo install --locked --git https://github.com/eaveshell/eave eave-build`
2. `eave-build init`: writes a starter `~/.config/eave/config.toml` (an existing one is kept) and the systemd user units into `~/.config/systemd/user/`.
3. `eave-build`: clones eave into `~/.local/share/eave/src`, builds the widgets config.toml uses, and installs `~/.local/bin/eave`. The first build takes a few minutes.
4. `systemctl --user enable --now eave-rebuild.path`, and `eave.service` when `graphical-session.target` is active (see [below](#as-a-systemd-user-service-recommended)). Otherwise it tells you what to add instead.

Running it again repairs an install and never updates one. `EAVE_REPO=<git URL or checkout>` installs from a fork or a local checkout.

## What an edit needs

| You edit | What happens |
|---|---|
| `~/.config/eave/config.toml`, moving widgets or changing options | Nothing to do. The bar reloads it as soon as you save ([D-21](decisions.md)). Widgets whose entry didn't change keep their state. A file that doesn't parse is reported on stderr and the current bar stays. |
| `~/.config/eave/config.toml`, `renderer` | Used from the bar's next start: `systemctl --user restart eave.service` ([D-67](decisions.md)). |
| `~/.config/eave/config.toml`, `notch`, `screen` or `islands` | Nothing to do: the bar reloads it ([D-71](decisions.md), [D-73](decisions.md)). |
| `~/.config/eave/config.toml`, using a widget that isn't compiled in yet | A rebuild: `eave-build`, run for you by the [path unit](#rebuild-automatically-when-configtoml-changes) ([D-22](decisions.md), [D-30](decisions.md)). |

`eave-build` builds, and if the new binary differs from `~/.local/bin/eave`, installs it and restarts a running bar on it. It uses `systemctl --user restart eave.service` when the service runs the bar. Otherwise it stops the bar and relaunches it through Hyprland. A failed build installs nothing and leaves the running bar alone. If no bar is running, it doesn't start one.

## Update

```sh
eave-build update
```

Nothing changes version until you run this ([D-31](decisions.md)). It fetches eave and every plugin, moves each to the newest commit of its `@rev` (or its default branch), prints what moved, and rebuilds as above. When eave itself moved, it also reinstalls `eave-build` from the new checkout and runs its `init`, which rewrites the systemd units (config.toml is kept) and reloads systemd. To stay on a version, pin it: `eave = "eaveshell/eave@<tag>"`, `widget = "someone/weather@v0.2"`.

With the AUR package, eave itself is pinned to the package's release: a config.toml without `eave = …` builds `eaveshell/eave@v<version>`. `paru -Syu` moves eave and `eave-build` together, and the next `eave-build` moves the bar to the new release. Run `eave-build init` after an upgrade to refresh the units. `eave-build update` still updates your plugins, and never reinstalls `eave-build` over the package ([D-54](decisions.md)).

## Uninstall

```sh
systemctl --user disable --now eave.service eave-rebuild.path
rm ~/.config/systemd/user/eave{,-rebuild}.* ~/.local/bin/eave
rm -r ~/.local/share/eave ~/.cache/eave     # sources and build cache
cargo uninstall eave-build                  # or, from the AUR: pacman -R eave
```

`~/.config/eave` is yours to keep or remove.

## As a systemd user service (recommended)

The release build aborts on a panic ([D-10](decisions.md)). Under systemd, `Restart=on-failure` brings the bar back, logs go to the journal (`journalctl --user -u eave`), and `systemctl --user status eave` shows its memory.

```fish
systemctl --user enable --now eave.service   # eave-build init put the unit in place
```

The unit starts with `graphical-session.target` and needs the session's environment. Two things must be true:

1. **systemd knows `WAYLAND_DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE`.** Without them the service can't find the compositor. The unit has `ConditionEnvironment=WAYLAND_DISPLAY`, so it's skipped instead of crash-looping. Check with `systemctl --user show-environment`.
2. **`graphical-session.target` is active.** Check with `systemctl --user is-active graphical-session.target`.

**With [uwsm](https://wiki.hyprland.org/Useful-Utilities/Systemd-start/)** (Hyprland's recommended launcher) both are taken care of. Nothing else to do.

**Without uwsm**, nothing starts `graphical-session.target`, and it can't be started by hand. Add a target that pulls it in, `~/.config/systemd/user/hyprland-session.target`:

```ini
[Unit]
Description=Hyprland session
BindsTo=graphical-session.target
Wants=graphical-session-pre.target
After=graphical-session-pre.target
```

Then import the environment and start the target from the Hyprland config. Use one command, so the import finishes before the target starts:

```lua
-- hyprland.lua (Hyprland 0.56+)
hl.on("hyprland.start", function()
	hl.exec_cmd("dbus-update-activation-environment --systemd --all && systemctl --user start hyprland-session.target")
end)
```

```ini
# hyprland.conf (older Hyprland)
exec-once = dbus-update-activation-environment --systemd --all && systemctl --user start hyprland-session.target
```

Remove any line that starts eave directly, or you'll get two bars.

Without uwsm, nothing stops the target when Hyprland exits. That only matters if you log out and back in without rebooting: the old `eave.service` keeps the old environment. `systemctl --user stop hyprland-session.target` before exiting fixes it.

## From the Hyprland config

No systemd involved. The catch is that nothing restarts the bar if it crashes.

```lua
-- hyprland.lua (Hyprland 0.56+)
hl.on("hyprland.start", function()
	hl.exec_cmd("~/.local/bin/eave")
end)
```

```ini
# hyprland.conf (older Hyprland)
exec-once = ~/.local/bin/eave
```

`eave-build` restarts the bar in this setup too. It finds the bar by its binary (`~/.local/bin/eave`), stops it, and relaunches it through Hyprland's request socket with `eval hl.exec_cmd(…)`, falling back to `dispatch exec` on older Hyprland. It doesn't need `hyprctl` or `kill` installed. That way the new bar gets Hyprland's environment and doesn't belong to your terminal. A dev build running from `target/` is left alone.

## Rebuild automatically when config.toml changes

A systemd path unit runs `eave-build` whenever `config.toml` changes. The installer enables it. By hand:

```fish
systemctl --user enable --now eave-rebuild.path   # eave-build init put the units in place
```

- A save that doesn't change which widgets are used finds nothing to compile, installs nothing and doesn't restart the bar. It costs a cargo freshness check at idle priority, about a second.
- The rebuild runs at idle CPU and I/O priority, so it never makes the desktop stutter.
- A failed build sends a notification (`notify-send`, or Hyprland's own `hyprctl notify` when no notification daemon is running). The old bar keeps running. Details are in `journalctl --user -u eave-rebuild`.
- Saving while a build runs doesn't start a second build, and that save isn't queued either. If you started using another widget mid-build, save again once the build is done.
- It works whether the bar runs as the service or from the Hyprland config. In the second case it relaunches the bar through Hyprland's socket. If systemd's environment has no `HYPRLAND_INSTANCE_SIGNATURE`, it uses the one Hyprland that's running, and it gives up (leaving the old bar running) only when there are several.
- `eave-build init` writes the real path into `PathChanged=` when `XDG_CONFIG_HOME` isn't `~/.config`.
- `eave-build init` writes its own full path into `ExecStart=`. systemd looks up a bare command name only in `/usr/bin` and `/usr/local/bin`, not in `~/.cargo/bin`.

## Calibrate the screen

`eave calibrate` measures how your screen's corners are rounded, and its notch, if it has one, so the bar fits them exactly ([D-71](decisions.md)). It covers the focused output with a model of what the screen shows: a one-pixel red line just inside its edge, and blue over what it says is hidden. Change the model until you can see every red pixel and no blue one. Look closely at the corners and around the notch.

| Key | Does |
|---|---|
| ↑ ↓, `k` `j`, Tab, Shift+Tab | pick a row |
| ← →, `h` `l` | change it by a pixel (smoothing by 0.05; 0 is a circle's corner, more a squircle's) |
| Shift with ← → | ten pixels (0.2 smoothing) |
| Space | turn the notch on or off, or move it between hanging from the top and a cut-out below it |
| `?` | fold the panel away, and back |
| Enter, `s` | save to config.toml and quit |
| Esc, `q` | quit without saving |

It starts from what config.toml says, or from the notch the hardware reports. Saving changes only `notch` and `screen`, keeps the notch's and the screen's `grow` as they were, keeps the rest of the file and its comments, and writes through a symlink. Run it with the binary the bar runs, `~/.local/bin/eave calibrate`, or `cargo run --release -- calibrate` in a checkout.

## Notifications

With `widget = "eaveshell/eave-notifications"` in config.toml, eave is your notification daemon ([D-48](decisions.md), [D-50](decisions.md)). Only one program can be. If mako, dunst or swaync is already running, eave waits in line and takes over as soon as it quits. Until then, the bell is dimmed and the popover says which daemon has them. To switch, stop starting the other daemon (remove its `exec-once`, or `systemctl --user disable --now mako`), then quit it.

Notifications sent while the bar restarts, after a rebuild for example, are lost, and the list starts empty after a restart.
