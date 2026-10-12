# Running eave

How to start the bar with your session, and what each kind of edit needs. The default is a systemd user service ([D-16](decisions.md)). Starting it from the Hyprland config works too.

## What an edit needs

| You edit | What happens |
|---|---|
| `~/.config/eave/config.toml` (what goes where) | Nothing to do. The bar reloads it as soon as you save ([D-17](decisions.md)). Widgets whose entry didn't change keep their state. A file that doesn't parse is reported on stderr and the current bar stays. |
| `~/.config/eave/plugins.toml` (what's compiled in) | A rebuild: `eave-build --restart`, or automatically with the [path unit](#rebuild-automatically-when-pluginstoml-changes) ([D-18](decisions.md)). |

`eave-build --restart` builds, installs to `~/.local/bin/eave`, then restarts a running bar on the new binary. It uses `systemctl --user restart eave.service` when the service runs the bar. Otherwise it stops the bar and relaunches it through Hyprland. A failed build installs nothing and leaves the running bar alone. If no bar is running, it doesn't start one.

To have `eave-build` on your `PATH` (the path unit needs this):

```fish
cargo install --path crates/eave-build
```

## As a systemd user service (recommended)

The release build aborts on a panic ([D-10](decisions.md)). Under systemd, `Restart=on-failure` brings the bar back, logs go to the journal (`journalctl --user -u eave`), and `systemctl --user status eave` shows its memory.

```fish
cp dist/systemd/eave.service ~/.config/systemd/user/
systemctl --user enable --now eave.service
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

`eave-build --restart` handles this setup too. It finds the bar by its binary (`~/.local/bin/eave`), stops it, and relaunches it with `hyprctl eval 'hl.exec_cmd(…)'`, falling back to `hyprctl dispatch exec` on older Hyprland. That way the new bar gets Hyprland's environment and doesn't belong to your terminal. A dev build running from `target/` is left alone.

## Rebuild automatically when plugins.toml changes

Optional. A systemd path unit runs `eave-build --restart` whenever `plugins.toml` changes:

```fish
cargo install --path crates/eave-build
cp dist/systemd/eave-rebuild.{path,service} ~/.config/systemd/user/
systemctl --user enable --now eave-rebuild.path
```

- The rebuild runs at idle CPU and I/O priority, so it never makes the desktop stutter.
- A failed build sends a notification (`notify-send`, or Hyprland's own `hyprctl notify` when no notification daemon is running). The old bar keeps running. Details are in `journalctl --user -u eave-rebuild`.
- Saving while a build runs doesn't start a second build, and that save isn't queued either. If you changed `plugins.toml` mid-build, save it again once the build is done.
- It works whether the bar runs as the service or from the Hyprland config. In the second case it needs `HYPRLAND_INSTANCE_SIGNATURE` in systemd's environment, as above.
- If `XDG_CONFIG_HOME` isn't `~/.config`, edit `PathChanged=` in `eave-rebuild.path`.
