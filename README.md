# eave

A status bar for [Hyprland](https://hyprland.org), written from scratch in Rust. Floating islands with workspaces, the focused window, a clock, volume, network and battery. Animations follow the compositor's frame clock, the bar redraws only when something changed, and it idles at about 14 MB and 0% CPU.

Everything in it is a plugin, compiled in rather than loaded at runtime. Plugins are git repositories you name in your config.

## Install

You need Hyprland, Rust (`cargo`) and `git`:

```sh
curl -fsSL https://raw.githubusercontent.com/francescomengarelli/eave/main/install.sh | sh
```

This installs `eave-build`, writes a starter `~/.config/eave/config.toml`, builds the bar into `~/.local/bin/eave` and starts it as a systemd user service. If your session doesn't use [uwsm](https://wiki.hyprland.org/Useful-Utilities/Systemd-start/), it tells you the one line to add to your Hyprland config instead. Read [the script](install.sh) first if you like: it's short. [docs/running.md](docs/running.md) has the manual steps.

## Configure

`~/.config/eave/config.toml` is the only file. Changes apply as you save:

```toml
[[left]]
widget = "workspaces"
persistent = 5

[[center]]
widget = "clock"
format = "%a %d %b  %H:%M"

[[right]]
widget = "volume"

[[right]]
widget = "someone/weather@v0.2"   # a plugin from github.com/someone/weather, pinned
```

Built-in widgets are `workspaces`, `window`, `clock`, `keyboard`, `volume`, `network` and `battery`. Anything else is a plugin: `owner/repo` on GitHub, any git URL, or a local path. When the config starts using a new one, the bar is rebuilt with it in the background. See [docs/plugins.md](docs/plugins.md) for the options and for writing your own.

## Update

```sh
eave-build update
```

Nothing changes version on its own. This fetches eave and every plugin, rebuilds and restarts the bar. Pin a version with `@tag` to stay on it.

## Uninstall

```sh
systemctl --user disable --now eave.service eave-rebuild.path
rm ~/.config/systemd/user/eave{,-rebuild}.* ~/.local/bin/eave
rm -r ~/.local/share/eave ~/.cache/eave
cargo uninstall eave-build
```

## More

- [docs/vision.md](docs/vision.md): what eave is for, and what it won't do
- [docs/architecture.md](docs/architecture.md): how it works
- [docs/development.md](docs/development.md): building from a checkout, measuring
- [docs/decisions.md](docs/decisions.md): every significant choice, and why

MIT licensed.
