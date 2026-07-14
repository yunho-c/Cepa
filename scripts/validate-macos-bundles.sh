#!/usr/bin/env bash

set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [bundle-directory]" >&2
  exit 2
fi
if [[ $(uname -s) != Darwin ]]; then
  echo "macOS bundle validation must run on macOS" >&2
  exit 2
fi

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
bundle_root=${1:-$repository/src-tauri/target/release/bundle}
if [[ $bundle_root != /* ]]; then
  bundle_root=$repository/$bundle_root
fi

for command in bun codesign hdiutil plutil shasum; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "$command is required to validate macOS bundles" >&2
    exit 2
  fi
done

fail() {
  echo "$1" >&2
  exit 1
}

require_equal() {
  local label=$1
  local expected=$2
  local actual=$3

  if [[ $actual != "$expected" ]]; then
    fail "$label: expected '$expected', found '$actual'"
  fi
}

find_one() {
  local label=$1
  local directory=$2
  local suffix=$3
  local expected_kind=$4
  local candidate
  local -a matches=()

  if [[ -d $directory ]]; then
    for candidate in "$directory"/*; do
      [[ -e $candidate ]] || continue
      [[ $candidate == *"$suffix" ]] || continue
      if [[ $expected_kind == directory && -d $candidate ]]; then
        matches+=("$candidate")
      elif [[ $expected_kind == file && -f $candidate ]]; then
        matches+=("$candidate")
      fi
    done
  fi
  if [[ ${#matches[@]} -ne 1 ]]; then
    fail "expected exactly one $label in $directory, found ${#matches[@]}"
  fi
  printf '%s\n' "${matches[0]}"
}

plist_value() {
  local plist=$1
  local key=$2

  plutil -extract "$key" raw -o - "$plist"
}

validate_app() {
  local app=$1
  local plist=$app/Contents/Info.plist
  local executable

  [[ -f $plist ]] || fail "application Info.plist is missing: $plist"
  require_equal "application name" Cepa "$(plist_value "$plist" CFBundleDisplayName)"
  require_equal "application identifier" "$expected_identifier" "$(plist_value "$plist" CFBundleIdentifier)"
  require_equal "application version" "$expected_version" "$(plist_value "$plist" CFBundleShortVersionString)"
  require_equal "application package type" APPL "$(plist_value "$plist" CFBundlePackageType)"

  executable=$(plist_value "$plist" CFBundleExecutable)
  [[ -x $app/Contents/MacOS/$executable ]] || fail "application executable is missing or not executable"
  codesign --verify --deep --strict --verbose=2 "$app"
}

expected_version=$(
  cd -- "$repository"
  bun -e 'const value = await Bun.file("package.json").json(); console.log(value.version)'
)
expected_identifier=$(
  cd -- "$repository"
  bun -e 'const value = await Bun.file("src-tauri/tauri.conf.json").json(); console.log(value.identifier)'
)
app=$(find_one "application bundle" "$bundle_root/macos" '.app' directory)
dmg=$(find_one "disk image" "$bundle_root/dmg" '.dmg' file)

validate_app "$app"
hdiutil verify "$dmg" >/dev/null

mount_directory=$(mktemp -d "${TMPDIR:-/tmp}/cepa-dmg.XXXXXX")
attached=false
cleanup() {
  if [[ $attached == true ]]; then
    hdiutil detach "$mount_directory" -quiet 2>/dev/null || true
  fi
  rmdir "$mount_directory" 2>/dev/null || true
}
trap cleanup EXIT

# Tauri embeds licenseFile as a DMG software license agreement. Accept that
# project-owned license non-interactively so the mounted payload can be checked.
printf 'Y\n' | hdiutil attach -readonly -nobrowse -noautoopen \
  -mountpoint "$mount_directory" "$dmg" >/dev/null
attached=true
mounted_app=$(find_one "mounted application bundle" "$mount_directory" '.app' directory)
validate_app "$mounted_app"
[[ -L $mount_directory/Applications ]] || fail "DMG Applications link is missing"
require_equal "DMG Applications link" /Applications "$(readlink "$mount_directory/Applications")"
hdiutil detach "$mount_directory" -quiet
attached=false
rmdir "$mount_directory"
trap - EXIT

printf 'version=%s\n' "$expected_version"
printf 'identifier=%s\n' "$expected_identifier"
printf 'app=%s\n' "$(basename -- "$app")"
printf 'dmg=%s\n' "$(basename -- "$dmg")"
shasum -a 256 "$dmg"
