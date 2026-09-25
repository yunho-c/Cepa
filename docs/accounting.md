# Filesystem accounting semantics

Cepa reports a best-effort point-in-time view of a live directory tree. The
portable and native backends share these rules; an optimized backend must match
them before it can replace `jwalk` for a platform.

## Scan roots and volume capacity

The landing screen performs read-only local-volume discovery before a scan. It
reports the operating system's total and user-available bytes for each usable
mount and derives the usage bar by subtraction. These figures help choose where
to start; they are not scanner accounting. Filesystem metadata, snapshots,
reserved blocks, inaccessible files, mount boundaries, and concurrently changing
data can all prevent the completed reachable-file total from matching used
volume capacity.

Discovery removes zero-capacity and relative entries and deduplicates identical
mount paths. On Linux, Cepa does not enable `sysinfo`'s optional network-device or
tmpfs discovery features. On macOS, a sealed `/` mount and matching
`/System/Volumes/Data` mount with the same name and capacity are treated as one
user-facing volume. Cepa displays `/` but scans the writable Data mount because
the native scanner deliberately does not traverse the system root's firmlink.

Volume names are presentation metadata retained from the discovery request.
They can label the result root and first breadcrumb, but they never replace the
canonical scan path used for traversal, snapshot authorization, reveal,
inspection, estimation, or planning.

## Sizes and entry counts

- Logical size is the byte length reported for regular files. Directory,
  symbolic-link, and other entry types contribute zero direct bytes.
- Allocated size uses native allocation attributes on macOS,
  `FILE_STANDARD_INFO::AllocationSize` in the Windows MFT backend, and physical
  blocks multiplied by 512 on other Unix filesystems. The portable Windows
  fallback reports logical size as an estimate.
- Btrfs is also explicitly estimated for both `jwalk` and `statx`. Its
  `st_blocks`/`statx` block count can expose the uncompressed referenced length
  for encoded extents instead of their compressed physical length. The ordinary
  scanner cannot recover that exact length cheaply: FIEMAP identifies encoded
  extents but does not return their compressed length, while Btrfs encoded reads
  and internal tree search require `CAP_SYS_ADMIN`. Cepa retains the kernel block
  count for ranking but never labels it exact on Btrfs.
- Sparse files can therefore have a logical size larger than their allocated
  size. Cepa preserves both values instead of substituting one for the other.
- A directory's totals are the saturating sum of its accounted descendants.
  File and directory counts describe directory entries, not unique inodes.

The explorer defaults to space on disk. Switching to logical size requests a
new directory view from the retained Rust snapshot; ranking, the bounded top-500
list, recursive chart selection, aggregate remainder, percentages, and geometry
all use the selected metric. The summary retains both totals so the distinction
remains visible.

Ordinary list and chart entries suppress folders with zero accounted bytes in
the selected metric, regular files whose exact name is `.DS_Store`, and symbolic
links. This is a presentation rule applied after aggregation and before bounded ranking; it
does not skip traversal, discard retained nodes, or change byte totals, scan
counts, hard-link ownership, or unavailable-item warnings. Ordinary small files
and other dotfiles remain visible. A zero-byte folder is not necessarily empty:
its contents may be unavailable, charged to another hard-link path, or occupy
bytes only under the other metric.

The directory response retains the full direct-child count in `totalItems` and
reports `suppressedItems` separately. The list count and truncation use eligible
items. A folder with only suppressed children shows `No items to show`, while
a folder with no retained children keeps `This folder is empty`. Suppressed bytes
remain in the chart's aggregate remainder under both metrics; zero-byte
aggregates do not draw a segment in the selected metric. Explicit name search
includes suppressed entries, preserving navigation and inspection access.

On macOS, a `.fseventsd` directory directly beneath a confirmed volume scan root
is also suppressed, including when it has nonzero bytes. The protected scan
worker checks the canonical root against `statfs`'s mount path once and retains
the result; navigation and metric switching perform no additional filesystem
queries. This includes the APFS Data volume used for startup-disk scans. Failed
mount queries, other platforms, nested directories, and non-directory entries
keep the ordinary visibility rules. This directory-specific rule retains all
accounted bytes and search access just like the file rule. Selecting `.fseventsd`
itself as a scan root still allows its contents to be explored.

Current-folder search runs against the retained snapshot and never rereads the
filesystem. It trims the query, limits it to 128 Unicode characters, and performs
a Unicode-aware case-insensitive literal substring match against every direct
child name. It is intentionally not recursive: opening a matching directory
moves the coordinated chart and list into that directory, where a new search can
be made. Matching happens before the selected size metric ranks and bounds the
top 500 results, so an item omitted from the ordinary list can still be found.
The chart continues to represent the complete current folder while the list is
filtered. Names that were not valid Unicode at scan time use the same lossy
display representation for matching and presentation.

## Hard links

When `(filesystem, file ID)` identity is available, Cepa charges a hard-linked
file's logical and allocated bytes exactly once. The owner is the
lexicographically first relative path inside the selected scan root. Every hard
link still contributes one file entry to file counts.

This ownership rule is deliberate: parallel traversal can discover directory
records in different orders, but the same unchanged tree must produce the same
directory breakdown. The live “largest files so far” list can change while an
earlier relative path is discovered; the completed result uses the deterministic
owner.

On platforms where stable file identity is unavailable, the result explicitly
marks hard-link deduplication as unsupported.

The Windows MFT backend enumerates every link name for records whose NTFS link
count exceeds one. Those names are inserted into the same tree before
aggregation, so file counts, lexicographic ownership, and deduplicated bytes use
the shared rule above. A link-enumeration failure is counted as a skipped entry
rather than silently claiming complete path accounting.

## Links, mounts, and special entries

- Symbolic links are retained and searchable but suppressed from the ordinary
  list and map. They are never followed and contribute zero accounted bytes.
  Suppression uses the entry type, not the name: real directories named `bin`,
  `lib`, or similar remain eligible. Links to files, directories, missing
  targets, and cycles all receive the same treatment without resolving targets.
  They are also excluded from reveal-in-file-manager actions because the
  cross-platform opener canonicalizes paths and would otherwise follow the
  target silently.
- Cepa does not intentionally cross filesystem boundaries. A mount point is
  listed as a directory but its children are not traversed when the backend can
  establish the boundary.
- macOS firmlinks and entries whose mount-boundary status cannot be established
  are not traversed by the native backend. They are reflected in skipped-entry
  accounting rather than guessed through.
- Linux native traversal compares `statx` mount IDs, not only device numbers,
  so bind mounts are boundaries too. Child directories are opened relative to
  their retained parent descriptor with no-follow semantics and are traversed
  only if device, inode, and mount identity still match discovery.
- Windows native traversal is selected only for the root of an NTFS volume.
  Reparse points are listed as links and not followed. Subfolder, network, and
  non-NTFS scans use the portable backend before native progress is emitted.
- Sockets, devices, and other special entries are listed as `other` and
  contribute no bytes.

## Cloud-backed storage on macOS

Scanning never intentionally downloads cloud content. Both `getattrlistbulk`
and the macOS `jwalk` fallback install Darwin's thread-local
`IOPOL_MATERIALIZE_DATALESS_FILES_OFF` policy before root resolution and on every
worker. A failure to install protection fails the scan rather than retrying
unprotected. The previous policy is restored when the thread leaves its scan
scope. This follows [Apple TN3150](https://developer.apple.com/documentation/technotes/tn3150-getting-ready-for-data-less-files).

Entries marked `SF_DATALESS` are omitted before retention or directory descent.
The flag can apply to directories and document packages as well as regular
files. Downloaded files inside iCloud, Google Drive, or other provider folders
are still counted normally; there is no provider-name or path blacklist.
A cloud-only selected root is rejected, and intermediate path resolution is
protected too. If an item becomes dataless after discovery, the OS policy blocks
materialization; `EDEADLK` from protected traversal is an intentional cloud
exclusion rather than a permission failure. Other IO failures keep their normal
unavailable-item treatment.

`skippedCloudEntries` counts observed placeholder entries or blocked directory
reads, not the unknown number of descendants in excluded subtrees. Details
shows **Cloud-only items skipped** when that count is nonzero. Known placeholders
are absent from the list and chart rather than shown as empty folders or included
in logical totals. Totals describe the reachable local files; a concurrently
evicted directory can already have a retained node or partially observed children.
Placeholder metadata, provider caches, and local content hidden beneath a
non-enumerable directory are not a complete accounting of provider disk usage.

This protection covers macOS APFS dataless/File Provider semantics. It does not
establish a no-network guarantee for arbitrary network mounts or proprietary
virtual filesystems. Windows recall attributes and Linux provider-specific
behavior are not implemented by this macOS change.

A read-only integration test accepts a small existing cloud directory containing
already-evicted entries. It checks both backends, rejects placeholder roots and
paths through them, and verifies that the original placeholders remain dataless
with unchanged identity, allocated blocks, and regular-file lengths. Directory
`st_size` is not payload size and may change after a protected enumeration
refusal. The test also sends one evicted directory directly to a native worker
to exercise the protection independently of preflight filtering. It never creates, evicts, or
modifies cloud files:

```sh
CEPA_CLOUD_FIXTURE_ROOT="/path/to/small/cloud/folder" RUSTC_WRAPPER="" CARGO_BUILD_RUSTC_WRAPPER="" \
  cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  real_cloud_placeholders_remain_evicted_on_both_backends -- --ignored --nocapture
```

Host validation on 2026-09-22 (macOS arm64) exercised both production scanner
backends through that debug integration test. Both returned the same figures:

| Existing provider tree | Cloud exclusions | Local files | Allocated bytes |
| --- | ---: | ---: | ---: |
| iCloud Books documents | 59 | 455 | 6,488,064 |
| Google Drive My Drive | 505,207 | 2,145 | 2,582,552,576 |

The checked placeholders remained dataless. The native worker refused direct
enumeration of an evicted directory with `EDEADLK`. These are live-tree scan
observations, not matched performance comparisons, complete-volume accounting,
or provider-independent guarantees. The ordinary `just check` gate passed 79
frontend and 141 Rust tests, plus formatting, type checking, and Clippy.
The production-protocol native WebView smoke at 620 by 480 also passed on a
disposable 1,025-file fixture, with no page errors, correct focus and Tab bounds,
metric switching, navigation, matching/empty searches, Home snapshot release,
and a second scan. Its first run timed out at the no-match-search step; an
unchanged retry passed. The cause of that first timeout was not established.
Optional cancellation and terminal-failure preflights were not supplied for
this smoke run; its default-true report fields are not evidence for those flows.

## Errors and concurrent changes

Permission failures, entries that disappear during traversal, and recoverable
metadata failures increment `skippedEntries`; they do not abort the entire
scan. Completed results with a nonzero `skippedEntries` count show a visible,
cause-neutral coverage notice because their totals may be lower than the space
actually in use. Intentional mount-boundary exclusions remain in `Scan details`
through `skippedFilesystems` and are not presented as scan failures. A root that
cannot be opened is a fatal error.

The filesystem remains live while Cepa scans it. Files created, removed, linked,
or resized during traversal can make the result differ from any single instant.
Cepa avoids undefined accounting and arithmetic overflow, but it does not claim
transactional snapshot semantics. For backend parity and performance evidence,
use a quiescent fixture.
