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
  action for local volumes, while keeping the folder picker visible at the
  default and minimum window heights
- Native directory picker on supported desktop platforms
- Native single-folder drag and drop with a preflight check that preserves the
  current result until the dropped directory is accepted
- Responsive cancellation and automatic cancellation of superseded scans
- Failed stop requests keep the live scan visible and make cancellation retryable
- Folder-picker failures preserve an existing completed result and remain retryable
- Logical and allocated byte accounting, with exact allocation on macOS,
  non-Btrfs Unix filesystems, and native Windows MFT scans; Btrfs and portable
  Windows scans are explicitly labeled estimates
- Metric-aware directory ranking and charts switchable between space on disk
  and logical size
- Deterministic Unix hard-link deduplication and same-filesystem traversal
  boundaries
- Permission and traversal-error accounting without aborting the whole scan,
  with a visible incomplete-coverage notice when unavailable items can lower
  the reported totals
- Bounded progress updates over a Tauri channel, with a calm active-scan view
  that keeps space, current location, elapsed time, cancellation, and unavailable
  items visible without exposing backend vocabulary
- On-demand directory views backed by the completed in-memory scan snapshot
- A scan-authorized Home transition that releases the retained snapshot and
  cancels related background work instead of hiding a still-resident result
- Adaptive metric ranking that keeps ordinary-folder selection fast while
  capping transient child-ID storage at 2 MiB for extremely wide directories
- Case-insensitive current-folder search across every retained direct child,
  bounded to 500 metric-ranked results after matching
- Reveal-in-file-manager actions authorized by completed scan and item IDs
- Compact scan-time file identity and revision retention across native and
  portable backends, used to reject changed files before compression planning
- Scan-authorized, read-only volume compression capability reporting on macOS,
  Windows, and Linux, with unsupported and unavailable states kept explicit
- Selection-driven, no-follow compression-state inspection for regular files:
  current decmpfs/NTFS data state and clearly separated Btrfs future-write policy
- A repo-native real-Btrfs fixture covering volume capability, explicit enabled
  and disabled inode policy, inherited policy, retained handles, and link
  replacement
- Explicit, cancellable savings estimates that read at most three 256 KiB ranges,
  report a range and confidence, and identify exact target codecs versus proxies
- Request-owned estimate cancellation so a late stop error cannot replace a
  valid result, with retry kept available when estimation is still running
- Single-Tab-stop radial navigation with wrapping arrow keys, Home/End movement,
  Enter/Space activation, and coordinated breadcrumbs and ranked item lists
- Roving directory-list focus that keeps at most the current row's primary and
  Reveal controls in the Tab order, with Up/Down and Home/End movement across
  all retained rows
- A coordinated map-and-list explorer that remains side by side in the default
  880 by 620 window and throughout the supported desktop range down to the
  configured 620 by 480 minimum
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
the list's scroll extent. Keyboard focus moves through those rows with the arrow
keys instead of adding every row and action to the sequential Tab order. These
bounds keep bridge and rendering costs
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
Windows MFT backend now streams file-ID measurements into its retained node
arena through bounded batches; on one 5.57-million-entry system-volume
observation this reduced peak working set by 24.1%, while a stable 8,202-entry
fixture improved median traversal by 25.2% with identical accounting. These are
single-machine warm measurements, not a universal speed claim. The first native
Linux ext4 comparison found exact accounting parity and responsive
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
content-integrity gating remain before an apply command can exist. The one
active preview now keeps its no-follow file open as a read-only identity anchor,
inspects state through that handle, and revalidates both the retained file and
its current path binding. A future writer still needs a mutation-capable handle
and byte-integrity verification; this anchor does not authorize writes.
The Unix snapshot hoists the shared filesystem identity out of each retained
node; on one 101,011-entry APFS fixture this reduced measured retained payload
by 5.15% without an observed throughput regression, though process peak RSS did
not fall. The scan-details disclosure, selection inspector, and bounded estimator
are likewise read-only and never infer compression state from allocated size.
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
http://localhost:1420/?mock=cancel-error
http://localhost:1420/?mock=discard-error
http://localhost:1420/?mock=picker-error
http://localhost:1420/?mock=picker-recovery
http://localhost:1420/?mock=estimate-cancel-error
http://localhost:1420/?mock=estimate-cancel-late-error
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
`data-cepa-scan-render-ms` development attribute. `?mock=cancel-error` keeps a
scan active after a failed stop request so its recovery state can be exercised.
`?mock=discard-error` keeps a completed result visible when its retained snapshot
cannot be released and verifies the focused, contextual recovery state.
`?mock=picker-error` covers a first-launch picker failure, while
`?mock=picker-recovery` fails only after a completed scan is visible.
The two estimate-cancel scenarios cover an immediate failed stop request and a
late failure delivered after the estimate has already completed.
The drop preview is visual only; use `just dev` and drag a real folder from the
platform file manager to validate the native window event. During frontend
development, `?appearance=dark` or `?appearance=light` fixes the preview
appearance without adding a production preference; production follows the
operating system and updates live.

On Linux, the ignored Btrfs metadata fixture can be run against any writable
directory on a mounted Btrfs filesystem. It creates and removes one isolated
child directory and requires `btrfs-progs`:

```sh
just validate-btrfs-compression /mnt/btrfs
```

This validates capability and future-write inode-policy inspection only. It
does not prove existing extents are compressed or exercise a writer.

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
just observe-scan /path/to/live-volume auto
just benchmark-search /path/to/wide-folder file- 9 auto allocated
```

The optional third argument selects `jwalk`, `getattrlistbulk`, `mft`, `statx`,
or `auto`. Platform-specific backends reject explicit use on the wrong OS. MFT
is deliberately limited to an NTFS volume root because whole-volume enumeration
has a fixed cost that is unsuitable for arbitrary subfolders.

The repeat benchmark rejects any workload that changes after warmup. Use the
single-run observation command for a live system volume that cannot be made
quiescent; compare each observation's own entry counts and timings instead of
presenting it as a stable repeated benchmark.

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
entry semantics, desktop command conflict rules, and sunburst geometry and
keyboard navigation, plus directory-list focus movement.
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
