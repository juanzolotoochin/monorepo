#!/usr/bin/env bash
set -euo pipefail

directory="$HOME/.local/opt/kestrel"
device_name="${KESTREL_DEVICE_NAME:-Steam Deck}"
bazel_target="${KESTREL_BAZEL_TARGET:-steamdeck}"
desktop_pid="$(pgrep -n -u "$(id -u)" -x plasmashell || true)"
if [[ -z "$desktop_pid" || ! -r "/proc/$desktop_pid/environ" ]]; then
    echo "Start $device_name Desktop Mode before launching Kestrel over SSH." >&2
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
    echo "The $device_name desktop display was not found in the active session." >&2
    exit 1
fi

cd "$directory"
environment=()
for name in DISPLAY WAYLAND_DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_SESSION_TYPE; do
    if [[ -v "$name" ]]; then
        environment+=("--setenv=$name=${!name}")
    fi
done
# Frame's nested desktop uses a session bus distinct from the user's systemd
# bus. Connect systemd-run/systemctl to the user bus while passing the desktop
# environment to the game via --setenv.
user_runtime="/run/user/$(id -u)"
user_bus=(env "XDG_RUNTIME_DIR=$user_runtime" "DBUS_SESSION_BUS_ADDRESS=unix:path=$user_runtime/bus")
if [[ "${1:-}" == "--steam" ]]; then
    game_id="${2:?missing Steam shortcut ID}"
    [[ "$game_id" =~ ^[0-9]+$ ]] || { echo "Invalid Steam shortcut ID." >&2; exit 1; }
    "${user_bus[@]}" systemd-run --user --unit="kestrel-steam-launch-$(date +%s)-$$" --collect \
        "${environment[@]}" -- /usr/bin/steam "steam://rungameid/$game_id"
    echo "Sent Kestrel launch request to Steam (shortcut $game_id)."
    exit 0
fi
: >last-run.log
unit="kestrel-game"
"${user_bus[@]}" systemctl --user stop "$unit.service" 2>/dev/null || true
"${user_bus[@]}" systemd-run --user --unit="$unit" --collect \
    --property="WorkingDirectory=$directory" \
    --property="StandardOutput=append:$directory/last-run.log" \
    --property="StandardError=append:$directory/last-run.log" \
    "${environment[@]}" -- "$directory/demo" --fullscreen --auto-claim-gamepad "$@"
sleep 1
if ! "${user_bus[@]}" systemctl --user is-active --quiet "$unit.service"; then
    echo "Kestrel exited during startup; see $directory/last-run.log on the Deck." >&2
    tail -n 20 last-run.log >&2
    exit 1
fi
game_pid="$("${user_bus[@]}" systemctl --user show --value -p MainPID "$unit.service")"
echo "Kestrel started on the $device_name (PID $game_pid; unit: $unit.service; log: $directory/last-run.log)."
echo "Stop it with: bazel run --config=remote -c opt //kestrel/app:$bazel_target -- --stop"
