#!/usr/bin/env bash
set -euo pipefail

game_path="${1:?missing Bazel game runfile}"
launcher_path="${2:?missing Bazel launcher runfile}"
shift 2

runfiles_dir="${RUNFILES_DIR:-$0.runfiles}"
game="$runfiles_dir/$game_path"
launcher="$runfiles_dir/$launcher_path"
[[ -f "$game" && -f "$launcher" ]] || {
    echo "Kestrel runfiles are missing; build //kestrel/app:steam_deck with Bazel." >&2
    exit 1
}

target="${STEAM_DECK_TARGET:-deck@steamdeck.local}"
if [[ ! "$target" =~ ^[a-zA-Z_][a-zA-Z0-9_-]*@[a-zA-Z0-9][a-zA-Z0-9._-]*$ ]]; then
    echo "STEAM_DECK_TARGET must be user@host, for example deck@steamdeck.local." >&2
    exit 2
fi

dry_run=false
if [[ "${1:-}" == "--dry-run" ]]; then
    dry_run=true
    shift
fi

# SSH runs a shell command on the Deck. Keep each forwarded game argument a
# simple token so it cannot change that command's meaning.
for arg in "$@"; do
    if [[ ! "$arg" =~ ^[a-zA-Z0-9_./:=+-]+$ ]]; then
        echo "Unsupported game argument: $arg" >&2
        exit 2
    fi
done

if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    echo "The Steam Deck target needs a Linux x86-64 Bazel build host." >&2
    exit 2
fi

if "$dry_run"; then
    echo "Would copy the Bazel-built game and launcher to $target:.local/opt/kestrel/"
    echo "Would start Kestrel in the Deck's Desktop Mode session${*:+ with arguments: $*}"
    exit 0
fi

ssh_options=(-o BatchMode=yes -o ConnectTimeout=10)
ssh "${ssh_options[@]}" "$target" 'mkdir -p "$HOME/.local/opt/kestrel"'
scp -q "${ssh_options[@]}" "$game" "$target:.local/opt/kestrel/demo.next"
scp -q "${ssh_options[@]}" "$launcher" "$target:.local/opt/kestrel/launch.sh.next"
ssh "${ssh_options[@]}" "$target" \
    'mv "$HOME/.local/opt/kestrel/demo.next" "$HOME/.local/opt/kestrel/demo" && mv "$HOME/.local/opt/kestrel/launch.sh.next" "$HOME/.local/opt/kestrel/launch.sh" && chmod +x "$HOME/.local/opt/kestrel/demo"'

remote_command='bash "$HOME/.local/opt/kestrel/launch.sh"'
for arg in "$@"; do
    remote_command+=" $arg"
done
ssh -n "${ssh_options[@]}" "$target" "$remote_command"
