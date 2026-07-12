# Cepa

Cepa is a fast, cross-platform disk space analyzer built with Tauri 2, Rust,
Svelte 5, Tailwind CSS 4, and shadcn-svelte.

The current milestone is a usable cross-platform scanner: choose a folder,
watch bounded progress updates, and explore the result through a coordinated
radial storage map and size-ranked directory list. All scanning happens locally.

## Current capabilities

- Parallel portable traversal with `jwalk`
- Batched, bounded-parallel `getattrlistbulk` traversal on macOS, with automatic
  fallback to `jwalk` when the native API is unavailable for the selected
  filesystem
- Native directory picker on supported desktop platforms
- Responsive cancellation and automatic cancellation of superseded scans
- Logical and allocated byte accounting (allocated size is exact on Unix and
  currently an estimate elsewhere)
- Deterministic Unix hard-link deduplication and same-filesystem traversal
  boundaries
- Permission and traversal-error accounting without aborting the whole scan
- Bounded progress updates over a Tauri channel
- On-demand directory views backed by the completed in-memory scan snapshot
- Keyboard-accessible radial navigation, breadcrumbs, and ranked item lists
- Explicit scanning, cancelling, cancelled, error, empty-folder, navigation,
  and completed states with visible backend/accounting semantics

Symlinks are reported but never followed. Mounted filesystems are not traversed
when the portable backend can identify filesystem boundaries. The result view
returns at most 500 rows for a directory, while the radial chart is bounded to
16 segments per directory and three visible levels; omitted chart segments are
combined into an aggregate. These bounds keep bridge and rendering costs
predictable even when a scan contains millions of entries.

Hard-linked bytes are counted once and assigned to the lexicographically first
relative path in the selected root, so parallel discovery order cannot change
the completed directory breakdown. See
[`docs/accounting.md`](docs/accounting.md) for the complete size, link, mount,
error, and concurrent-mutation semantics.

MFT traversal on Windows, `statx` traversal on Linux, native-backend performance
measurement, and transparent filesystem compression remain roadmap work.

## Prerequisites

- [Bun](https://bun.sh/)
- A Rust toolchain managed by [rustup](https://rustup.rs/)
- [just](https://just.systems/)
- The [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/)
  for your desktop platform

## Development

```sh
just install
just dev
```

`just web` runs only the Vite frontend. Folder selection and scanning require
the native Tauri application, so the web-only mode is intended for frontend
layout work. During development, append one of the following mock scenarios to
exercise the complete workflow without a native process:

```text
http://localhost:1420/?mock=complete
http://localhost:1420/?mock=scanning
http://localhost:1420/?mock=error
http://localhost:1420/?mock=navigation-error
```

These mocks are removed from production builds.

Run `just` to list every available recipe.

## Performance work

Generate reproducible fixtures and run the complete portable scan pipeline in
release mode with:

```sh
just benchmark-fixture /tmp/cepa-fixture 1000 100 0
just benchmark-scan /tmp/cepa-fixture 9 jwalk
```

The optional third argument selects `jwalk`, `getattrlistbulk`, or `auto`.

Validate aggregate parity on a quiescent tree and measure asynchronous
cancellation latency with:

```sh
just validate-scan /path/to/tree jwalk getattrlistbulk
just benchmark-cancellation /tmp/cepa-fixture getattrlistbulk 9 2048
```

See [`docs/performance.md`](docs/performance.md) for the measurement contract,
current baseline, raw evidence, and interpretation limits.

## Checks and builds

```sh
just check
just build
```

`just check` runs Svelte diagnostics, frontend unit tests, Rust formatting
checks, `cargo check`, and the Rust tests. Frontend coverage includes formatting,
backend labels, cancellation detection, entry semantics, and sunburst geometry.
The scanner tests use real temporary filesystem fixtures for aggregation,
cancellation, invalid roots, nested directory views, and hard-link accounting,
plus symlink and result-bound behavior.

Native recipes clear the machine's configured `sccache` wrapper so it cannot
block Cargo.

```sh
just bundle
```

`just bundle` generates the platform desktop bundles. Mobile targets are not
initialized or configured.
