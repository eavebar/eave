#!/bin/sh
# Smoke test: hypr-eave in a nested Hyprland (a window in the current session), with this
# checkout's eave. Loads the plugin, checks eave gets glass, reloads it many times, turns the
# monitor, adds an output and kills eave mid-frame: after each, the nested Hyprland must still
# answer. Nothing touches the session it runs in.
#
#   hypr-eave/smoke.sh [reloads]     (build eave first: cargo build --release)
set -eu
cd "$(dirname "$0")"
make -s
root=$(git rev-parse --show-toplevel)
eave=${EAVE:-$root/target/release/eave}
reloads=${1:-20}
dir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/hypr-eave-smoke.XXXXXX")
fail() { echo "FAIL: $*"; exit 1; }

# The user's config, in the glass theme.
mkdir -p "$dir/config/eave"
{
    echo 'theme = "glass"'
    grep -v '^theme *=' "${XDG_CONFIG_HOME:-$HOME/.config}/eave/config.toml" 2>/dev/null || true
} > "$dir/config/eave/config.toml"

cat > "$dir/hyprland.lua" <<EOF
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 2 })
hl.config({ misc = { disable_hyprland_logo = true, disable_splash_rendering = true } })
hl.on("hyprland.start", function() hl.exec_cmd("XDG_CONFIG_HOME=$dir/config $eave") end)
EOF

before=$(ls "$XDG_RUNTIME_DIR/hypr")
Hyprland -c "$dir/hyprland.lua" > "$dir/hyprland.out" 2>&1 &
nested=$!
trap 'kill $nested 2>/dev/null; rm -rf "$dir"' EXIT
sleep 5
for s in $(ls "$XDG_RUNTIME_DIR/hypr"); do
    echo "$before" | grep -qx "$s" || HYPRLAND_INSTANCE_SIGNATURE=$s
done
export HYPRLAND_INSTANCE_SIGNATURE
ctl() { timeout 5 hyprctl "$@"; }
alive() { ctl version > /dev/null || fail "Hyprland stopped answering after $1"; }
glass() {
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        ctl hypreave 2>/dev/null | grep -q "glass regions" && return 0
        sleep 0.5
    done
    ctl hypreave || true
    ctl layers || true
    fail "eave got no glass after $1"
}

# eave's bar is up.
for _ in $(seq 20); do
    ctl layers 2>/dev/null | grep -q "namespace: eave" && break
    sleep 0.5
done
ctl layers | grep -q "namespace: eave" || fail "eave didn't start"

n=0
load() {
    n=$((n + 1))
    cp hypr-eave.so "$dir/hypr-eave-$n.so" # dlopen hands back the old code for a path it has
    ctl plugin load "$dir/hypr-eave-$n.so" > /dev/null || fail "load $n"
}
unload() { ctl plugin unload "$dir/hypr-eave-$n.so" > /dev/null || fail "unload $n"; }

load
glass "loading"
echo "ok: eave has glass"
sleep 1 # a few frames: the shaders compile, and the buffers are drawn, as the glass first draws
ctl hypreave | grep -q "glass on" || { ctl hypreave; fail "the glass turned itself off"; }
echo "ok: the glass draws"

i=0
while [ $i -lt "$reloads" ]; do
    unload
    sleep 0.1
    load
    sleep 0.2
    alive "reload $i"
    i=$((i + 1))
done
glass "$reloads reloads"
echo "ok: $reloads reloads"
regions=$(ctl hypreave | sed -n 's/.*surface: \([0-9]*\) glass regions, \([0-9]*\) solid.*/\1 \2/p' | head -1)
[ -n "$regions" ] && [ $((${regions% *} + ${regions#* })) -le 64 ] || fail "more regions than fit: $regions"
echo "ok: $regions regions (glass, solid)"
ctl hypreave | grep -q "[1-9][0-9]* bezel" || fail "eave sent no bezel records (D-113)"
echo "ok: the notch and the edge are told"

ctl eval 'hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 2, transform = 1 })' > /dev/null
sleep 1
alive "turning the monitor"
glass "turning the monitor"
ctl output create headless > /dev/null
sleep 1
alive "adding an output"
echo "ok: turned, and a second output"

# Only the nested eave: the one whose layer this Hyprland has.
pid=$(ctl layers -j | grep -o '"pid": *[0-9]*' | grep -o '[0-9]*$' | head -1)
[ -n "$pid" ] || fail "no eave to kill"
kill -KILL "$pid"
sleep 1
alive "killing eave"
unload
alive "unloading with no eave"
echo "ok: eave killed, plugin unloaded"
echo "PASS"
