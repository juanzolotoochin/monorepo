#!/usr/bin/env bash
# Debug only: runs the VFX stress matrix (`--vfx-stress`, src/vfx_stress.rs)
# on the Steam Deck and prints the same tables as the desktop runs.
#
# Usage, from the repository root on the Linux build host:
#   kestrel/app/deck_vfx_stress.sh <set> [loads...]
# Loads: idle i10 i50 i200 lasers arcs mix50 mix200 (default: all).
# BACKENDS="legacy sprinkles" (default) picks the backends, EXTRA adds spec
# keys (for example EXTRA=max_seconds=120), STEAM_DECK_TARGET the Deck.
#
# Installs the game with `//kestrel/app:steamdeck -- --install-only`, then
# starts each run through the Deck's `launch.sh` (fullscreen in the desktop
# session) and waits for it to exit. Reports land in
# ~/.local/opt/kestrel/vfx-stress/<set>/ on the Deck and are copied to
# kestrel-vfx-stress/<set>/ here. Remove it with src/vfx_stress.rs.
set -euo pipefail

set_name="${1:?usage: deck_vfx_stress.sh <set> [loads...]}"
shift
[[ "$set_name" =~ ^[A-Za-z0-9_-]+$ ]] || { echo "The set name may use letters, digits, - and _." >&2; exit 1; }
loads="${*:-idle i10 i50 i200 lasers arcs mix50 mix200}"
backends="${BACKENDS:-legacy sprinkles}"
target="${STEAM_DECK_TARGET:-deck@steamdeck.local}"
here="$(cd "$(dirname "$0")" && pwd)"
local_out="$PWD/kestrel-vfx-stress/$set_name"

bazel run --config=remote -c opt //kestrel/app:steamdeck -- --install-only

# The whole matrix runs in one SSH session, its script read from stdin (so
# commands in it read /dev/null). A run ends by itself (`--vfx-stress` exits
# after `frames`), so each waits on the game's unit.
ssh -o BatchMode=yes "$target" "bash -s -- $(printf '%q ' "$set_name" "$loads" "$backends" "${EXTRA:-}")" <<'DECK'
set -euo pipefail
set_name=$1 loads=$2 backends=$3 extra=$4
directory="$HOME/.local/opt/kestrel"
out="$directory/vfx-stress/$set_name"
mkdir -p "$out"
# Each frame is 1/60 s of game time (`ManualDuration`); outlasts any run.
cat >"$directory/vfx-stress/stress.json" <<'SCRIPT'
{"version": 1, "scene": "boulder-lab", "fps": 60, "width": 1440, "height": 960,
 "max_frames": 7200, "steps": [{"op": "wait", "frames": 7100}]}
SCRIPT
declare -A spec=(
    [idle]="" [i10]="impacts=10" [i50]="impacts=50" [i200]="impacts=200"
    [lasers]="lasers=8" [arcs]="arcs=8"
    [mix50]="impacts=50,lasers=8,arcs=8" [mix200]="impacts=200,lasers=8,arcs=8"
)
user_bus=(env "XDG_RUNTIME_DIR=/run/user/$(id -u)" "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus")
for load in $loads; do
    [[ -v "spec[$load]" ]] || { echo "Unknown load $load." >&2; exit 1; }
    for backend in $backends; do
        base="$out/${backend}_$load"
        rm -f "$base.json" "$base.csv"
        keys="frames=4200,warmup=600${extra:+,$extra}${spec[$load]:+,${spec[$load]}},out=$base"
        echo "== $set_name $backend $load ($keys)"
        bash "$directory/launch.sh" --demo-script "$directory/vfx-stress/stress.json" \
            --vfx "$backend" --vfx-stress "$keys" </dev/null >/dev/null || true
        while "${user_bus[@]}" systemctl --user is-active --quiet kestrel-game.service; do
            sleep 2
        done
        cp "$directory/last-run.log" "$base.log"
        [[ -f "$base.json" ]] || { echo "  No report; the log's end:"; tail -n 5 "$base.log"; }
    done
done
DECK

mkdir -p "$local_out"
scp -q -o BatchMode=yes "$target:.local/opt/kestrel/vfx-stress/$set_name/*" "$local_out/"
echo "Reports: $local_out"
python3 "$here/vfx_stress_summary.py" "$local_out"
