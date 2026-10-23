#!/usr/bin/env bash
set -euo pipefail
[[ -n "${BASH_VERSION}" ]]
# The launcher puts the pinned runtime's bin directory first.
shell_dir="$(dirname "$(command -v bash)")"
[[ -x "$shell_dir/cp.exe" ]]
[[ -x "$shell_dir/sed.exe" ]]
printf 'pinned shell works\n' | "$shell_dir/sed.exe" 's/works/passes/'
