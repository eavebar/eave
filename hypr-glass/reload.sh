#!/bin/sh
# Rebuild and reload into the Hyprland in $HYPRLAND_INSTANCE_SIGNATURE (the nested one).
# dlopen hands back the old code for a path it already has, so each load gets a new name.
set -e
cd "$(dirname "$0")"
make -s
last="${XDG_RUNTIME_DIR:-/tmp}/eave-glass.last"
if [ -f "$last" ]; then hyprctl plugin unload "$(cat "$last")" || true; else hyprctl plugin unload "$PWD/eave-glass.so" || true; fi
new="${XDG_RUNTIME_DIR:-/tmp}/eave-glass-$(date +%s%N).so"
cp eave-glass.so "$new"
echo "$new" > "$last"
hyprctl plugin load "$new"
