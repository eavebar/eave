#!/bin/sh
# Installs eave, a status bar for Hyprland: https://github.com/francescomengarelli/eave
#
#   curl -fsSL https://raw.githubusercontent.com/francescomengarelli/eave/main/install.sh | sh
#
# It installs eave-build (with cargo), writes a starter ~/.config/eave/config.toml if you have
# none, builds the bar into ~/.local/bin/eave and sets up the systemd user units. Running it again
# repairs an install and never updates one: that's `eave-build update` (D-31).
#
# EAVE_REPO=<git URL or local checkout> installs from somewhere else, such as a fork.
set -eu

repo=${EAVE_REPO:-https://github.com/francescomengarelli/eave}
config=${XDG_CONFIG_HOME:-$HOME/.config}/eave/config.toml
eave_build=${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/eave-build

say() { printf 'eave: %s\n' "$*"; }
die() {
	say "$*" >&2
	exit 1
}

[ "$(uname -s)" = Linux ] || die "eave runs on Linux, under Hyprland"
command -v cargo >/dev/null || die "needs cargo: https://rustup.rs, or your distribution's rust package"
command -v git >/dev/null || die "needs git"
[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || say "Hyprland doesn't seem to be running here; installing anyway"

# 1. eave-build, which builds and updates the bar.
if [ -x "$eave_build" ]; then
	say "eave-build is already installed (eave-build update updates it)"
elif [ -d "$repo" ]; then
	cargo install --locked --path "$repo/crates/eave-build"
else
	cargo install --locked --git "$repo" eave-build
fi

# 2. config.toml and the systemd units. An existing config.toml is kept.
had_config=$([ -e "$config" ] && echo yes || echo no)
"$eave_build" init --config "$config"
if [ "$had_config" = no ] && [ -n "${EAVE_REPO:-}" ]; then
	# Build eave from where it was installed from, not the main repository.
	tmp=$(mktemp)
	{ printf 'eave = "%s"\n\n' "$repo"; cat "$config"; } >"$tmp"
	mv "$tmp" "$config"
fi

# 3. The bar itself. The first build takes a few minutes; later ones are incremental.
"$eave_build" --config "$config"

# 4. Start it with the session.
running_outside_systemd() {
	! systemctl --user is-active --quiet eave.service 2>/dev/null &&
		[ -n "$(pgrep -x eave 2>/dev/null || true)" ]
}
if command -v systemctl >/dev/null && systemctl --user show-environment >/dev/null 2>&1; then
	systemctl --user daemon-reload
	# Rebuilds the bar when config.toml starts using another plugin.
	systemctl --user enable --now eave-rebuild.path
	if running_outside_systemd; then
		say "eave is already running outside systemd (from your Hyprland config?), so the"
		say "service stays off. To switch, remove that line and run:"
		say "  systemctl --user enable --now eave.service"
	elif systemctl --user is-active --quiet graphical-session.target; then
		systemctl --user enable --now eave.service
		say "eave runs as a systemd user service: journalctl --user -u eave for its logs"
	else
		say "graphical-session.target isn't active (no uwsm?), so eave.service can't start yet."
		say "Either start the bar from your Hyprland config:"
		say "  exec-once = ~/.local/bin/eave"
		say "or make the session start graphical-session.target, then"
		say "  systemctl --user enable --now eave.service"
		say "Both are explained in docs/running.md."
	fi
else
	say "no systemd user session: start ~/.local/bin/eave from your Hyprland config"
fi

say "done. Edit $config: it applies as you save."
if [ "$(command -v eave-build)" = "$eave_build" ]; then
	say "Update with: eave-build update"
else
	say "Update with: $eave_build update"
	say "($(dirname "$eave_build") isn't on your PATH: add it to call eave-build by name)"
fi
