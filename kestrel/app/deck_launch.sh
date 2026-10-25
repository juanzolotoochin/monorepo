#!/usr/bin/env bash
set -euo pipefail

directory="$HOME/.local/opt/kestrel"
desktop_pid="$(pgrep -n -u "$(id -u)" -x plasmashell || true)"
if [[ -z "$desktop_pid" || ! -r "/proc/$desktop_pid/environ" ]]; then
    echo "Start Steam Deck Desktop Mode before launching Kestrel over SSH." >&2
    exit 1
fi

# The SSH session does not inherit the graphical session's display variables.
# Read only the variables needed to connect to the active desktop and audio.
while IFS= read -r -d '' entry; do
    case "$entry" in
        DISPLAY=*|WAYLAND_DISPLAY=*|XAUTHORITY=*|XDG_RUNTIME_DIR=*|DBUS_SESSION_BUS_ADDRESS=*|XDG_SESSION_TYPE=*)
            export "$entry"
            ;;
    esac
done < "/proc/$desktop_pid/environ"

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    echo "The Deck's desktop display was not found in the active session." >&2
    exit 1
fi

cd "$directory"
nohup ./demo "$@" >last-run.log 2>&1 </dev/null &
game_pid=$!
sleep 1
if ! kill -0 "$game_pid" 2>/dev/null; then
    echo "Kestrel exited during startup; see $directory/last-run.log on the Deck." >&2
    tail -n 20 last-run.log >&2
    exit 1
fi
echo "Kestrel started on the Deck (PID $game_pid; log: $directory/last-run.log)."
