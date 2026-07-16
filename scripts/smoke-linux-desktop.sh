#!/usr/bin/env bash

set -euo pipefail

if [[ $# -gt 2 ]]; then
  echo "usage: $0 [executable] [survival-seconds]" >&2
  exit 2
fi
if [[ $(uname -s) != Linux ]]; then
  echo "Linux desktop smoke validation must run on Linux" >&2
  exit 2
fi

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
executable=${1:-$repository/src-tauri/target/release/cepa}
survival_seconds=${2:-8}
if [[ $executable != /* ]]; then
  executable=$repository/$executable
fi
if [[ ! -x $executable ]]; then
  echo "Linux desktop executable is missing or not executable: $executable" >&2
  exit 2
fi
if [[ ! $survival_seconds =~ ^[1-9][0-9]*$ ]]; then
  echo "survival-seconds must be a positive integer" >&2
  exit 2
fi

for command in dbus-run-session timeout xvfb-run; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "$command is required to smoke-test the Linux desktop executable" >&2
    exit 2
  fi
done

smoke_root=$(mktemp -d "${TMPDIR:-/tmp}/cepa-linux-smoke.XXXXXX")
cleanup() {
  rm -rf -- "$smoke_root"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -m 700 "$smoke_root/home" "$smoke_root/config" "$smoke_root/runtime"
log=$smoke_root/cepa.log
set +e
timeout --signal=TERM --kill-after=2s "${survival_seconds}s" \
  xvfb-run -a dbus-run-session -- env \
  HOME="$smoke_root/home" \
  XDG_CONFIG_HOME="$smoke_root/config" \
  XDG_RUNTIME_DIR="$smoke_root/runtime" \
  NO_AT_BRIDGE=1 \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  "$executable" >"$log" 2>&1
status=$?
set -e

if [[ $status -ne 124 ]]; then
  echo "Linux desktop executable exited before the ${survival_seconds}-second smoke window (status $status)" >&2
  if [[ -s $log ]]; then
    echo "--- Cepa smoke output ---" >&2
    cat -- "$log" >&2
  fi
  exit 1
fi

printf 'executable=%s\n' "$executable"
printf 'survived_seconds=%s\n' "$survival_seconds"
printf 'captured_log_bytes=%s\n' "$(wc -c < "$log")"
