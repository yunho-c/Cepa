# Cepa Agent Guide

## Product direction

Cepa is a cross-platform, desktop-only disk space analyzer. Its goal is to make
understanding and reclaiming storage fast enough, clear enough, and pleasant
enough that users spend less time manually cleaning up or moving files.

The two primary product requirements are:

1. Maximize scan and analysis performance without compromising correctness or
   making the application difficult to use.
2. Deliver an exquisite, minimal interface with the restraint, hierarchy, and
   polish of shadcn/ui.

Transparent filesystem compression is a future product capability. Its proposed
semantics, platform coverage, safety model, and rollout gates are documented in
`docs/compression.md`. A read-only, scan-authorized volume capability probe is
implemented in `src-tauri/src/compression.rs`, along with no-follow per-item state
inspection. Bounded savings estimation and immutable single-file plan previews
are implemented separately and remain read-only. Every plan is blocked because
no writer exists; mutation is not implemented. Preserve those evidence boundaries
instead of presenting inspection, estimation, or planning as writable compression
support.

## Current state and roadmap

The repository contains the first cross-platform scanner milestone. It can
select and scan a directory, stream bounded progress over a Tauri channel,
cancel active work, retain an in-memory result snapshot for drill-down, and
render coordinated radial and list views in Svelte. macOS uses an initial
`getattrlistbulk` traversal, Linux uses `getdents64` directory batches plus
`statx` metadata, and Windows uses MFT enumeration for NTFS volume roots. Each
falls back to `jwalk` when its native API is unavailable or unsuitable; Windows
subfolder scans deliberately use `jwalk` because MFT enumeration has a
whole-volume fixed cost.
The active-scan view is deliberately unframed: space found and the current path
lead, followed by a compact facts row and the largest files observed so far.
Keep Stop immediately available and unavailable-item counts visible, but do not
restore a dashboard card, metric-tile grid, repeated privacy slogan, or backend
vocabulary to this state. During an ordinary scan at the 620 by 480 minimum,
keep the first observed file row visible below the facts instead of ending the
viewport on an orphaned `Largest so far` heading. Preserve the more spacious
rhythm above 560 logical pixels tall; contextual failure callouts take priority
and may make the remaining progress content scroll.
The native window also accepts exactly one dropped folder. Drag state is reduced
through `src/lib/folder-drop.ts`; after release, Rust canonicalizes and validates
the root before the existing result is cleared and a scan begins. Invalid or
ambiguous drops preserve completed results and surface the ordinary scan or
navigation error treatment. Drops are ignored while another operation is busy.
This is an entry-point convenience, not a general path-authorized action: all
post-scan operations retain their completed scan ID and opaque node ID boundary.
The landing screen discovers local scan roots through the dependency-light
`sysinfo` disk API and presents free/total capacity in a native grouped list.
Zero-capacity and relative roots are filtered, duplicate mount paths are
collapsed, and the folder picker remains available if discovery fails or returns
nothing. On macOS, a matching `/` and `/System/Volumes/Data` APFS pair becomes one
entry: the UI displays `/`, while the scanner uses the Data volume so firmlink
enforcement does not omit user data. The backend caches display names from the
discovery response and applies them to the root node without changing its
authoritative scan path; scan start does not rerun device discovery.
When the storage picker is visible, the landing view compacts its hero and bounds
the volume list so the folder picker and manual-path fallback remain visible at
both 880 by 620 and 620 by 480. At 560 logical pixels tall or less, only the
decorative app tile is removed and volume rows tighten to 64 pixels. Preserve the
free-capacity value in the 560 through 720 logical-pixel compact desktop range;
only browser previews below 560 pixels omit it. Preserve the spacious centered
hero when no storage panel is rendered; use
`shouldShowScanRoots` for both the component and layout state so they cannot
drift.
The UI keeps the storage map and ranked items primary. Backend, accounting, and
intentional mount-boundary semantics remain available under the collapsed
`Scan details` disclosure rather than appearing as status badges or a diagnostic
footer. A completed scan with unavailable items shows one restrained,
cause-neutral coverage notice because its totals may be low; do not mislabel all
such items as permission failures. It also has explicit cancellation and
navigation-error states. Appearance follows the operating system and updates
live; development-only `?appearance=dark` and `?appearance=light` previews cover
both palettes without introducing a production setting.
If a cancellation command fails, the UI keeps the still-live scan visible and
offers Stop again; do not turn that command failure into a terminal scan error.
Folder-picker failures are likewise contextual: preserve a completed result and
show the error inline, while a first-launch failure remains on the landing view
with a picker-specific heading rather than pretending a scan began.
Completed directory views retain at most 500 list rows. The hierarchical chart
retains at most 16 ranked children per directory, three levels, and 512 recursive
wire nodes globally; every omitted sibling set is folded into byte-preserving
aggregate coverage. Keep the global budget deterministic and metric-aware.
Directory ranking uses one full child-ID partition only while that clone fits a
2 MiB budget. Wider folders use repeated 16 × limit partial selection, bounding
the top-500 ranking buffer to about 64 KiB on 64-bit platforms. Preserve exact
metric, secondary-size, name, and node-ID ordering across both paths; do not
raise the clone budget or shrink the measured window without comparable wide-
folder latency and memory evidence.
Expose the interactive radial map as a named group, not an image with interactive
descendants. Its real segments use one roving Tab stop: arrow keys move and wrap,
Home and End jump to the bounds, and Enter or Space activates the focused item.
Aggregate segments remain non-interactive. Keep the concise screen-reader
instruction associated with the group when changing chart interaction.
Pointer previews begin on actual pointer movement, not merely because a scan or
directory transition rendered a segment or row beneath a stationary cursor.
This keeps a newly completed view anchored on its current directory until the
user deliberately explores it. Keyboard focus must continue to preview its item
immediately. Drive chart emphasis, row emphasis, and the row Reveal affordance
from that same selected-entry state; raw CSS `:hover` must not reintroduce a
visual preview that disagrees with the coordinated map/list state.
List rows use `content-visibility: auto` with a 61-pixel intrinsic block size so
offscreen work can be skipped while every row remains in the DOM and reachable
through focus, find, and scrolling. File inspection realigns a list-origin
selection after both the initial and final inspector layouts; chart-origin
inspection must not scroll the page.
The list's primary and Reveal controls use roving row focus so a completed view
contributes at most two sequential Tab stops rather than one for every retained
control. Up/Down and Home/End preserve the action kind while moving; a row
without Reveal falls back to its primary control. Preserve the associated
screen-reader instructions and recover focus ownership when search results or
the current directory change. Do not make all 500 rows and actions tabbable.
The inspector is an internal scroll container when its contents exceed the
compact right pane; focused recovery messages must remain visible instead of
overflowing the explorer at the minimum window height. In the 560 through 720
logical-pixel compact two-column layout, keep the primary Estimate prompt inline
so its action remains visible at 620 by 480; secondary metadata may stay below
the inspector fold. Browser previews below 560 pixels use the stacked prompt.
The coordinated explorer must also work at the real window geometry, not only in
a wide browser preview. Cepa's 880 by 620 first-launch window keeps the radial map
and ranked list side by side throughout the native window's supported width range.
From 560 through 720 logical pixels wide, use the compact two-column explorer;
only browser previews below 560 pixels stack the panes. The narrow layout always
compacts the result header; wider views do so at 560 logical pixels tall or less.
At the configured 620 by 480 minimum, keep the coverage warning, map, and at least
two ranked rows visible together without horizontal overflow. Validate both the
configured minimum and default size when changing result spacing, column bounds,
or breakpoints.
The desktop shell restores stable window geometry with the official Tauri
window-state plugin. Track and restore only size, on-screen position, and
maximized state; do not add visibility (which can relaunch the app hidden),
decorations, or fullscreen to `StateFlags`, and never add scan paths or result
data to this state file. The plugin's macOS
startup monitor query can be empty and skip position restoration, so
`desktop_window/placement.rs` reapplies only an on-screen saved position (or
centers safely when monitor metadata is unavailable). Keep the two-launch
smoke test green when changing startup ordering or window configuration.
The packaged webview CSP permits bundled assets and the two Tauri IPC transports;
it does not allow remote content, inline scripts, or inline styles. The Vite-only
development policy separately permits its localhost HMR socket and injected
styles. Keep production and development policies distinct, preserve Tauri's
automatic asset hash/nonce injection, and prefer semantic elements or classes
over weakening the packaged policy for dynamic presentation.
Distribution metadata is explicit and project-owned: `LICENSE`, `package.json`,
`src-tauri/Cargo.toml`, and the Tauri bundle configuration agree on the MIT
license, repository, version, and `Cepa contributors` attribution. Keep the npm
package private and the Rust crate non-publishable; Cepa ships as native desktop
bundles. `src-tauri/tests/tauri_config.rs` guards this contract.
CI produces and retains exact native package archives on every platform.
Platform validators check the macOS app seal and mounted DMG, the Windows MSI
metadata and administratively extracted executable plus NSIS metadata, and the
Linux package structures. These are unsigned or ad-hoc native packaging gates,
not proof of notarization, Authenticode reputation, interactive installation, or
public release readiness.
Hard-linked bytes are deterministically assigned to the lexicographically first
relative path so parallel discovery order cannot change directory totals.
The retained arena encodes optional parent indexes in one machine word by
storing the parent ID plus one as a nonzero value. Keep the root as the only
missing parent and preserve the layout regression when changing node identity
or ancestry; the saving is material at million-entry scale.
Completed items can be revealed in the platform file manager through a backend
command that validates the retained scan and opaque node ID before reconstructing
the path; do not replace that boundary with a frontend-supplied arbitrary path.
Directory views can switch between allocated and logical size. The selected
metric is applied in Rust before bounded list and chart selection, not merely to
frontend labels, so sparse or compressed entries cannot be truncated incorrectly.
Completed directory views also support debounced, current-folder name search.
Rust matches every direct child before retaining the metric-ranked top 500, so
items below the ordinary list cutoff remain discoverable without expanding IPC
or rendering bounds. Search is scan-authorized and never accepts a path.
Completed scans can be rerun against the same root without reopening the folder
picker. Returning to the landing view first invokes the scan-authorized
`discard_scan` command. A matching scan ID releases the retained snapshot,
cancels active estimate/search work, and invalidates any compression plan; a
stale ID cannot affect a newer snapshot. If that command fails, keep the result
visible, focus its contextual error, and allow retry. After success, move focus
to the landing heading. Do not clear only the frontend and leave a potentially
multi-million-node snapshot resident. Detach the state owner under its mutex,
then retain one owner on the Tauri blocking pool until any in-flight clones
finish so the final large-arena destructor cannot run on a command/runtime
thread. Platform-aware desktop commands live in
`src/lib/shortcuts.ts`; keep
their availability state-driven, preserve IME and modified-key behavior, and
restore focus when Escape dismisses search or item details. The native
application menu in `src-tauri/src/desktop_menu.rs` shares this command path and
mirrors live availability. Keep backend menu IDs private to Rust, emit only the
small stable frontend command vocabulary, and revalidate every emitted command
in the frontend before acting. Serialize and coalesce availability updates so a
slower IPC completion cannot leave the native menu in an older state.
Directory navigation and Reveal also capture a frontend request generation in
addition to the retained scan ID. Starting another scan or returning Home
invalidates both generations before clearing state. Preserve that second
boundary: a delayed response must not replace a newer directory view, clear a
newer request's busy state, or focus an error in a fresh result even if a mock
reuses the same scan ID. `?mock=stale-actions` is the deterministic browser
regression for these races.
The radial-C app identity has one vector source at `public/cepa-icon.svg`. The
in-app mark mirrors that geometry, while `just icons` regenerates the native
desktop and store assets. macOS normalizes ICNS output deterministically;
non-macOS runs preserve the checked-in ICNS. Do not hand-edit derived PNG, ICNS,
or ICO files.
The Rust crate's default `desktop` feature owns Tauri and its plugins. `just
native-check` disables that feature to compile, lint, and test the exact scanner,
accounting, search, and compression module graph on hosts without desktop UI
libraries. Benchmark and parity recipes use the same dependency-light path.
This is native core evidence, not proof that the desktop shell builds or runs.
Because Cepa is desktop-only, the library target emits only an `rlib`; do not
restore mobile-oriented `staticlib` or `cdylib` artifacts without a real target
and native validation. MinGW debug tests can overflow the DLL export table.
After completion, the UI requests a volume compression capability using the
retained scan ID. The probe reports `inspectOnly`, `unsupported`, or `unavailable`
and always reports that no writer exists. Do not accept a frontend path for this
or infer compression state from logical-versus-allocated size.
Selecting a non-directory item requests its compression state through the same
retained scan/node boundary. macOS and Windows report existing-data metadata;
Btrfs reports future-write inode policy because those flags do not prove that
existing extents are compressed. The inspector opens or stats paths without
following links. Inspection alone does not create a mutation plan.
The Linux-only ignored Btrfs fixture verifies the real `statfs` and
`FS_IOC_GETFLAGS` paths for enabled, disabled, and inherited policy through both
path and retained-handle inspection, plus link replacement. Run
`just validate-btrfs-compression /mounted/btrfs/path`; the runner creates and
removes a uniquely named child fixture. It also requires both `jwalk` and
`statx` to mark allocated size as estimated with identical accounting. Btrfs
`st_blocks` can retain uncompressed referenced length for compressed extents;
FIEMAP does not expose compressed physical length, and the exact encoded/internal
queries require `CAP_SYS_ADMIN`. Do not restore an exact allocation claim without
a safe, unprivileged, measured replacement. This remains metadata evidence, not
proof of estimator accuracy or mutation.
Savings estimation begins only from an explicit file action. It reads at most
three aligned 256 KiB ranges, is cancellable, rejects changed sizes and links, and
returns lower/upper savings bounds with confidence and algorithm fidelity. Windows
uses LZNT1, Btrfs uses 128 KiB-chunked Zstd level 3, and macOS uses a clearly
labeled zlib proxy because no writer algorithm has been selected. Do not turn a
proxy estimate into a guaranteed savings number or run estimation automatically
on hover/selection.
If an estimate cancellation command fails, keep the still-live estimate visible
and make Cancel retryable. Guard the failure by request ID so a late stop error
cannot replace a newer or already-completed estimate.
Compression-plan preparation is also scan-authorized: the frontend supplies a
completed scan ID, opaque node ID, and operation, never a path. Rust opens the
file without following links, retains that read-only handle as the active plan's
identity anchor, snapshots platform identity and revision metadata, and inspects
compression state through the same open file. Revalidation compares both the
retained file and a fresh no-follow open of its current path. A new scan or newer
plan invalidates the old plan and releases its anchor after any in-flight
validation completes. For regular files, the scanner retains a compact exact
identity plus modification/change revision when the backend can provide it, and
preparation rejects unavailable or mismatched scan-time revisions before
producing a plan.
Unix snapshots hoist their enforced single filesystem ID once and store a
24-byte compact revision per eligible node; Windows retains the full 32-byte
revision. `scan_benchmark` schema 7 reports capacity-aware retained snapshot
payload, bytes per entry, and isolated synchronous snapshot-release time,
excluding allocator bookkeeping and the separate initial response view. Keep
those evidence boundaries distinct from peak RSS and UI latency.
Bottom-up aggregation polls cancellation every 2,048 nodes. If cancellation is
observed there, move the now-unreachable node arena to the named background
release thread so foreground response does not wait for a million-node
destructor. `aggregation_cancellation` deterministically measures the production
loop and waits for reclamation between runs while reporting foreground latency
and background release separately. Do not present its synthetic flat arena as a
filesystem, traversal, Tauri IPC, or RSS benchmark.
This metadata is not a content fingerprint: same-clock-tick data rewrites may
remain indistinguishable. The current anchor is read-only and does
not prove that a future writer can mutate and verify through that exact handle.
Do not add an apply command or expose a dead-end plan UI until a
mutation-capable held-handle/content-integrity design closes that gate.
The same constraint applies to destructive cleanup. Do not add a path-based
trash or delete action authorized only by a completed scan: a replacement could
occupy that path between scanning and mutation. Reclaim actions need an
identity-safe, race-resistant contract before they can enter the UI.

The intended scanning architecture is:

- `jwalk` as the implemented portable fallback and behavioral reference.
- `getattrlistbulk` as the implemented macOS backend. Its first parity fixture
  plus synthetic and local real-tree validation exist, but broader filesystem
  coverage and cold-cache measurements remain.
- Master File Table (MFT) traversal as the implemented Windows volume-root
  backend. It recovers all hard-link names, queries exact allocation size by
  file ID, and falls back for subfolders and non-NTFS volumes. The checked-in
  evidence from a native fixture covers parity, deterministic ownership,
  performance, and cancellation. File-ID measurements stream into a prebuilt
  node arena through bounded 256-item batches; the eight-worker cap limits the
  channel to 4,096 measurements. A single representative system-volume
  observation covers retained-state peak memory, but broader hardware,
  filesystem, and cold-cache evidence remains required.
- `getdents64` + `statx` as the implemented Linux backend. Native CI is configured
  to run parity and cancellation fixtures. The first native warm ext4 comparison
  found exact parity and bounded cancellation but slower traversal than `jwalk`;
  do not claim a Linux speedup. It remains the automatic backend because current
  measurements show materially lower peak RSS and tighter cancellation tails.
  Measured queue tuning and lazy path/name construction improved both canonical
  warm fixtures without weakening the existing bounds. Each worker also retains
  one reusable 64 KiB directory buffer instead of allocating one per directory;
  this improved a 396,033-entry mixed real tree while leaving both canonical
  shapes effectively flat. The eight-worker cap bounds those buffers to 512 KiB.
  Smaller initial result buffers, bounded buffer recycling, and shallow task
  batching were measured and rejected; read `docs/performance.md` before
  revisiting scheduler coordination from syscall counts alone.
  Broader filesystem and hardware coverage, cold-cache measurements, and
  scheduler redesign remain.

Keep roadmap items described as planned until the code and validation exist.
Do not present compilation, UI wiring, or a mocked scan as proof of real
filesystem performance.

## Technology and architecture

Cepa uses Tauri 2, Rust, Svelte 5, TypeScript, Tailwind CSS 4,
shadcn-svelte, Bun, and `just`.

Keep filesystem traversal, metadata collection, aggregation, and other
performance-sensitive work in Rust. Keep Svelte focused on presentation and
interaction; business logic and scanning logic do not belong in components.

When implementing scanning:

- Put traversal implementations behind one backend-neutral interface so the
  portable and platform-specific backends produce compatible results.
- Isolate platform-specific code. Every supported platform must retain a
  portable fallback when its optimized backend is unavailable or unsuitable.
- Stream bounded progress and incremental results to the UI. Do not require a
  complete scan before showing useful information, and do not flood the Tauri
  bridge with an event for every filesystem entry.
- Make scans cancellable and keep cancellation responsive during traversal,
  aggregation, and transport.
- Treat permission errors, disappearing entries, symlinks, hard links, mount
  boundaries, sparse files, and apparent versus allocated size as explicit
  correctness decisions. Document the chosen behavior and test it.
- Do not silently follow links or cross filesystem boundaries. Any such policy
  must be deliberate, visible to the caller, and consistent across backends.
- Keep wire types stable and compact. Avoid serializing internal traversal
  structures directly into frontend-facing APIs.

## Performance expectations

Performance claims require measurements. Establish a representative baseline,
record the environment and dataset shape, and compare like-for-like behavior.
Separate cold startup, traversal throughput, aggregation cost, bridge/update
cost, and UI rendering responsiveness when diagnosing performance.

In hot paths, pay particular attention to unnecessary allocations, path and
string conversions, repeated metadata syscalls, synchronization contention,
serialization volume, and overly frequent frontend updates. Prefer bounded
parallelism and bounded queues. Faster traversal must not cause unbounded memory
growth, nondeterministic accounting, or sluggish cancellation.
Deterministic hard-link ownership reuses two scan-local node-ID buffers when it
compares relative paths. Their capacity grows only to the deepest compared path,
they are cleared between duplicate names, and they are not retained in the
completed snapshot. Preserve that allocation bound instead of rebuilding two
ancestor vectors for every duplicate hard link.

Use the portable backend as the behavioral reference for optimized backends.
Add parity tests for shared semantics and platform-specific tests for native
behavior. Keep deterministic microbenchmarks separate from end-to-end scans,
and preserve benchmark evidence when an optimization drives a design change.

## UI and interaction standards

Build on the existing shadcn-svelte components and neutral design tokens in
`src/app.css`. Favor restrained color, strong typography, deliberate spacing,
clear hierarchy, and information-dense layouts that remain calm under large or
rapidly changing datasets.

Use DaisyDisk's primary exploration pattern as the layout direction: a
hierarchical pie chart on the left and an interactive file-and-directory list
on the right. Treat the two views as one coordinated navigator—hover,
selection, drill-down, breadcrumbs, and the current path should stay in sync so
users can move fluidly between spatial and textual exploration.

Every workflow should have intentional empty, loading, partial-result, error,
cancelled, and completed states. Preserve keyboard navigation, visible focus,
semantic controls, readable contrast, and reduced-motion usability. Progressive
updates should feel smooth without hiding freshness or blocking interaction.
Follow the live operating-system light/dark appearance rather than adding an
application-only theme preference. New bespoke surfaces must use the semantic
tokens in `src/app.css` and be reviewed in both appearances.

Avoid decorative complexity, excessive animation, generic dashboard layouts,
and bespoke controls when an existing shadcn-svelte primitive fits. Any
shadcn-svelte component may be used where it improves the experience, including
the shadcn-svelte LayerChart integration for the hierarchical visualization.
Add UI components through the repository's configured shadcn-svelte setup and
preserve the aliases and styling conventions in `components.json`.

Keep implementation vocabulary out of the primary hierarchy. Do not repeat
completion state, native backend names, syscall names, or accounting guarantees
in banners, badges, headings, and footers. User-relevant exceptions stay visible;
routine technical evidence belongs in the existing progressive disclosure.
Treat unavailable entries as an incomplete-coverage exception, but keep expected
filesystem-boundary counts in `Scan details` rather than presenting them as an
error.
Prefer native-feeling grouped surfaces, compact list rows, sentence-case labels,
and subtle separators over bordered dashboard grids and all-caps microcopy.

## Repository map

Start with these files:

- `README.md`: current setup and developer workflow.
- `src/App.svelte`: scan workflow and coordinated storage explorer.
- `src/lib/scanner.ts`: frontend scan protocol types and formatters.
- `src/lib/dev-mock.ts`: development-only Tauri workflow scenarios loaded by
  the `?mock=` query parameter.
- `src/lib/dev-stress.ts`: deterministic 500-row, 512-chart-node frontend
  stress fixture and coherent drill-down views.
- `src/lib/appearance.ts`: root appearance synchronization and live system-theme
  change handling.
- `src/lib/folder-drop.ts`: pure native drag-event decisions and privacy-safe
  dropped-item labels.
- `src/lib/scan-roots.ts`: scan-root wire type and bounded capacity helpers.
- `src/lib/components/scan-root-picker.svelte`: grouped landing-screen volume
  chooser and its loading, unavailable, and preparing states.
- `src/lib/shortcuts.ts`: platform-aware desktop command resolution and
  shared native-menu availability and shortcut conflict rules.
- `src/lib/completed-scan-request.ts`: shared scan-ID and request-generation
  ownership check for delayed completed-scan actions.
- `src/lib/components/cepa-mark.svelte`: adaptive in-app rendering of the
  canonical radial-C identity.
- `public/cepa-icon.svg` and `scripts/generate-icons.ts`: canonical app icon and
  bounded desktop-only asset generation.
- `src/app.css`: Tailwind setup and the shared shadcn-svelte theme tokens.
- `src/lib/components/ui/`: reusable shadcn-svelte UI primitives.
- `src-tauri/src/lib.rs`: Tauri commands and active/completed scan lifecycle.
- `src-tauri/src/desktop_menu.rs`: native menu construction, stable command
  event mapping, and live item availability.
- `src-tauri/src/desktop_window.rs`: deliberately bounded window-state
  persistence policy and plugin construction.
- `src-tauri/examples/window_state_smoke.rs`: two-process native geometry
  persistence and restoration proof with isolated temporary state.
- `src-tauri/src/scan_roots.rs`: cross-platform local-volume discovery,
  normalization, APFS system/Data collapsing, and compact wire contract.
- `src-tauri/src/compression.rs`: read-only platform volume-capability and
  per-item state probes plus their backend-neutral wire contracts.
- `src-tauri/src/compression/estimator.rs`: bounded sampling, codec adapters,
  conservative savings ranges, confidence, and cancellation.
- `src-tauri/src/file_revision.rs`: no-follow file opens, exact cross-platform
  metadata snapshots, and the retained plan identity-anchor primitive.
- `src-tauri/src/scanner.rs`: backend dispatch, portable traversal, shared compact arenas,
  and aggregation.
- `src-tauri/src/scanner/macos.rs`: macOS `getattrlistbulk` traversal and record parsing.
- `src-tauri/src/scanner/linux.rs`: Linux bounded worker pool, `getdents64`
  batches, descriptor-relative `statx`, mount boundaries, and identity checks.
- `docs/performance.md`: benchmark contract, baseline evidence, and limitations.
- `docs/accounting.md`: shared filesystem accounting and traversal semantics.
- `docs/compression.md`: proposed transparent-compression contract and rollout gates.
- `src-tauri/Cargo.toml` and `package.json`: Rust and frontend dependencies.
- `Justfile`: canonical development, checking, building, and bundling commands.
- `scripts/validate-{linux,macos,windows}-bundles.*`: platform package structure,
  metadata, payload, integrity, and digest validation after bundling.
- `.github/workflows/ci.yml`: native Linux, macOS, and Windows check/build matrix.

Frontend helper and visualization tests live beside their modules as
`src/lib/*.test.ts` and run through `just check`.

As the application grows, prefer small modules with clear ownership over adding
scan, state, and visualization logic directly to the current entry files.

## Development workflow

Use the repository workflows rather than inventing parallel command sequences:

```sh
just install   # install frontend dependencies from the Bun lockfile
just icons     # regenerate derived desktop icons from the canonical vector
just dev       # run the native Tauri application
just web       # run only the Vite frontend
just native-check # validate Rust core without Tauri desktop libraries
just window-state-smoke # prove native geometry persistence across two launches
just check     # frontend diagnostics, Rust formatting, checks, and tests
just build     # build the frontend and native executable without packaging
just bundle    # produce platform desktop bundles
just validate-linux-bundles # validate completed Linux bundle metadata and files
just validate-macos-bundles # validate a code-sealed app and mounted DMG
just validate-windows-bundles # validate MSI and NSIS metadata and MSI payload
```

The native recipes deliberately clear configured Rust compiler wrappers so a
machine-level `sccache` configuration cannot block Cargo. Prefer these recipes
when validating Rust or Tauri work. On macOS, `just bundle` also prioritizes
Apple's system bundle tools and defaults to ad-hoc signing when
`APPLE_SIGNING_IDENTITY` is absent. This produces a sealed local test bundle;
it is not evidence of Developer ID signing or notarization.
Canonical frontend and Tauri recipes use `bun --bun` so host Node versions do
not leak into Vite, Svelte, or Tauri builds. Preserve that runtime boundary in
both `Justfile` and Tauri's `beforeDevCommand`/`beforeBuildCommand` hooks.

## Change and validation discipline

- Inspect the relevant implementation, manifests, and existing conventions
  before changing architecture or adding dependencies.
- Keep changes narrow. Preserve unrelated worktree changes and generated files,
  and do not reformat unrelated code.
- Add focused Rust tests for meaningful scanner, accounting, protocol, and
  error-handling logic. Add frontend checks or tests when interaction or state
  behavior becomes nontrivial.
- For identity changes, regenerate from the canonical SVG, inspect a large
  raster and at least one 30–32 px asset, then build native bundles. A frontend
  screenshot alone does not prove that an executable or installer embeds it.
- For scanning changes, exercise real filesystem fixtures covering ordinary
  trees and the relevant edge cases. A successful build alone is not evidence
  that traversal or accounting works.
- For native drag-and-drop changes, test the pure event reducer and Rust root
  preflight, inspect the development-only visual preview, and run a real
  file-manager-to-window drop when desktop UI automation or a manual host is
  available. Do not report the visual preview as proof of a native window event.
- For native-menu changes, test command/availability mapping, build the desktop
  shell, and inspect a real packaged or development window. A frontend shortcut
  test does not prove that the operating-system menu was constructed or updated.
- For window-state changes, keep persistence limited to non-sensitive geometry,
  inspect the actual state file, and prove restoration across two native process
  launches. A plugin compile alone does not prove persistence.
- For scan-root discovery changes, run the real-host discovery test on each
  supported platform in addition to normalization fixtures. Check that presented
  capacity is contextual volume information rather than claiming it equals the
  scanner's reachable-file total.
- For performance changes, report the baseline, comparison, workload, and
  measurement method. Do not claim a speedup from intuition or a synthetic test
  that measures different behavior.
- Run `just check` for normal code changes. Run the narrowest relevant checks
  while iterating, then the full suite before handoff when feasible.
- Use `just native-check` when a host lacks Tauri system dependencies, but keep
  that result labeled as scanner/compression core validation. It does not cover
  Tauri command macros, plugins, the desktop executable, or bundling.
- Report exactly what was validated and distinguish static checks, mocked or
  fixture-based tests, real local scans, platform-specific validation, and
  end-to-end application proof.
- For Linux distribution changes, run `just bundle` and
  `just validate-linux-bundles`, then distinguish raw executable launch,
  installed-package launch, AppImage launch, and RPM metadata inspection. A
  headless DBus/Xvfb survival window does not prove physical desktop interaction.
- For macOS distribution changes, verify the app seal before and after mounting
  the DMG. For Windows, inspect both installer formats and administratively
  extract the MSI payload. Keep signing/notarization and interactive installer
  evidence distinct from structural package validation on every platform.
- Treat local workflow lint as wiring validation, not proof that hosted jobs
  passed; inspect the actual GitHub Actions run before claiming it.

When a requested change conflicts with correctness, portability, user safety,
or measured performance, surface the tradeoff explicitly instead of silently
choosing one priority.
