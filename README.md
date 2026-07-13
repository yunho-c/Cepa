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
- Read-only native storage discovery with free/total capacity and a direct scan
  action for local volumes, while keeping the folder picker as a fallback
- Native directory picker on supported desktop platforms
- Native single-folder drag and drop with a preflight check that preserves the
  current result until the dropped directory is accepted
- Responsive cancellation and automatic cancellation of superseded scans
- Logical and allocated byte accounting (allocated size is exact on Unix and
  currently an estimate elsewhere)
- Metric-aware directory ranking and charts switchable between space on disk
  and logical size
- Deterministic Unix hard-link deduplication and same-filesystem traversal
  boundaries
- Permission and traversal-error accounting without aborting the whole scan,
  with a visible incomplete-coverage notice when unavailable items can lower
  the reported totals
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
- A coordinated map-and-list explorer that remains side by side in the default
  880 by 620 desktop window, with a narrow stacked fallback at the configured
  minimum width
- System-synchronized light and dark appearance, including live operating-system
  changes and theme-aware chart, warning, and native-window surfaces
- Cross-platform restoration of the last stable window size, on-screen position,
  and maximized state without restoring hidden or fullscreen state
- Native application menus with state-aware Open Folder, Scan Again, Search,
  and parent-folder commands, backed by the same guarded desktop shortcuts
- Explicit scanning, cancelling, cancelled, error, empty-folder, navigation,
  partial-coverage, and completed states, with routine backend/accounting
  semantics available under a compact scan-details disclosure

Symlinks are reported but never followed. Mounted filesystems are not traversed
when the portable backend can identify filesystem boundaries. The result view
returns at most 500 rows for a directory, while the radial chart is bounded to
16 segments per directory, three visible levels, and 512 recursive wire nodes
in total; omitted chart segments are combined into an aggregate. Offscreen rows
use standards-based rendering containment without leaving the DOM or changing
the list's scroll extent. These bounds keep bridge and rendering costs
predictable even when a scan contains millions of entries.

On macOS, the landing screen collapses the sealed read-only system root and its
matching APFS Data volume into one user-facing entry. That entry displays `/`
but scans `/System/Volumes/Data`, avoiding a misleading root scan that would
skip user data at the firmlink boundary. Volume capacity is context, not a sum
of scannable files; snapshots, filesystem metadata, reserved space, and skipped
items can make the numbers differ.

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
without changing those semantics. Worker-local reuse of the native 64 KiB
directory buffer also reduced median traversal by 6.4% on a 396k-entry mixed
workspace while leaving both canonical fixtures effectively unchanged. The app
can prepare and revalidate an immutable, scan-authorized single-file plan preview,
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

Backend work can be validated on a machine without Tauri's GTK/WebKit desktop
libraries:

```sh
just native-check
```

This compiles, lints, and tests the production scanner, accounting, search,
compression inspection, estimation, and plan code with the default `desktop`
feature disabled. The fixture, parity, scan, search, and cancellation benchmark
recipes use the same dependency-light feature set. This is native core evidence,
not proof that the Tauri shell builds or launches; use both `just check` and
`just build` on a fully provisioned desktop host for that boundary.

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
http://localhost:1420/?mock=stress
http://localhost:1420/?drop=active
http://localhost:1420/?roots=preview
http://localhost:1420/?appearance=dark&roots=preview
```

The mock workflows, drop affordance, and storage preview are removed from
production builds. Combine `?mock=complete&roots=preview` to exercise a volume
selection through the complete mocked scan.
`?mock=stress` exercises the production bounds of 500 list rows and 512 recursive
chart nodes and records response-to-painted-frame time on the document's
`data-cepa-scan-render-ms` development attribute. The drop preview is visual only;
use `just dev` and drag a real folder from the platform file manager to validate
the native window event. During frontend development, `?appearance=dark` or
`?appearance=light` fixes the preview appearance without adding a production
preference; production follows the operating system and updates live.

## Desktop commands

The application menu exposes the primary desktop commands and disables each item
when the current app state cannot perform it. Keyboard commands follow the host
platform and pass through the same availability checks. Modified variants such
as Command-Shift-R remain available to the host webview instead of being
intercepted. Reserved commands that are temporarily unavailable are consumed as
no-ops so they cannot reload the webview or open its built-in find surface.

| Action | macOS | Windows and Linux |
| --- | --- | --- |
| Choose a folder | Command-O | Ctrl-O |
| Scan the current root again | Command-R | Ctrl-R |
| Search the current folder | Command-F | Ctrl-F |
| Move to the parent folder | Option-Left | Alt-Left |
| Close search, then item details | Escape | Escape |

Closing search or item details restores focus to the control that opened it.
Rescanning clears snapshot-bound state and returns focus to the completed scan
heading when the replacement scan finishes.

Window geometry is saved in Tauri's application configuration directory when
Cepa exits and restored on the next launch. Only size, on-screen position, and
maximized state are captured; schema fields for fullscreen, visibility, and
decorations remain at defaults, while scanned paths and results never enter the
file.

On a provisioned desktop host, `just window-state-smoke` launches two brief
native sessions, verifies the second window matches the first session's saved
geometry, and removes its dedicated smoke-test state file.

## App identity

[`public/cepa-icon.svg`](public/cepa-icon.svg) is the source of truth for the
favicon and every native desktop or Windows Store icon. The in-app wordmark and
landing tile reuse the same radial-C geometry through
`src/lib/components/cepa-mark.svelte`, with theme tokens supplying the adaptive
foreground and accent colors.

After editing the vector, regenerate only the desktop icon family with:

```sh
just icons
```

The generator uses a temporary Tauri icon output and copies the macOS, Windows,
Linux PNG, and Windows Store assets into `src-tauri/icons`; mobile outputs are
deliberately excluded. On macOS it repacks ICNS frames through `iconutil` for a
byte-stable result; other platforms preserve the checked-in ICNS while updating
their native assets. Inspect both `32x32.png` and `icon.png`, then run `just
bundle` before committing an identity change.

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
entry semantics, desktop command conflict rules, and sunburst geometry.
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
