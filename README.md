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
- Bounded-parallel `getdents64` + `statx` traversal on Linux, with automatic
  fallback to `jwalk` when the kernel/runtime cannot supply the required fields
- MFT enumeration on Windows NTFS volume roots, with exact allocation sizes,
  hard-link name recovery, and automatic `jwalk` fallback for subfolders,
  non-NTFS volumes, or unavailable volume access
- Native directory picker on supported desktop platforms
- Responsive cancellation and automatic cancellation of superseded scans
- Logical and allocated byte accounting (allocated size is exact on Unix and
  currently an estimate elsewhere)
- Metric-aware directory ranking and charts switchable between space on disk
  and logical size
- Deterministic Unix hard-link deduplication and same-filesystem traversal
  boundaries
- Permission and traversal-error accounting without aborting the whole scan
- Bounded progress updates over a Tauri channel
- On-demand directory views backed by the completed in-memory scan snapshot
- Case-insensitive current-folder search across every retained direct child,
  bounded to 500 metric-ranked results after matching
- Reveal-in-file-manager actions authorized by completed scan and item IDs
- Compact scan-time file identity and revision retention across native and
  portable backends, used to reject changed files before compression planning
- Scan-authorized, read-only volume compression capability reporting on macOS,
  Windows, and Linux, with unsupported and unavailable states kept explicit
- Selection-driven, no-follow compression-state inspection for regular files:
  current decmpfs/NTFS data state and clearly separated Btrfs future-write policy
- Explicit, cancellable savings estimates that read at most three 256 KiB ranges,
  report a range and confidence, and identify exact target codecs versus proxies
- Keyboard-accessible radial navigation, breadcrumbs, and ranked item lists
- Explicit scanning, cancelling, cancelled, error, empty-folder, navigation,
  and completed states, with backend/accounting semantics available under a
  compact scan-details disclosure

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

Broader native-filesystem and cold-cache validation, additional Windows and
Linux hardware measurements, and compression mutation remain roadmap work. The
first native Linux ext4 comparison found exact accounting parity and responsive
cancellation, but `statx` was slower than `jwalk` on all three warm workloads;
Cepa does not claim a Linux speedup. It remains the automatic Linux backend
because the measured native process uses less than half the peak RSS and has a
substantially tighter cancellation tail on the canonical 100k-entry fixtures.
Lazy path construction has since reduced native median traversal by 6–11%
without changing those semantics. The app can prepare and revalidate an
immutable, scan-authorized single-file plan preview,
but every preview is blocked because no writer exists. This dormant protocol has
no UI action and does not authorize mutation. Planning now requires an exact
retained scan-time identity/revision match, closing ordinary identical-size
replacement and rewrite gaps; same-clock-tick metadata collisions and
content-integrity/held-handle gating remain before an apply command can exist. The
scan-details disclosure, selection inspector, and bounded estimator are likewise
read-only and never infer compression state from allocated size.
The safety and backend contract is specified in
[`docs/compression.md`](docs/compression.md).

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
http://localhost:1420/?mock=reveal-error
```

These mocks are removed from production builds.

Run `just` to list every available recipe.

## Performance work

Generate reproducible fixtures and run the complete portable scan pipeline in
release mode with:

```sh
just benchmark-fixture /tmp/cepa-fixture 1000 100 0
just benchmark-scan /tmp/cepa-fixture 9 jwalk
just benchmark-search /path/to/wide-folder file- 9 auto allocated
```

The optional third argument selects `jwalk`, `getattrlistbulk`, `mft`, `statx`,
or `auto`. Platform-specific backends reject explicit use on the wrong OS. MFT
is deliberately limited to an NTFS volume root because whole-volume enumeration
has a fixed cost that is unsuitable for arbitrary subfolders.

The search benchmark scans once, warms one current-folder query, then reports
repeat latency and bounded result counts without rescanning between runs.

Validate aggregate parity on a quiescent tree and measure asynchronous
cancellation latency with:

```sh
just validate-scan /path/to/tree jwalk getattrlistbulk
just benchmark-cancellation /tmp/cepa-fixture getattrlistbulk 9 2048
# On Linux, replace getattrlistbulk with statx.
```

See [`docs/performance.md`](docs/performance.md) for the measurement contract,
current baseline, raw evidence, and interpretation limits.

## Checks and builds

```sh
just check
just build
```

`just check` runs Svelte diagnostics, frontend unit tests, Rust formatting
checks, `cargo check`, strict Clippy across all Rust targets, and the Rust tests.
Frontend coverage includes formatting, backend labels, cancellation detection,
entry semantics, and sunburst geometry.
The scanner tests use real temporary filesystem fixtures for aggregation,
cancellation, invalid roots, nested directory views, and hard-link accounting,
plus symlink and result-bound behavior.

Native recipes clear the machine's configured `sccache` wrapper so it cannot
block Cargo.

The `CI` workflow repeats `just install`, `just check`, and `just build` on
native Ubuntu 22.04, macOS, and Windows runners. Local workflow lint and macOS
execution validate the definition before handoff; only an actual GitHub Actions
run proves the Linux and Windows jobs.

```sh
just bundle
```

`just bundle` generates the platform desktop bundles. Mobile targets are not
initialized or configured.
