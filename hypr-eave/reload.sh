#!/bin/sh
# Rebuild and reload into the Hyprland in $HYPRLAND_INSTANCE_SIGNATURE (the nested one).
# dlopen hands back the old code for a path it already has, so each load gets a new name.
set -e
cd "$(dirname "$0")"
make -s
last="${XDG_RUNTIME_DIR:-/tmp}/hypr-eave.last"
if [ -f "$last" ]; then hyprctl plugin unload "$(cat "$last")" || true; else hyprctl plugin unload "$PWD/hypr-eave.so" || true; fi
new="${XDG_RUNTIME_DIR:-/tmp}/hypr-eave-$(date +%s%N).so"
cp hypr-eave.so "$new"
echo "$new" > "$last"
hyprctl plugin load "$new"
