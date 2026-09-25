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
select and scan a directory, stream bounded progress and terminal results over
a Tauri channel, cancel active work, retain an in-memory result snapshot for
drill-down, and render coordinated radial and list views in Svelte. The
`scan_directory` command must acknowledge the scan ID immediately; completion
and failure remain ordered channel events so a Stop command cannot be serialized
behind a long-lived scan response in a platform WebView. If the initial channel
send fails, do not start traversal. If a completed result cannot be delivered,
detach and release its retained snapshot off the runtime thread. macOS uses an
initial `getattrlistbulk` traversal, Linux uses `getdents64` directory batches
plus `statx` metadata, and Windows uses MFT enumeration for NTFS volume roots.
Active and completed scan ownership share one lifecycle lock. Allocate scan IDs
while holding that lock, and let completion install a snapshot only while the
same ID still owns the active slot. A superseded worker must not clear a newer
scan or replace its snapshot; release its rejected snapshot away from the
runtime thread. Preserve the concurrent-start and superseded-completion
regressions when changing this state machine.
Each falls back to `jwalk` when its native API is unavailable or unsuitable;
Windows subfolder scans deliberately use `jwalk` because MFT enumeration has a
whole-volume fixed cost.
On macOS, protect root resolution and every native or fallback scanner worker
with the thread-local no-materialization policy in `scanner/local_only.rs`.
Fail closed if that policy cannot be installed; never retry unprotected.
Exclude dataless entries before retention and descent, and count them separately
through `skippedCloudEntries` in quiet Details. Downloaded provider files remain
ordinary local files: do not blacklist iCloud or Google Drive folder names.
Keep the scoped policy restoration, protected fallback pool, and opt-in real
cloud fixture test. This is macOS APFS/File Provider protection, not Windows,
Linux, or arbitrary network-provider qualification.
The active-scan view is deliberately unframed: space found and the current path
lead, followed by a compact facts row. Do not restore a live largest-files list
or its empty placeholder; the scan state should stay focused on overall progress.
Center ordinary scan progress vertically with safe alignment so an oversized
contextual error can still fall back toward the top instead of clipping.
Keep the scan-progress grid item shrinkable so a long no-wrap current path
ellipsizes inside the viewport instead of widening the document.
Keep Stop immediately available and unavailable-item counts visible, but do not
restore a dashboard card, metric-tile grid, repeated privacy slogan, or backend
vocabulary to this state. Preserve the more spacious rhythm above 560 logical
pixels tall; contextual failure callouts take priority and may make the remaining
progress content scroll. At scan start, focus the
target heading and associate it with the visible Scanning status. Keep the polite
atomic progress region empty while the frontend still has only its zero-value
placeholder; `Scanned 0 entries and 0 B` is not a useful transition announcement.
Begin live progress only after a real event, and retain accurate singular/plural
entry wording through scanning, finishing, and stopping states. Do not bind the
polite region directly to every visual progress render: speak the first useful
event, wait at least two seconds between ordinary updates, and announce a phase
change or Stop request immediately. Reset this checkpoint for every scan. The
100 ms bridge/visual cadence and the two-second assistive cadence are separate
contracts; do not slow the visible surface to make the live region calm. The
shared completion path emits one backend-neutral `finishing` progress phase
before bottom-up aggregation
and no redundant post-aggregation update. Present it as `Finishing` with
`Preparing results…`, keep Stop available because aggregation is cancellable,
and do not expose aggregation terminology in the primary UI. Refresh its elapsed
time only from the existing 2,048-node checkpoints and at most once per 100 ms;
never turn arena size into unbounded bridge traffic. A pending or failed Stop
request must retain the finishing phase's `Space found` and `Preparing results…`
context instead of visually regressing to traversal copy. Stop must remain
focusable with guarded `aria-disabled` state while its request is live, reject
repeat activation, and move focus to either the cancelled state or its contextual
retry error when the request settles. If the Stop command fails, clear the stale
`Stopping` live sentence and resynchronize the announcement checkpoint to the
still-running scan; the focused error callout owns that recovery message.
Traversal progress transport is time-bounded to one update per 100 ms on
`jwalk`, macOS, and Linux, matching Windows and finishing heartbeats. Do not
restore the former 2,048-entry OR condition: fast local scans could emit hundreds
of bridge payloads per second, each rebuilding partial ranking and paths. Stop
responsiveness is independent because traversal checks cancellation per ingested
entry; the retained 2,048-entry constant is for bounded polling in loops that do
not already check every entry, not for UI cadence.
The traversal deadline clock is sampled after the first retained entry and then
adaptively at most once per 32 retained entries. Fast ingestion therefore avoids
a monotonic-clock read for every node, while observed slow entries reduce the
next window toward one so visual progress does not wait for a fixed 32-item
batch. Preserve the 32-entry cap, first-entry sample, adaptive slow-entry test,
and independent per-entry cancellation unless replacement measurements cover
both throughput and progress latency.
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
hero for an ordinary first launch when no storage panel is rendered. At short
window heights, first-launch error and cancellation states compact that hero so
their recovery controls remain visible. Use
`shouldShowScanRoots` for both the component and layout state so they cannot
drift. If a first-launch error or cancellation callout shares that compact
storage landing view, tighten the hero and callout rhythm enough that focusing
the recovery message does not scroll the heading out of view at 620 by 480.
Storage-discovery retry must not strand keyboard focus when its button is
replaced. Move focus to the loading status immediately, then to the first
recovered volume, the renewed Try again control, or Choose folder when no roots
are returned. Keep the loading status focusable only programmatically and retain
its single status announcement. Development-only `?roots=loading` and
`?roots=error` previews hold those states; the error preview's Try again moves
through loading to the existing ready fixture. `?roots=preview` and
`?roots=ready` both render that ready fixture. Keep these modes out of production.
Selecting a discovered volume must likewise retain focus while its root is
validated. Keep that one row focusable but `aria-disabled`, announce its Opening
state, disable the other volume and landing actions, and reject repeated
activation. When any entry path replaces the landing or result view with an
active scan, move focus to the scan heading so the next Tab reaches Stop.
Development-only `?mock=root-preparing&roots=preview` holds the volume-validation
state; keep it out of production.
The UI keeps the storage map and ranked items primary. Do not restore the
completed result's former logical-size, file-count, folder-count, and elapsed-time
summary row. The completed analysis header contains Back, the current-path
breadcrumbs, and the warning, Info, and appearance actions in one row. The first breadcrumb
shows the scan display name and reveals the authoritative root path in a Tooltip
on hover or keyboard focus. It replaces the former large disk-name heading and
Space on disk total. Keep that breadcrumb in a compact level-one heading; the
heading and its button retain both the display name and root path in their
accessible names. The heading receives focus on completion without automatically
opening the Tooltip, including after a keyboard-started scan. Long
paths scroll within the breadcrumb strip without displacing Back or the actions;
navigation scrolls the strip to the current folder. Logical size remains
available through the advanced `Size basis`
control at the top of the Info action's collapsed `Details` popover; file and
folder counts plus scan time follow it. Keep `On disk` as the default and do not
restore the metric control beside the breadcrumbs. While a metric change is
pending, keep its metric button focused with guarded `aria-disabled` state.
Success keeps Details open and retains focus on the selected metric; failure
closes it and focuses the contextual result error. Backend, accounting, and
intentional mount-boundary semantics follow there rather than appearing as
status badges or a diagnostic footer. Asynchronous technical evidence in that
popover must remain quiet: do
not put live regions, status roles, or alert roles inside scan details. A
completed scan with unavailable items shows one restrained,
cause-neutral warning action beside Info because its totals may be low; do not
mislabel all such items as permission failures. Its Tooltip opens on hover and
keyboard focus, while the trigger's accessible name carries the full warning
and unavailable-item count. Do not make the warning hover-only or put live,
status, or alert semantics inside its Tooltip. It also has explicit cancellation and
navigation-error states. Appearance starts with the operating system and updates
live until the user uses the analysis header's sun/moon toggle, immediately to
the right of Info in the same action group. A manual light/dark
choice lasts for the app session, including navigation and new scans; the next
launch follows the system again. Keep the toggle inert with the result view
during a folder-drop overlay. Development-only
`?appearance=dark` and `?appearance=light` previews set the initial palette and
ignore system changes while still allowing the toggle. Keep them out of production.
Do not restore a persistent application header on landing, scanning, or result
views. The landing tile owns app identity, active scans keep Stop beside their
status, and completed results use a single compact navigation row: breadcrumbs
sit to the right of Back, with the icon-only Details and appearance actions at
the far end. The Info
action opens an anchored shadcn-svelte Popover containing the quiet technical
evidence; do not restore a full-width disclosure, turn it into a no-op, or make
it a competing live region. Returning home must first
discard the retained scan. While that
discard is pending, keep Back focused with guarded `aria-disabled` and
`aria-busy` state, reject repeat activation, and use a wait cursor without
fading the control. Success moves focus to the landing heading; failure moves
focus to the contextual result error and makes Back available again.
If a cancellation command fails, the UI keeps the still-live scan visible and
offers Stop again; do not turn that command failure into a terminal scan error.
Folder-picker failures are likewise contextual: preserve a completed result and
show the error inline, while a first-launch failure remains on the landing view
with a picker-specific heading rather than pretending a scan began.
Landing, scan-stop, and estimate-stop failures lead with calm, actionable copy;
the exact backend or platform error belongs under their collapsed `Error
details` disclosure. Do not concatenate raw errors into the primary alert text
or remove the diagnostic disclosure. A terminal channel event can arrive before
the immediate `scan_directory` acknowledgement resolves, so observe the scan
completion promise as soon as it is created while still awaiting it normally
after acknowledgement; otherwise a handled terminal failure can briefly become
an unhandled WebView rejection.
The native folder-drop overlay is interaction-exclusive while a drag is active
or its single dropped folder is being validated. Keep the covered main view
inert and out of the accessibility tree; keyboard and screen
reader users must encounter the overlay status rather than hidden controls.
Ordinary result views suppress zero-byte folders under the selected size metric
and regular files named exactly `.DS_Store`, plus all symbolic links, after
aggregation and before bounded ranking. Keep these entries in scan totals and explicit name searches; preserve
their bytes in chart aggregate coverage, and do not draw zero-byte aggregates.
Suppress links by entry type, never by names such as `bin` or `lib`, which can
also identify real directories. Retain no-follow traversal and the guarded
Reveal boundary for links found through search.
Keep `totalItems` as the full direct-child count and `suppressedItems` separate;
list counts and truncation use eligible items. A suppressed-only folder shows
`No items to show`, not an empty-folder claim. Preserve unavailable-item warnings.
On macOS, also suppress a directory named exactly `.fseventsd` only as a direct
child of a scan root confirmed by `statfs` to be a volume mount root. Capture
that fact once on the protected scan worker; view construction stays entirely
snapshot-based. An ordinary folder with that name, a non-directory entry, or an
unconfirmed mount root keeps the ordinary visibility rules. Totals, aggregate
coverage, search, and explicit navigation into the directory remain available.
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
instruction associated with the group when changing chart interaction. While
directory navigation is pending, expose the map as busy and each real segment
as guarded `aria-disabled`, preserve the initiating segment's focus, freeze
roving keys and pointer previews, and use the same restrained opacity as the
busy list. Do not make the two halves of the navigator disagree about whether
the current view is interactive. The center name-and-size preview is visual-only:
keep it out of the accessibility tree instead of turning pointer movement into a
live announcement. Focused segments already expose the same name and size, while
the named group and its associated instruction own the chart semantics.
Sunburst render keys use real node identity for interactive segments and a
stable traversal position for aggregates. Aggregate labels repeat across
branches, so names and depths are not unique keys; preserve the repeated-label
regression when changing chart geometry or keyed rendering.
Pointer previews begin on actual pointer movement, not merely because a scan or
directory transition rendered a segment or row beneath a stationary cursor.
This keeps a newly completed view anchored on its current directory until the
user deliberately explores it. Keyboard focus must continue to preview its item
immediately, and a pointer-leave event must not erase an item that still owns
keyboard focus. Drive chart emphasis, row emphasis, and the row context menu
from that same selected-entry state; raw CSS `:hover` must not
reintroduce a visual preview that disagrees with the coordinated map/list state.
Use the V2 study's cool neutral surfaces and muted branch palette, keeping the
existing system font family until a separate font comparison is approved.
Each displayed top-level chart item owns one color, inherited by its descendants
and corresponding list icon. Resolve colors and branch selection from the
unfiltered, bounded chart tree so search cannot recolor or disconnect a row.
Keep top-level aggregates and rows outside that tree neutral; they must not
imply a known individual chart match. Nested aggregates inherit their containing
branch but remain non-interactive. Previewing a segment keeps that item and its
descendants at full opacity, the rest of its top-level branch at 50%, and other
branches at 24%, with the existing 160 ms transition. Apply the same hierarchy
to keyboard focus. Keep its top-level row highlighted while the center describes
the exact previewed item. Rows outside the bounded chart tree leave the map
undimmed. Keep the larger chart opening and flat, visual-only
name, number, and unit readout; do not restore a raised center disc.
The chart pane has a shadcn context menu with a Show percentage checkbox for its
center preview, off by default. Keep this display choice in memory for the app
session. Right-click anywhere in the pane or use Shift+F10/Menu from a focused
segment; dismissal restores the initiating chart control without adding a Tab
stop. Close and block the menu during busy transitions. With percentages off,
show only the name and inline number/unit; do not add a size-basis caption.
List rows use `content-visibility: auto` with a 56-pixel intrinsic block size so
offscreen work can be skipped while every row remains in the DOM and reachable
through focus, find, and scrolling. Their grid must shrink within the directory
pane without horizontal scrolling. Compact row columns and secondary metadata
follow the directory pane's inline size, not the window width; truncate long
names and descriptions while retaining the size column. File inspection realigns
a list-origin selection after both the initial and final inspector layouts; chart-origin
inspection must not scroll the page.
Directory file/folder counts belong in a shadcn Tooltip on the existing row
button, available on hover and keyboard focus even in compact panes. Keep them
out of the default row copy. The tooltip shows only the file/folder counts, adds no
Tab stop, and closes while navigation or the row context menu is active.
Prefer placement below the row and wait one second on each hover, without skipping
the delay when moving between rows. Keyboard focus still exposes the counts
immediately. Ordinary files have no redundant File subtext; retain the explanatory
subtext for symbolic links and other filesystem entries.
Row sizes use the V2 study's compact number above a 60-pixel branch-colored gauge.
Size the gauge against the current directory total using the selected size basis,
including during search; clamp its fill and leave it empty for a zero-byte total.
Keep the decorative SVG hidden from assistive technology and preserve compact
pane sizing without horizontal overflow.
The list's primary controls use roving row focus so a completed view contributes
one sequential Tab stop rather than one for every retained row. Up/Down and
Home/End move between rows. Reveal lives in a shadcn-svelte context menu opened
by right-click, Shift+F10, or the Menu key; symlinks keep their existing exclusion.
Keep the menu anchored to its selected row, retain the focused menu item with
guarded `aria-disabled` while Reveal is pending, and reject repeat activation.
Success closes the menu and restores row focus; failure closes it without taking
focus from the contextual error. Escape dismisses the menu and restores row
focus. A menu must close when its view becomes busy or its row disappears.
Preserve the associated screen-reader instructions and recover focus ownership
when search results or the current directory change. Do not make all 500 rows
tabbable or restore a separate per-row Reveal button.
Pending directory navigation and Reveal must not native-disable the control that
owns focus. Keep chart segments, list, breadcrumb, Up, and recovery actions
focusable with guarded `aria-disabled` state while their request is live; reject
repeat activation and freeze that action kind's roving-arrow movement until it
settles. Desktop-shortcut and native-menu Up can begin while focus is elsewhere;
move focus to the visible Up control before starting that request so it owns the
pending interval, then move to the destination heading on success.
Successful navigation moves focus to the visible directory-pane heading, while
Reveal success returns focus to its originating row. The hidden `Storage map for`
heading labels the chart but must never own transition focus; sighted keyboard
users need a visible destination when the directory changes. Preserve the
restrained pending opacity and wait cursor without dropping focus to the document
body.
The inspector is an internal scroll container when its contents exceed the
compact right pane; focused recovery messages must remain visible instead of
overflowing the explorer at the minimum window height. Keep its identity header
and live Estimate Cancel action outside the scrollable evidence body so a focused
recovery message cannot hide the selected item or retry control. Keep Cancel and
Close in one horizontal action row; scope heading-copy layout to its named copy
wrapper instead of a broad child selector that also matches the actions. A live
estimate's Cancel action must remain focusable with guarded `aria-disabled`
state while its stop request is pending and reject repeat activation; a failed
request moves focus to its contextual error and makes Cancel retryable. In the
inspector, one always-mounted, visually hidden, polite atomic status sentence
owns compression inspection, estimation, cancellation, and result announcements.
Do not make the interactive inspector or its readout a live region: that would
re-announce controls and nest competing status owners. Focused `role="alert"`
callouts own estimate-action failures, so clear the polite status while one is
present instead of duplicating it. In the 560 through 720 logical-pixel compact
two-column layout, keep the primary Estimate
prompt inline so its action remains visible at 620 by 480; secondary metadata
may stay below the inspector fold. Browser previews below 560 pixels use the
stacked prompt.
The coordinated explorer must also work at the real window geometry, not only in
a wide browser preview. Cepa's 880 by 620 first-launch window keeps the radial map
and ranked list side by side throughout the native window's supported width range.
Keep this primary analysis surface flat within the application window: the
explorer has no outer border, shadow, or shared card background. Clip its contents
to a 16-pixel outer corner radius, rounding the map's left corners and the list's
right corners while keeping the internal divider straight.
The chart retains its quiet surface, the ranked list retains its brighter card
surface, and their single internal divider preserves the coordinated two-pane
structure without making the explorer look like a nested window. At wider
windows, let the result view reach its responsive outer gutters and let the
explorer use the available viewport height. Default the chart column to 40% of
the window width with a 340-pixel preferred floor, clamped to the divider bounds
below, and give the remaining space to the ranked list. Do not restore the former
520-pixel default chart-column cap, 1,240-pixel result-width ceiling, or 590-pixel
explorer-height ceiling.
From 560 through 720 logical pixels wide, use the compact two-column explorer;
only browser previews below 560 pixels stack the panes. The internal divider is
draggable and keyboard focusable. Left/Right resize by 10 pixels (40 with Shift),
Home/End move to its bounds, and Enter or double-click resets the responsive default.
Keep at least 320 pixels for the list on ordinary desktop layouts, or 300 in the
compact two-column layout; the chart minimum is 320 or 196 respectively and its
maximum is 1,040. Clamp the displayed size on window resize without overwriting
the user's preferred width. Keep that choice during directory navigation;
returning Home resets it. Do not persist it across launches. Escape, pointer
cancellation, lost capture, window blur, or a busy-view transition cancels a drag
and releases capture. Hide the divider in stacked browser previews. Keep sizing
inside the explorer component and update only its numeric CSS property through
CSSOM; do not inject a runtime stylesheet or weaken the packaged CSP.
On ordinary desktop layouts, let the radial map grow with the chart pane up to
the available window-height allowance; do not restore its former 440-pixel cap.
Fit its square to the chart pane's actual content width and height, with 32-pixel
padding (16 in the compact two-column layout). Do not restore viewport-height
deductions or a fixed compact chart cap. Keep the SVG bounds close to the outer
ring with room for focus strokes, and keep Up clear of the segments.
The narrow layout always compacts the result header; wider views do so at 560
logical pixels tall or less.
At the configured 620 by 480 minimum, keep the coverage warning action, map, and at least
two ranked rows visible together without horizontal overflow. Validate both the
configured minimum and default size when changing result spacing, column bounds,
or breakpoints.
Empty-folder, searching, no-match, and search-error panels must shrink with the
compact directory pane instead of imposing a desktop-height minimum inside the
clipped explorer. At 620 by 480, keep the empty label and ordinary no-match
guidance plus its Clear action fully visible without scrolling. Search errors
may use the pane as an internal scroll container only when their disclosed
details exceed it; keep Try again, Clear search, and the collapsed Search
details affordance visible in the ordinary compact state. Browser previews
below 560 pixels may retain taller stacked-state minima.
The desktop shell reserves a 20-pixel integrated drag strip. macOS retains native
traffic lights through Tauri's overlay title bar with the established 20-pixel
vertical inset. Do not halve that native inset with the CSS strip: Tauri changes
the native button container height, and a 10-pixel inset clips the controls.
The native buttons extend into the view's empty top padding; keep them clear of
the header actions. Windows and Linux remove decorations before startup placement
and use accessible webview window controls.
Keep the drag region separate from buttons, retain double-click maximize, and
derive view heights from the remaining content height so controls never overlap
the application. Window controls are inert during the folder-drop overlay.
Development-only `?titlebar=macos`, `windows`, and `linux` previews are visual
fixtures, not native window-management proof. Keep platform overrides out of
production and preserve the native smoke's title-bar clearance assertions.
The desktop shell restores stable window geometry with the official Tauri
window-state plugin. Track and restore only size, on-screen position, and
maximized state; do not add visibility (which can relaunch the app hidden),
decorations, or fullscreen to `StateFlags`, and never add scan paths or result
data to this state file. Keep the configured window hidden until restore or
first-launch centering finishes and the main webview reports its initial page
finished loading, then show and focus it so startup exposes neither default
geometry nor an unloaded surface. Preserve the two-second fallback so a broken
page cannot leave Cepa permanently invisible. The plugin's macOS
startup monitor query can be empty and skip position restoration, so
`desktop_window/placement.rs` reapplies only an on-screen saved position (or
centers safely when monitor metadata is unavailable). Keep the two-launch
smoke test green when changing startup ordering or window configuration.
The `native_scan_smoke` example reuses the production builder, bundled frontend,
custom protocol, commands, and startup path while isolating window state under a
temporary filename. At 620×480 it submits the real manual-path form, scans a
disposable fixture, waits through painted frames, switches metrics, navigates,
and searches. Its optional third fixture is a regular file and runs before the
other flows: the acknowledged real scan must terminate through the failed
channel event, show and focus the finishing-error callout, and preserve recovery
entry points. The primary guidance must remain cause-neutral, `Error details`
must stay collapsed, and the early terminal event must not register as an
unhandled page rejection. Its optional second fixture then starts a bounded long
scan, activates Stop, requires the cancelled notice to own focus, and verifies that
the landing scan entry points remain available. Preserve its one chart Tab
stop, one list Tab stop, bounded initial and logical chart nodes, clean
page-error capture, no horizontal overflow, backend disclosure, and state-file
cleanup. After the matching search, it runs a real zero-match search and
requires both the message panel and Clear action to remain fully inside the
compact directory pane. It also exercises chart
arrow/Home movement, list arrow movement, keyboard opening and dismissal of the
row context menu with focus restoration, divider keyboard resizing and reset
under the packaged CSP, nested chart-to-row branch highlighting, and Enter
activation of a chart folder.
It then returns Home, verifies
landing focus and rejection of the exact completed scan ID, and completes a
second scan in the same process with scan-heading and root-breadcrumb focus
restored for both successful scans. Derive that
completed ID from the optional failure and cancellation preflights; with both it
is ID 3. Checking either earlier ID is a false positive and does not prove Home
released the snapshot. Keep the Rust report validator paired with that expected
ID. This is programmatic WebView/IPC, terminal-event, and keyboard-event
evidence, not native picker, physical input, assistive-technology, drag-and-drop,
or installed-package proof.
macOS CI runs the harness directly. Linux CI runs the same production protocol
under isolated Xvfb and DBus sessions through `native-scan-smoke-linux`; keep
that gate after the ordinary native build and before packaging. Windows remains
outside this UI gate until a native WebView2 run is measured successfully.
The packaged webview CSP permits bundled assets and the two Tauri IPC transports;
it does not allow remote content, inline scripts, or inline styles. The Vite-only
development policy separately permits its localhost HMR socket and injected
styles. Keep production and development policies distinct, preserve Tauri's
automatic asset hash/nonce injection, and prefer semantic elements or classes
over weakening the packaged policy for dynamic presentation.
Tailwind utility detection is likewise intentionally bounded: `src/app.css`
disables automatic repository-wide discovery and registers only `src/` as a
source tree. Keep utility-bearing frontend code under that boundary or register
a narrow additional source explicitly. Do not restore automatic discovery;
untracked root files and disposable validation artifacts can otherwise change
the packaged stylesheet and make identical-source builds non-reproducible.
Distribution metadata is explicit and project-owned: `LICENSE`, `package.json`,
`src-tauri/Cargo.toml`, and the Tauri bundle configuration agree on the MIT
license, repository, version, and `Cepa contributors` attribution. Keep the npm
package private and the Rust crate non-publishable; Cepa ships as native desktop
bundles. `src-tauri/tests/tauri_config.rs` guards this contract.
CI produces and retains exact native package archives on every platform. The
Linux job smoke-tests the raw executable under isolated Xvfb and DBus sessions
after `just build` and before packaging, so startup and bundle failures remain
distinct.
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
Directory views can switch between allocated and logical size through the
advanced `Size basis` control in Details. The selected metric is applied in Rust
before bounded list and chart selection, not merely to frontend labels, so sparse
or compressed entries cannot be truncated incorrectly.
Completed directory views also support debounced, current-folder name search.
Rust matches every direct child before retaining the metric-ranked top 500, so
items below the ordinary list cutoff remain discoverable without expanding IPC
or rendering bounds. Search is scan-authorized and never accepts a path.
Search and savings-estimate requests each own a separate cancellable backend
lifecycle, but use the same atomic transition contract: allocate the monotonic
token and replace the active request under one lock, cancel the prior token, and
let finish clear only the matching owner. Preserve the concurrent-start tests;
an older call must not become active after a newer token has been issued.
Starting another scan must invalidate and detach the completed snapshot before
it sweeps search, estimate, and plan state. Each auxiliary start installs its
owner, then revalidates both the scan ID and exact retained snapshot identity
before dispatching blocking work. Thus a request before invalidation is caught
by the sweep, while one after it rejects itself. Home already follows the same
detach-before-sweep order. Preserve both cross-lifecycle race regressions; do not
authorize these starts from a previously cloned snapshot alone.
Completed scans can be rerun against the same root without reopening the folder
picker through the native application menu and its platform shortcut. Keep the
result surface focused on analysis: it should not expose persistent `Scan again`
or `Choose folder` actions. Returning Home is the visible route to selecting a
different target.
Returning to the landing view first invokes the scan-authorized
`discard_scan` command. A matching scan ID releases the retained snapshot,
cancels active estimate/search work, and invalidates any compression plan; a
stale ID cannot affect a newer snapshot. If that command fails, keep the result
visible, focus its contextual error, and allow retry. After success, move focus
to the landing heading. The initiating Back action must retain focus while the
discard request is live; do not native-disable it and strand focus on the
document body. Do not clear only the frontend and leave a potentially
multi-million-node snapshot resident. Detach the state owner under its mutex,
then retain one owner on the Tauri blocking pool until any in-flight clones
finish so the final large-arena destructor cannot run on a command/runtime
thread. Platform-aware desktop commands live in
`src/lib/shortcuts.ts`; keep
their availability state-driven, preserve IME and modified-key behavior, and
restore focus when Escape dismisses search or item details. The shared Up
command must focus the rendered Up control before invoking navigation instead
of native-disabling whichever unrelated persistent control happened to own
focus. The native
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
regression for these races. Starting directory navigation also supersedes an
older Reveal request: neither its delayed success nor failure may clear or focus
state in the newly opened folder.
If navigation, metric switching, or Reveal fails, keep the raw cause in the
collapsed `Error details`, preserve the current result, and retain only the
scan-local opaque node/metric intent needed for Try again. A successful retry
must restore focus to the opened view, the selected metric inside a reopened
Details popover, or the originating row for Reveal rather than removing the focused
recovery button without a successor.
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
If that metadata read fails, label the primary state `Couldn’t be checked` and
keep the actual reason in `Compression details`; do not expose backend failure
wording as the inspector's headline.
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
If estimation itself fails, label the primary result `Couldn’t be estimated`,
retain the actual reason in `Estimate details`, and keep Estimate again visible;
do not surface request or task vocabulary as primary copy.
Compression-plan preparation is also scan-authorized: the frontend supplies a
completed scan ID, opaque node ID, and operation, never a path. Rust opens the
file without following links, retains that read-only handle as the active plan's
identity anchor, snapshots platform identity and revision metadata, and inspects
compression state through the same open file. After the scan-time revision
matches, preparation reads the complete file through positioned reads into a
BLAKE3 content digest. The buffer is fixed at 1 MiB and cancellation is checked
between chunks. Revalidation compares both the retained file and a fresh
no-follow open of its current path, then recomputes the digest while bracketing
that read with metadata snapshots and rechecks the path afterward. A new scan,
newer plan, or Home transition cancels the old plan's hashing and releases its
anchor after any in-flight validation completes. For regular files, the scanner
retains a compact exact identity plus modification/change revision when the
backend can provide it, and preparation rejects unavailable or mismatched
scan-time revisions before producing a plan.
Plan ID allocation, generation, prepared plan, and cancellation token share one
lifecycle lock. Begin invalidates the previous plan and token atomically;
finish can install only the current still-preparing generation; current failure
advances that generation, while a stale failure cannot clear a newer preparation
or stored plan. Keep the concurrent-start token-pairing and stale-failure
regressions when changing this dormant protocol.
On Windows, keep ordinary `snapshot_no_follow` opens attribute-only so portable
scans do not require content-read permission for every file. Only explicit plan
preparation uses `open_content_snapshot_no_follow` to add `GENERIC_READ`; do not
merge those access paths.
Unix snapshots hoist their enforced single filesystem ID once and store a
24-byte compact revision per eligible node; Windows retains the full 32-byte
revision. `scan_benchmark` schema 8 reports progress-event counts,
capacity-aware retained snapshot payload, bytes per entry, and isolated
synchronous snapshot-release time,
excluding allocator bookkeeping and the separate initial response view. Keep
those evidence boundaries distinct from peak RSS and UI latency.
During the existing reverse aggregation pass, completed directory nodes release
excess child-ID vector capacity before entering the retained snapshot; the root
does so after the loop. Cancellation checkpoints precede compaction, including
a final root check, and `aggregationUs` includes this work. Preserve that
single-pass placement rather than adding another full arena traversal.
Bottom-up aggregation polls cancellation every 2,048 nodes. If cancellation is
observed there, move the now-unreachable node arena to the named background
release thread so foreground response does not wait for a million-node
destructor. `aggregation_cancellation` deterministically measures the production
loop and waits for reclamation between runs while reporting foreground latency
and background release separately. Do not present its synthetic flat arena as a
filesystem, traversal, Tauri IPC, or RSS benchmark.
Scan metadata is not a content fingerprint: a same-clock-tick rewrite between
scan and plan preparation may remain indistinguishable because hashing every
scanned file would violate the scanner's performance contract. The plan's digest
does detect content changes after preparation even when metadata collides, but
the current anchor is read-only and does not prove that a future writer can
mutate and verify through that exact handle without a race after validation. Do
not add an apply command or expose a dead-end plan UI until a mutation-capable
held-handle design performs immediate pre/post-operation integrity verification
and closes that remaining handoff gate.
The same constraint applies to destructive cleanup. Do not add a path-based
trash or delete action authorized only by a completed scan: a replacement could
occupy that path between scanning and mutation. Reclaim actions need an
identity-safe, race-resistant contract before they can enter the UI.

The intended scanning architecture is:

- `jwalk` as the implemented portable fallback and behavioral reference.
- `getattrlistbulk` as the implemented macOS backend. Its first parity fixture
  plus synthetic and two-shape local real-tree validation exist. A current
  221,920-entry build checkout and 63,904-entry source registry retained exact
  `jwalk` parity while improving warm median wall time by 84.3% and 26.4%.
  Native cancellation and process-memory observations were also favorable on
  the larger tree. These remain one-machine warm-cache results; broader
  filesystem coverage and cold-cache measurements remain. A clean 31-pair rerun
  rejected eight initial workers: it regressed paired median and p95 wall time,
  cancellation median and maximum, and process-memory observations on the
  canonical directory-rich APFS fixture. Keep the measured four-worker start
  and conditional expansion. Do not repeat that constant-only retune without a
  materially different scheduler design and representative multi-shape
  evidence.
- Master File Table (MFT) traversal as the implemented Windows volume-root
  backend. It recovers all hard-link names, queries exact allocation size by
  file ID, and falls back for subfolders and non-NTFS volumes. The checked-in
  evidence from a native fixture covers parity, deterministic ownership,
  performance, and cancellation. File-ID measurements stream into a prebuilt
  node arena through bounded 256-item batches; the eight-worker cap limits the
  channel to 4,096 measurements. After subtree ordering, reserve the known
  primary node count exactly. After measurement, reserve the summed possible
  hard-link aliases once before ingestion; a primary-only exact arena can double
  from a single recovered alias. Keep both reservations fallible and preserve
  the cancellation check before alias allocation rather than adding a post-scan
  arena copy. A single representative system-volume
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
Use `just benchmark-compare` for a backend optimization decision instead of
comparing separate sequential benchmark invocations. It alternates first-run
order and rejects accounting drift on every run, but it still requires a
quiescent immutable workload and an otherwise idle machine. Treat a broad paired
range or unrelated system load as inconclusive rather than selecting by median
alone. Use `observe-scan` for changing live volumes; do not weaken the paired
harness to accept mutable workloads.

In hot paths, pay particular attention to unnecessary allocations, path and
string conversions, repeated metadata syscalls, synchronization contention,
serialization volume, and overly frequent frontend updates. Prefer bounded
parallelism and bounded queues. Faster traversal must not cause unbounded memory
growth, nondeterministic accounting, or sluggish cancellation.
The compact count formatter is shared across streamed progress, summaries, and
directory rows. Keep that locale formatter scan-independent and reusable;
`just benchmark-count-format` alternates it against per-call construction and
rejects output drift. This is formatter evidence, not scan or full-render
throughput.
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
Follow the live operating-system light/dark appearance until the user makes a
session-only choice with the analysis-header toggle. New bespoke surfaces must use the semantic
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
routine technical evidence belongs in the existing Details popover.
Treat unavailable entries as an incomplete-coverage exception, but keep expected
filesystem-boundary counts in `Details` rather than presenting them as an
error. Loading routine capability or accounting evidence into that popover is
not an application status change and must not compete with the
focused root breadcrumb, coverage warning action, or folder-search status.
Prefer native-feeling grouped surfaces, compact list rows, sentence-case labels,
and subtle separators over bordered dashboard grids and all-caps microcopy.

Responsive hiding must not remove status updates from the accessibility tree.
Folder search uses one polite live region for its asynchronous state and result
count; it remains visible at ordinary widths and becomes visually hidden, not
`display: none`, in the compact layout. Keep error alerts separate, and do not
add a second live region to the visual searching or empty-result panels.
Search failures keep their cause in `Search details`, preserve the query, and
offer a same-query retry. Do not move typing focus away from the search field
when the error appears or the retry begins.

## Repository map

Start with these files:

- `README.md`: current setup and developer workflow.
- `src/App.svelte`: scan workflow and coordinated storage explorer.
- `src/lib/scanner.ts`: frontend scan protocol types and formatters.
- `src/lib/dev-mock.ts`: development-only Tauri workflow scenarios loaded by
  the `?mock=` query parameter.
- `src/lib/dev-stress.ts`: deterministic 500-row, 512-chart-node frontend
  stress fixture and coherent drill-down views.
- `src/lib/appearance.ts`: root appearance synchronization, live system-theme
  change handling, and the session-only manual toggle.
- `src/lib/folder-drop.ts`: pure native drag-event decisions and privacy-safe
  dropped-item labels.
- `src/lib/scan-roots.ts`: scan-root wire type and bounded capacity helpers.
- `src/lib/components/scan-root-picker.svelte`: grouped landing-screen volume
  chooser and its loading, unavailable, and preparing states.
- `src/lib/shortcuts.ts`: platform-aware desktop command resolution and
  shared native-menu availability and shortcut conflict rules.
- `src/lib/completed-scan-request.ts`: shared scan-ID and request-generation
  ownership check for delayed completed-scan actions.
- `src/lib/inspector-announcement.ts`: deterministic priority and concise copy
  for the inspector's single atomic status sentence.
- `src/lib/result-action-recovery.ts`: scan-local navigation retry descriptors
  and cause-neutral preservation copy for completed-result failures.
- `src/lib/recovery-copy.ts`: cause-neutral scan-entry and terminal-failure
  guidance kept separate from exact diagnostic details.
- `src/lib/components/cepa-mark.svelte`: adaptive in-app rendering of the
  canonical radial-C identity.
- `scripts/benchmark-count-format.ts`: alternating frontend compact-number
  formatter microbenchmark with output-parity checks.
- `public/cepa-icon.svg` and `scripts/generate-icons.ts`: canonical app icon and
  bounded desktop-only asset generation.
- `src/app.css`: Tailwind setup and the shared shadcn-svelte theme tokens.
- `src/lib/components/ui/`: reusable shadcn-svelte UI primitives.
- `src-tauri/src/lib.rs`: Tauri commands and active/completed scan lifecycle.
- `src-tauri/src/desktop_menu.rs`: native menu construction, stable command
  event mapping, and live item availability.
- `src-tauri/src/desktop_window.rs`: deliberately bounded window-state
  persistence policy and plugin construction.
- `src-tauri/examples/window_state_smoke.rs`: two-process native initial-page,
  geometry persistence, and restoration proof with isolated temporary state.
- `src-tauri/examples/native_scan_smoke.rs`: production-protocol WebView/IPC
  scan, metric, navigation, visible focus-handoff, search, focus-bound, and
  minimum-size smoke proof.
- `src-tauri/examples/content_integrity_benchmark.rs`: release throughput and
  controller-rendezvoused cancellation measurement for the production
  plan-content hashing loop. The rendezvous excludes controller scheduling from
  the reported acknowledgement latency.
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
- `src-tauri/examples/scan_comparison.rs`: alternating two-backend performance
  comparison with warmup and per-run accounting gates.
- `docs/performance.md`: benchmark contract, baseline evidence, and limitations.
- `docs/accounting.md`: shared filesystem accounting and traversal semantics.
- `docs/compression.md`: proposed transparent-compression contract and rollout gates.
- `src-tauri/Cargo.toml` and `package.json`: Rust and frontend dependencies.
- `Justfile`: canonical development, checking, building, and bundling commands.
- `scripts/validate-{linux,macos,windows}-bundles.*`: platform package structure,
  metadata, payload, integrity, and digest validation after bundling.
- `scripts/smoke-linux-desktop.sh`: bounded raw-executable startup survival under
  isolated Xvfb and DBus sessions; not physical UI or installed-package proof.
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
just smoke-linux-desktop # Linux-only raw executable startup survival
just window-state-smoke # prove native geometry persistence across two launches
just native-scan-smoke /path/to/fixture /path/to/cancellation-fixture /path/to/failure-file # production WebView, terminal failure, Stop, and scan IPC
just native-scan-smoke-linux /path/to/fixture /path/to/cancellation-fixture /path/to/failure-file # same proof under Linux Xvfb/DBus
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
- For Linux desktop changes, run `just smoke-linux-desktop` after the native
  build. For distribution changes, also run `just bundle` and
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
