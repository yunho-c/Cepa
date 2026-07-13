#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <mounted-btrfs-directory>" >&2
  exit 2
fi

root=$1
if [[ ! -d "$root" ]]; then
  echo "fixture root is not a directory: $root" >&2
  exit 2
fi
if [[ $(stat -f -c %T -- "$root") != btrfs ]]; then
  echo "fixture root is not on Btrfs: $root" >&2
  exit 2
fi
if ! command -v btrfs >/dev/null 2>&1; then
  echo "btrfs-progs is required to prepare inode policies" >&2
  exit 2
fi

fixture=$(mktemp -d -- "$root/cepa-compression-fixture.XXXXXX")
cleanup() {
  rm -rf -- "$fixture"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

printf 'inherited-fixture' > "$fixture/inherited.bin"
printf 'enabled-fixture' > "$fixture/enabled.bin"
printf 'disabled-fixture' > "$fixture/disabled.bin"
btrfs property set "$fixture/enabled.bin" compression zstd
btrfs property set "$fixture/disabled.bin" compression none
ln -s enabled.bin "$fixture/replacement-link.bin"

echo "filesystem=$(stat -f -c %T -- "$fixture")"
btrfs --version

test_name=compression::tests::reads_btrfs_capability_and_future_write_policies
if [[ -n ${CEPA_BTRFS_TEST_BINARY:-} ]]; then
  CEPA_BTRFS_FIXTURE_ROOT="$fixture" \
    "$CEPA_BTRFS_TEST_BINARY" --exact "$test_name" --ignored --nocapture
else
  repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
  CEPA_BTRFS_FIXTURE_ROOT="$fixture" \
    cargo test --manifest-path "$repository/src-tauri/Cargo.toml" \
      --no-default-features --lib "$test_name" -- --exact --ignored --nocapture
fi
