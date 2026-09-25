# Cepa

Cepa is a fast, cross-platform disk space analyzer built with Tauri 2, Rust,
Svelte 5, Tailwind CSS 4, and shadcn-svelte.

The current milestone is a usable cross-platform scanner: choose a folder,
watch bounded progress updates, and explore the result through a coordinated
radial storage map and size-ranked directory list. All scanning happens locally.

## Current capabilities

- Parallel portable traversal with `jwalk` on macOS and Linux
- Batched, bounded-parallel `getattrlistbulk` traversal on macOS, with automatic
  fallback to `jwalk` when the native API is unavailable for the selected
  filesystem
- Bounded-parallel `getdents64` + `statx` traversal on Linux, with automatic
  fallback to `jwalk` when the kernel/runtime cannot supply the required fields
- MFT enumeration on Windows NTFS volume roots, with exact allocation sizes,
  hard-link name recovery, and a local-only `win32` fallback for subfolders,
  non-NTFS volumes, or unavailable volume access
- Cloud-only entries skipped on macOS APFS/File Provider and Windows Cloud
  Files/NTFS, while downloaded provider files remain included
- Google Drive's Windows streaming volumes excluded before traversal because
  they do not expose reliable local residency; mirrored folders and physical
  cache storage remain scannable
- Read-only native storage discovery with free/total capacity and a direct scan
  action for local volumes, while keeping the folder picker visible at the
  default and minimum window heights
- Native directory picker on supported desktop platforms
- Native single-folder drag and drop with a preflight check that preserves the
  current result until the dropped directory is accepted
- Responsive cancellation and automatic cancellation of superseded scans, with
  atomic lifecycle ownership so an older worker cannot replace a newer scan's
  retained result
- Aggregation polls cancellation every 2,048 retained nodes and releases a
  cancelled full scan arena away from the foreground response path
- Failed stop requests keep the live scan visible and make cancellation retryable
- Folder-picker failures preserve an existing completed result and remain retryable
- Scan-entry and stop failures lead with actionable recovery guidance while exact
  platform diagnostics remain available under collapsed error details
- Logical and allocated byte accounting, with exact allocation on macOS,
  non-Btrfs Unix filesystems, and both Windows scan paths; Btrfs scans are
  explicitly labeled estimates
- Metric-aware directory ranking and charts switchable between space on disk
  and logical size
- Deterministic Unix and Windows hard-link deduplication and same-filesystem traversal
  boundaries
- Permission and traversal-error accounting without aborting the whole scan,
  with a visible incomplete-coverage warning action and hover/focus Tooltip when
  unavailable items can lower the reported totals
- Bounded progress and terminal scan results over one Tauri channel. The scan
  command acknowledges its ID immediately so Stop is never serialized behind a
  long-lived command response, while the calm active-scan view keeps space,
  current location, elapsed time, cancellation, and unavailable items visible
  without exposing backend vocabulary; traversal updates are capped at 10 Hz
  independently of per-entry cancellation polling, and deadline checks adapt
  between one and 32 ingested items instead of reading the clock for every node
- An explicit, cancellable finishing phase while retained directory totals are
  prepared, with time-bounded elapsed updates instead of leaving a completed
  traversal looking stalled
- A focused scan target described by its visible status, with an atomic progress
  announcement that stays quiet until real filesystem progress exists, then
  speaks ordinary updates at least two seconds apart while keeping phase and
  Stop transitions immediate
- On-demand directory views backed by the completed in-memory scan snapshot
- A scan-authorized Home transition that releases the retained snapshot and
  cancels related background work instead of hiding a still-resident result;
  final large-arena destruction runs away from the command thread
- Exact retained-snapshot revalidation at search, estimate, and dormant-plan
  dispatch, preventing work from escaping a concurrent Home or new scan
- Adaptive metric ranking that keeps ordinary-folder selection fast while
  capping transient child-ID storage at 2 MiB for extremely wide directories
- Case-insensitive current-folder search across every retained direct child,
  bounded to 500 metric-ranked results after matching, with one responsive-safe
  live result-count announcement and atomic backend request ownership
- Reveal-in-file-manager actions authorized by completed scan and item IDs
- Compact scan-time file identity and revision retention across native and
  portable backends, used to reject changed files before compression planning
- Scan-authorized, read-only volume compression capability reporting on macOS,
  Windows, and Linux, with unsupported and unavailable states kept explicit and
  quiet inside the collapsed technical disclosure until requested
- Selection-driven, no-follow compression-state inspection for regular files:
  current decmpfs/NTFS data state and clearly separated Btrfs future-write policy
- A repo-native real-Btrfs fixture covering volume capability, explicit enabled
  and disabled inode policy, inherited policy, retained handles, and link
  replacement
- Explicit, cancellable savings estimates that read at most three 256 KiB ranges,
  report a range and confidence, and identify exact target codecs versus proxies
- Request-owned estimate cancellation with atomic concurrent-start ordering so
  an older worker cannot become current after a newer token, while retry remains
  available when estimation is still running
- One concise atomic inspector status for compression and estimate progress or
  results, with focused error alerts kept separate from interactive controls
- Single-Tab-stop radial navigation with wrapping arrow keys, Home/End movement,
  Enter/Space activation, coordinated breadcrumbs and ranked item lists, and a
  visual-only center preview that does not duplicate segment announcements
- Roving directory-list focus that keeps at most the current row's primary and
  Reveal controls in the Tab order, with Up/Down and Home/End movement across
  all retained rows, followed by a visible folder-heading focus destination
  after successful navigation
- Deliberate pointer previews that keep newly rendered scan and directory views
  anchored on the current folder until the pointer actually moves over a chart
  segment or list row
- A coordinated map-and-list explorer that remains side by side in the default
  880 by 620 window and throughout the supported desktop range down to the
  configured 620 by 480 minimum
- System-synchronized light and dark appearance, including live operating-system
  changes and theme-aware chart, warning, and native-window surfaces
- An integrated, draggable title bar with native macOS traffic lights and compact
  Windows/Linux minimize, maximize/restore, and close controls
- Cross-platform restoration of the last stable window size, on-screen position,
  and maximized state without restoring hidden or fullscreen state
- Native application menus with state-aware Open Folder, Scan Again, Search,
  and parent-folder commands, backed by the same guarded desktop shortcuts
- A restrictive packaged-webview Content Security Policy that permits bundled
  assets and Tauri IPC without remote, inline-script, or inline-style sources
- Native package validators for macOS app/DMG integrity, Windows MSI/NSIS
  metadata and payloads, and Linux DEB/RPM/AppImage structure
- Explicit scanning, cancelling, cancelled, error, empty-folder, navigation,
  partial-coverage, and completed states, with routine backend/accounting
  semantics available from a compact scan-details popover

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
current macOS refresh matched portable accounting on two real APFS tree shapes.
On the 221,920-entry build checkout, `getattrlistbulk` reduced warm median wall
time by 84.3%, median cancellation latency by 82.7%, and observed median maximum
RSS by 49.3% versus `jwalk`; on a 63,904-entry source registry it reduced warm
median wall time by 26.4%. These remain one-machine warm-cache observations, not
universal or cold-storage claims. The
Windows MFT backend now streams file-ID measurements into its retained node
arena through bounded batches; on one 5.57-million-entry system-volume
observation this reduced peak working set by 24.1%, while a stable 8,202-entry
fixture improved median traversal by 25.2% with identical accounting. These are
single-machine warm measurements, not a universal speed claim. The MFT node
arena also reserves its known primary records and later hard-link-name
upper bound exactly instead of retaining geometric growth slack. On a controlled
100,116-entry NTFS fixture with one duplicate hard link, this reduced measured
retained payload by 20.9%; sampled process peak working set remained effectively
flat, so this is not a whole-process memory claim. The first native
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
replacement and rewrite gaps. It then computes a complete BLAKE3 digest through
bounded positioned reads on the retained no-follow handle, and revalidation
requires the content, metadata, and current path binding to match. A newer plan,
new scan, or Home transition cancels hashing between 1 MiB chunks. This detects
post-plan content changes even when metadata timestamps collide, but a
same-clock rewrite between the scan and initial plan can still evade scan
metadata. A future writer also needs a mutation-capable held handle and immediate
pre/post-operation verification; this read-only anchor does not authorize
writes.
The Unix snapshot hoists the shared filesystem identity out of each retained
node; on one 101,011-entry APFS fixture this reduced measured retained payload
by 5.15% without an observed throughput regression, though process peak RSS did
not fall. The scan-details popover, selection inspector, and bounded estimator
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

After `just build` on Linux, validate that the raw desktop executable remains
alive under isolated display and session buses with:

```sh
just smoke-linux-desktop
```

The smoke recipe requires `xvfb-run` and `dbus-run-session`. It is a headless
startup-survival gate; it does not prove physical desktop interaction,
folder-picker integration, installed-package behavior, or public release
readiness.

On a provisioned desktop host, exercise the bundled production frontend, Tauri
IPC transport, real scanner, and painted explorer at the supported 620×480
minimum with a disposable directory:

```sh
just native-scan-smoke /path/to/completion-fixture /path/to/cancellation-fixture /path/to/failure-file
```

The harness builds the frontend and explicitly enables Tauri's production
custom protocol. It first submits the optional third fixture, which must be a
regular file, through the real scan command. The acknowledged scan must fail
through its terminal channel event, focus the finishing-error callout, and leave
scan recovery available. Its primary guidance must stay cause-neutral, its exact
diagnostic must remain collapsed, and the early terminal event must not surface
as an unhandled page rejection. It next starts the optional second fixture as a
larger scan, activates the real Stop control, and requires the cancelled landing
notice to own focus while another scan remains available. Before cancellation
settles, the focused Stop control must expose guarded `aria-disabled` state
without a native disabled attribute. Every submitted scan must focus a target
heading described by its visible Scanning status without announcing placeholder
zero-entry progress. It then submits the
ordinary completion fixture, waits for the terminal channel event
and two painted frames, opens and closes a real file inspector, and requires its
single polite atomic status to remain outside the interactive inspector with no
nested live region. It waits for real filesystem capability evidence and requires
the Details popover to contain no live, status, or alert owner.
It then switches metrics, navigates into a directory, performs
a debounced matching folder search followed by a zero-match search, and validates
that the compact message and Clear action stay inside the directory pane. It
also validates bounded focus and overflow invariants in both size metrics while
rejecting uncaught page errors. It moves chart focus
with Arrow/Home, moves both list action kinds with arrows, opens a chart folder
with Enter, and requires that segment to retain focus with guarded
`aria-disabled` state while the map is busy and its roving keys are frozen. It
returns to the root through the shared Alt+Left desktop command, requiring the
visible Up control to take focus from the active search field and retain it with
guarded `aria-disabled` state while IPC is pending. It then requires a real
list-origin navigation to
retain focus with guarded `aria-disabled` state while IPC is pending. Chart and
list activation choose actual directory items rather than assuming the first
ranked item is a folder. Successful chart, desktop-shortcut Up, and list
navigation must each hand focus to the visible directory heading rather than the
hidden chart label. It then returns Home, requires that focused action to
remain undimmed with guarded busy and disabled semantics while discard is
pending, confirms the discarded scan is no
longer addressable, and completes a second scan in the same process. With both
preflights, the
discard check targets completed scan 3 rather than the earlier failed or
cancelled scans. Both active-scan headings, the landing heading, and the second-
result heading must receive focus at their respective view transitions.
Its window state uses a dedicated filename that is removed after the run. This
proves programmatic production-WebView, IPC, terminal failure, cancellation,
keyboard-event, and completed-scan lifecycle behavior; it does not exercise
physical input, assistive technology, the native folder picker, drag-and-drop,
or an installed package.

Linux CI and provisioned Linux workstations run the same proof inside isolated
display and session buses:

```sh
just native-scan-smoke-linux /path/to/completion-fixture /path/to/cancellation-fixture /path/to/failure-file
```

This wrapper requires `xvfb-run` and `dbus-run-session`; it does not weaken the
application or WebKit sandbox.

`just web` runs only the Vite frontend. Folder selection and scanning require
the native Tauri application, so the web-only mode is intended for frontend
layout work. During development, append one of the following mock scenarios to
exercise the complete workflow without a native process:

```text
http://localhost:1420/?mock=complete
http://localhost:1420/?mock=announcement-cadence
http://localhost:1420/?mock=scanning
http://localhost:1420/?mock=finishing
http://localhost:1420/?mock=finishing-cancel-error
http://localhost:1420/?mock=inspection-error
http://localhost:1420/?mock=cancel-error
http://localhost:1420/?mock=discard-error
http://localhost:1420/?mock=picker-error
http://localhost:1420/?mock=picker-recovery
http://localhost:1420/?mock=estimate-cancel-error
http://localhost:1420/?mock=estimate-cancel-late-error
http://localhost:1420/?mock=estimate-error
http://localhost:1420/?mock=error
http://localhost:1420/?mock=navigation-error
http://localhost:1420/?mock=reveal-error
http://localhost:1420/?mock=root-preparing&roots=preview
http://localhost:1420/?mock=search-error
http://localhost:1420/?mock=stale-actions
http://localhost:1420/?mock=stress
http://localhost:1420/?drop=active
http://localhost:1420/?roots=preview
http://localhost:1420/?roots=loading
http://localhost:1420/?roots=error
http://localhost:1420/?appearance=dark&roots=preview
http://localhost:1420/?titlebar=macos&roots=preview
http://localhost:1420/?titlebar=windows&mock=complete
http://localhost:1420/?titlebar=linux&appearance=dark
```

The mock workflows, drop affordance, and storage preview are removed from
production builds. Combine `?mock=complete&roots=preview` to exercise a volume
selection through the complete mocked scan.
The development-only `titlebar` previews reserve the same 40-pixel window-control
area as the desktop app. macOS preview dots are decorative; Windows/Linux preview
controls only simulate the maximize/restore icon and never operate the browser
window. Native builds always use the host platform. Validate native dragging,
resizing, and window actions separately from these visual previews.
The `loading` storage preview stays pending. The `error` preview keeps Choose
folder available and makes Try again transition through the loading status to
the ready fixture, including the production retry focus handoff. `ready` is an
alias for `preview`.
`?mock=stress` exercises the production bounds of 500 list rows and 512 recursive
chart nodes and records response-to-painted-frame time on the document's
`data-cepa-scan-render-ms` development attribute. `?mock=cancel-error` keeps a
scan active after a failed stop request so its recovery state can be exercised.
`?mock=announcement-cadence` emits visual progress every 100 ms for 2.6 seconds
while leaving the scan active. It verifies that the polite screen-reader status
speaks the first useful update and then waits at least two seconds, without
slowing the visible progress surface.
`?mock=finishing` holds the scan after traversal so the cancellable result-
preparation state and its delayed Stop acknowledgement can be reviewed without
a large native fixture. `?mock=finishing-cancel-error` keeps that phase active
after Stop fails so its recovery treatment and collapsed diagnostic can be
checked separately.
`?mock=inspection-error` verifies that a failed per-item metadata read stays
cause-neutral in the primary inspector while retaining its reason in disclosure.
`?mock=estimate-cancel-error` deliberately leaves the estimate pending after
Cancel fails, matching the production contract that the live operation and its
retry action must remain visible while Cancel and Close stay on one row.
`?mock=estimate-error` verifies cause-neutral primary copy, disclosed failure
evidence, and the Estimate again recovery action after estimation itself fails.
`?mock=search-error` fails the first folder search and then succeeds, covering
the disclosed cause, same-query retry, and preserved search-field focus.
`?mock=navigation-error` and `?mock=reveal-error` likewise fail their first
completed-scan action and then succeed, covering scan-local retry intent,
disclosed causes, and stable focus after recovery.
`?mock=root-preparing&roots=preview` holds a selected volume in its validation
state so its focus, announcement, and progress treatment can be inspected.
`?mock=discard-error` briefly holds the focused Back action in its guarded
pending state, then keeps the completed result visible when its retained snapshot
cannot be released and verifies the focused, contextual recovery state.
`?mock=stale-actions` holds directory navigation and Reveal requests long enough
to rescan or return Home first. It deliberately reuses the mock scan ID so the
request-generation guard—not an incidental ID change—must prevent an old view or
error from entering the new workflow. It also covers pending action focus and a
directory navigation superseding an older Reveal response.
`?mock=picker-error` covers a first-launch picker failure, while
`?mock=picker-recovery` fails only after a completed scan is visible.
`?mock=error` covers a terminal scan failure with actionable primary copy and a
collapsed exact diagnostic.
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
file. The main window remains hidden until its saved geometry is restored or a
first-launch position is centered and its initial web content finishes loading,
avoiding both a visible startup jump and an unloaded window. A two-second
fallback still shows the placed window if content readiness never arrives.

On a provisioned desktop host, `just window-state-smoke` launches two brief
native sessions against a self-contained page, verifies content readiness while
hidden and that the second window matches the first session's saved geometry,
then removes its dedicated smoke-test state file.

## App identity

[`public/cepa-icon.svg`](public/cepa-icon.svg) is the source of truth for the
favicon and every native desktop or Windows Store icon. The landing tile reuses
the same radial-C geometry through
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
just benchmark-compare /tmp/cepa-fixture jwalk getattrlistbulk 9
just observe-scan /path/to/live-volume auto
just benchmark-search /path/to/wide-folder file- 9 auto allocated
just benchmark-aggregation-cancellation 1000000 9 500001
just benchmark-content-integrity /path/to/large-file 7 268435456
just benchmark-count-format 9 10000
```

The backend argument selects `jwalk`, `getattrlistbulk`, `mft`, `statx`, or
`auto`. Platform-specific backends reject explicit use on the wrong OS. MFT is
deliberately limited to an NTFS volume root because whole-volume enumeration has
a fixed cost that is unsuitable for arbitrary subfolders.

Use `benchmark-compare` for an optimization decision between two backends. It
warms each once, alternates which backend runs first in every measured pair,
rejects any accounting drift on every run, and reports paired percentage changes
alongside absolute medians. A changing live tree still belongs in
`observe-scan`; paired execution cannot make mutable workload data stable.

The repeat benchmark rejects any workload that changes after warmup. Use the
single-run observation command for a live system volume that cannot be made
quiescent; compare each observation's own entry counts and timings instead of
presenting it as a stable repeated benchmark.

The search benchmark scans once, warms one current-folder query, then reports
repeat latency and bounded result counts without rescanning between runs.
The aggregation-cancellation benchmark uses a synthetic retained-node arena and
reports foreground cancellation separately from background reclamation; it does
not access the filesystem or measure traversal.
The content-integrity benchmark runs the production complete-file BLAKE3 loop.
For cancellation runs, a controller rendezvous publishes cancellation at an
observed chunk boundary before the worker resumes. This isolates the production
loop's acknowledgement and return latency from controller scheduling; it does
not measure UI-to-worker dispatch or a cancellation arriving during a blocked
read. Use a stable regular file larger than 1 MiB; its results describe that
file and cache state, not general storage throughput.
The count-format benchmark alternates the production cached compact-number
formatter with per-call formatter construction and rejects output drift. It
isolates presentation formatting cost; it is not a scan or full-render
benchmark.

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
just bundle
# Linux only, after bundling:
just validate-linux-bundles
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
block Cargo. Frontend and Tauri recipes also force Bun's runtime for JavaScript
tools, so an unrelated system Node installation cannot change or block the
canonical build.
Tailwind scans only the checked-in `src/` frontend tree. This keeps temporary
root-level files, package-validation artifacts, and other untracked workspace
content from silently changing the production stylesheet.

The `CI` workflow repeats `just install`, `just check`, `just build`, and
`just bundle` on native Ubuntu 22.04, macOS, and Windows runners. After the
Linux native build, it also requires the raw executable to pass
`just smoke-linux-desktop` before packaging begins. Linux then validates all
three package formats and launches the resulting AppImage under an isolated
Xvfb and DBus session before archiving it. Each job runs its platform package
validator and retains one exact distributable archive for
14 days. Linux and macOS use tar archives so executable modes, application
contents, and symlinks survive workflow-artifact transport; Windows retains a
ZIP containing only its MSI and NSIS installers. Local workflow lint and native
execution validate the definition before handoff; only an actual GitHub Actions
run proves the hosted jobs and artifact upload.

```sh
just bundle
```

`just bundle` generates the platform desktop bundles. Mobile targets are not
initialized or configured. On macOS, the recipe defaults local builds to an
ad-hoc identity so the app has a complete resource seal, and prefers Apple's
system bundle tools over conflicting third-party commands on `PATH`. Supplying
`APPLE_SIGNING_IDENTITY` preserves that identity for a release build. Ad-hoc
signing is local integrity validation only; public distribution still requires
a suitable Developer ID identity and notarization.

On Linux, `just validate-linux-bundles` requires `dpkg-deb`, `rpm`, and `file`.
It requires exactly one package of each supported Linux type, verifies their
version, architecture, executable, desktop-entry surface, and AppImage format,
then prints SHA-256 digests. It is structural package evidence, not an installed
desktop smoke test or signature verification.

On macOS, `just validate-macos-bundles` checks the application plist and
executable, performs strict deep code-seal verification, verifies and mounts the
DMG, revalidates the mounted app, and checks its `/Applications` link. Ad-hoc
signing is accepted for local integrity evidence; it is not notarization or a
public distribution identity.

On Windows, `just validate-windows-bundles` checks MSI product metadata and its
administratively extracted executable, plus the NSIS version resource. An
Authenticode signature may be absent for local and CI builds; any present
signature must validate. This does not replace interactive install, uninstall,
elevation, SmartScreen, or WebView2 bootstrap testing.

The packaged webview loads only bundled scripts, styles, and images. Its CSP
allows the two Tauri IPC transports and denies objects, frames, workers, and
remote content. The separate development policy adds only Vite's local origin,
HMR socket, and inline styles used by the development server; it does not weaken
the packaged policy. Keep new asset sources explicit and narrowly scoped.

## License

Cepa is available under the [MIT License](LICENSE). The frontend package and
Rust crate are marked private/non-publishable because this repository produces
desktop applications, not npm or crates.io packages. Native bundle metadata is
kept consistent with the two manifests by the configuration regression tests.
