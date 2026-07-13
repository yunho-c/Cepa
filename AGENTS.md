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
The UI keeps the storage map and ranked items primary. Backend, accounting, and
intentional mount-boundary semantics remain available under the collapsed
`Scan details` disclosure rather than appearing as status badges or a diagnostic
footer. A completed scan with unavailable items shows one restrained,
cause-neutral coverage notice because its totals may be low; do not mislabel all
such items as permission failures. It also has explicit cancellation and
navigation-error states. Appearance follows the operating system and updates
live; development-only `?appearance=dark` and `?appearance=light` previews cover
both palettes without introducing a production setting.
Hard-linked bytes are deterministically assigned to the lexicographically first
relative path so parallel discovery order cannot change directory totals.
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
picker. Platform-aware desktop commands live in `src/lib/shortcuts.ts`; keep
their availability state-driven, preserve IME and modified-key behavior, and
restore focus when Escape dismisses search or item details.
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
Savings estimation begins only from an explicit file action. It reads at most
three aligned 256 KiB ranges, is cancellable, rejects changed sizes and links, and
returns lower/upper savings bounds with confidence and algorithm fidelity. Windows
uses LZNT1, Btrfs uses 128 KiB-chunked Zstd level 3, and macOS uses a clearly
labeled zlib proxy because no writer algorithm has been selected. Do not turn a
proxy estimate into a guaranteed savings number or run estimation automatically
on hover/selection.
Compression-plan preparation is also scan-authorized: the frontend supplies a
completed scan ID, opaque node ID, and operation, never a path. Rust opens the
file without following links, snapshots platform identity and revision metadata,
and can revalidate that immutable plan. A new scan or newer plan invalidates the
old plan. For regular files, the scanner retains a compact exact identity plus
modification/change revision when the backend can provide it, and preparation
rejects unavailable or mismatched scan-time revisions before producing a plan.
This metadata is not a content fingerprint: same-clock-tick inode reuse or data
rewrites may remain indistinguishable. Do not add an apply command or expose a
dead-end plan UI until a held-handle/content-integrity design closes that gate.

The intended scanning architecture is:

- `jwalk` as the implemented portable fallback and behavioral reference.
- `getattrlistbulk` as the implemented macOS backend. Its first parity fixture
  plus synthetic and local real-tree validation exist, but broader filesystem
  coverage and cold-cache measurements remain.
- Master File Table (MFT) traversal as the implemented Windows volume-root
  backend. It recovers all hard-link names, queries exact allocation size by
  file ID, and falls back for subfolders and non-NTFS volumes. The checked-in
  evidence from a native fixture covers parity, deterministic ownership,
  performance, and cancellation; broader real-volume and cold-cache evidence
  remains required.
- `getdents64` + `statx` as the implemented Linux backend. Native CI is configured
  to run parity and cancellation fixtures. The first native warm ext4 comparison
  found exact parity and bounded cancellation but slower traversal than `jwalk`;
  do not claim a Linux speedup. It remains the automatic backend because current
  measurements show materially lower peak RSS and tighter cancellation tails.
  Measured queue tuning and lazy path/name construction improved both canonical
  warm fixtures without weakening the existing bounds. Broader filesystem and
  hardware coverage, cold-cache measurements, and scheduler redesign remain.

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
- `src/lib/appearance.ts`: root appearance synchronization and live system-theme
  change handling.
- `src/lib/folder-drop.ts`: pure native drag-event decisions and privacy-safe
  dropped-item labels.
- `src/lib/scan-roots.ts`: scan-root wire type and bounded capacity helpers.
- `src/lib/components/scan-root-picker.svelte`: grouped landing-screen volume
  chooser and its loading, unavailable, and preparing states.
- `src/lib/shortcuts.ts`: platform-aware desktop command resolution and
  shortcut conflict rules.
- `src/lib/components/cepa-mark.svelte`: adaptive in-app rendering of the
  canonical radial-C identity.
- `public/cepa-icon.svg` and `scripts/generate-icons.ts`: canonical app icon and
  bounded desktop-only asset generation.
- `src/app.css`: Tailwind setup and the shared shadcn-svelte theme tokens.
- `src/lib/components/ui/`: reusable shadcn-svelte UI primitives.
- `src-tauri/src/lib.rs`: Tauri commands and active/completed scan lifecycle.
- `src-tauri/src/scan_roots.rs`: cross-platform local-volume discovery,
  normalization, APFS system/Data collapsing, and compact wire contract.
- `src-tauri/src/compression.rs`: read-only platform volume-capability and
  per-item state probes plus their backend-neutral wire contracts.
- `src-tauri/src/compression/estimator.rs`: bounded sampling, codec adapters,
  conservative savings ranges, confidence, and cancellation.
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
just check     # frontend diagnostics, Rust formatting, checks, and tests
just build     # build the frontend and native executable without packaging
just bundle    # produce platform desktop bundles
```

The native recipes deliberately clear configured Rust compiler wrappers so a
machine-level `sccache` configuration cannot block Cargo. Prefer these recipes
when validating Rust or Tauri work.

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
- Treat local workflow lint as wiring validation, not proof that remote Linux or
  Windows jobs passed; inspect the actual GitHub Actions run before claiming it.

When a requested change conflicts with correctness, portability, user safety,
or measured performance, surface the tradeoff explicitly instead of silently
choosing one priority.
