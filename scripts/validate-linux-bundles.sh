#!/usr/bin/env bash

set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [bundle-directory]" >&2
  exit 2
fi
if [[ $(uname -s) != Linux ]]; then
  echo "Linux bundle validation must run on Linux" >&2
  exit 2
fi

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
bundle_root=${1:-$repository/src-tauri/target/release/bundle}
if [[ $bundle_root != /* ]]; then
  bundle_root=$repository/$bundle_root
fi

for command in bun dpkg-deb file rpm sha256sum; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "$command is required to validate Linux bundles" >&2
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

require_match() {
  local label=$1
  local pattern=$2
  local value=$3

  if ! grep -Eq "$pattern" <<<"$value"; then
    fail "$label: expected a match for '$pattern'"
  fi
}

find_one() {
  local label=$1
  local directory=$2
  local pattern=$3
  local -a matches=()

  if [[ -d $directory ]]; then
    mapfile -d '' matches < <(
      find "$directory" -maxdepth 1 -type f -name "$pattern" -print0
    )
  fi
  if [[ ${#matches[@]} -ne 1 ]]; then
    echo "expected exactly one $label in $directory, found ${#matches[@]}" >&2
    exit 1
  fi
  printf '%s\n' "${matches[0]}"
}

expected_version=$(
  cd -- "$repository"
  bun -e 'const value = await Bun.file("package.json").json(); console.log(value.version)'
)
deb=$(find_one "Debian package" "$bundle_root/deb" '*.deb')
rpm_package=$(find_one "RPM package" "$bundle_root/rpm" '*.rpm')
appimage=$(find_one "AppImage" "$bundle_root/appimage" '*.AppImage')

require_equal "Debian package name" cepa "$(dpkg-deb -f "$deb" Package)"
require_equal "Debian package version" "$expected_version" "$(dpkg-deb -f "$deb" Version)"
require_equal "Debian package architecture" amd64 "$(dpkg-deb -f "$deb" Architecture)"
deb_contents=$(dpkg-deb -c "$deb")
require_match "Debian executable" '(^|[[:space:]])(\./)?usr/bin/cepa$' "$deb_contents"
require_match "Debian desktop entry" '(^|[[:space:]])(\./)?usr/share/applications/Cepa\.desktop$' "$deb_contents"

require_equal "RPM package name" cepa "$(rpm -qp --qf '%{NAME}' "$rpm_package")"
require_equal "RPM package version" "$expected_version" "$(rpm -qp --qf '%{VERSION}' "$rpm_package")"
require_equal "RPM package architecture" x86_64 "$(rpm -qp --qf '%{ARCH}' "$rpm_package")"
rpm_contents=$(rpm -qpl "$rpm_package")
require_match "RPM executable" '^/usr/bin/cepa$' "$rpm_contents"
require_match "RPM desktop entry" '^/usr/share/applications/Cepa\.desktop$' "$rpm_contents"

[[ -x $appimage ]] || fail "AppImage is not executable: $appimage"
appimage_description=$(file "$appimage")
require_match "AppImage format" 'ELF 64-bit' "$appimage_description"

printf 'version=%s\n' "$expected_version"
printf 'deb=%s\n' "$(basename -- "$deb")"
printf 'rpm=%s\n' "$(basename -- "$rpm_package")"
printf 'appimage=%s\n' "$(basename -- "$appimage")"
sha256sum "$deb" "$rpm_package" "$appimage"
