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
  `FILE_STANDARD_INFO::AllocationSize` in both Windows backends, and physical
  blocks multiplied by 512 on other Unix filesystems.
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

- Symbolic links are listed but never followed and contribute no target bytes.
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
virtual filesystems. Windows uses the separate protection described below;
Linux provider-specific protection is not implemented.

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

## Cloud-backed storage on Windows

Windows scans exclude entries with `FILE_ATTRIBUTE_OFFLINE` or
`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`. Directory enumeration also checks
`FILE_ATTRIBUTE_RECALL_ON_OPEN`; that bit is deliberately **not** interpreted as
recall in basic metadata or USN records, where the same value can mean extended
attributes. Pin/unpin intent does not establish residency. Downloaded Cloud
Files files and directories remain included, including unpinned local copies.
There is no provider-name blacklist.

The volume-root MFT backend filters known placeholders before retaining nodes
or dispatching file measurement, and prunes excluded directory subtrees. It
queries reparse tags through metadata-only file-ID handles so Cloud Files tags
are distinguished from symbolic links and junctions. Workers expose placeholder
metadata with a scoped, thread-bound compatibility guard and recheck recall
state through their opened handles. A file evicted after index admission may
remain as a zero-byte observed node; its exclusion is counted as cloud-related,
not as a permission error, and it is not ranked as a measured file.

Folder scans and the MFT fallback use the `win32` backend. The explicit `jwalk`
selector also resolves to `win32` on Windows and reports that actual backend:
the standard-library enumeration used by jwalk cannot request local-only
population. Other platforms keep their existing fallback. The Windows walker:

- Resolves roots component by component using metadata-only, no-reparse,
  no-recall opens relative to held parent handles. It rejects cloud-only roots
  and cloud-only ancestors. Selected paths through symbolic links or junctions
  are rejected rather than resolved implicitly.
- Enumerates held directory objects with `NtQueryDirectoryFileEx` and
  `SL_RETURN_ON_DISK_ENTRIES_ONLY`. It never retries without that flag.
- Rechecks each child's attributes before measurement or descent, retains only
  recognized downloaded Cloud Files reparse points as ordinary items, and
  preserves the existing no-follow treatment of other reparse points.
- Reads allocation, hard-link identity, and revision metadata through the same
  child handle without requesting file-content access. Hard-link deduplication
  is enabled for NTFS; other filesystems do not use the retained 64-bit ID for
  deduplication. Metadata inspection uses at most eight workers and 32-entry
  handoffs; each worker installs and restores placeholder exposure before IO.
  Cancellation is checked
  per entry and progress retains the shared adaptive clock and 100 ms cadence.

Placeholder compatibility mode only exposes metadata; it does **not** itself
prevent hydration. The handle-relative opens, no-recall/no-reparse flags, and
on-disk-only enumeration are all parts of the protection. Directory handles are
reopened relative to the already-checked held object; a pathname replacement
cannot change its identity or turn directory-list access into file-content access.

The implementation follows Microsoft's [file attribute definitions](https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants),
[placeholder compatibility mode](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-rtlsetthreadplaceholdercompatibilitymode),
and [on-disk directory query flags](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfileex).
The supported API baseline is Windows 10 version 1709 or later. Qualification
covers Windows Cloud Files on NTFS. Arbitrary network filesystems, third-party
virtual drives, and provider-specific filter behavior are not a universal
no-network guarantee. Unsupported local-only enumeration fails closed: a root
failure terminates the scan, while a child failure is an unavailable item.
Google Drive streaming on G14W is a measured exception to attribute-based
residency checks: its FAT32 virtual drive accepted the protected calls but
exposed ordinary attributes and virtual allocation sizes. The initial test
counted cloud entries with zero cloud exclusions; see the
[provider-specific test results](validation-results/2026-09-25-google-drive-windows.md).

Cepa now rejects a Google streaming volume or a path inside it before descendant
resolution or enumeration, and omits that volume from local storage discovery.
It enumerates live driver objects under the Windows `\FileSystem` object
directory and checks recognized `googledrivefs` drivers, including numeric
version suffixes, against the held volume using
[`FileFsDriverPathInformation`](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_fs_driver_path_information).
This kernel query is independent of the filesystem and sends no request to the
provider. Discovery hides only confirmed Google streaming volumes. Driver
inspection failures remain visible through scan validation and never cause an
unprotected retry. The driver list is refreshed for each root check rather than
cached across mount changes.

This excludes the entire streaming namespace, including cached files accessed
through it: Google does not expose usable per-file residency in the measured
Windows metadata. A selected stream root produces an actionable error, not an
empty result or an invented cloud-exclusion count. The local disk containing
Drive's cache and ordinary mirrored folders can still be scanned. Folder names,
volume labels, drive letters, and FAT32 are not exclusion criteria. This does not
establish support for every future Google driver or other proprietary virtual
filesystem.
As on macOS, skipped counts describe observed entries, not unknown descendants.
Virtual entries omitted entirely by the filesystem are not counted. Local data
beneath an excluded directory and provider caches are not a complete provider
storage inventory.

Windows tests include a disposable, connected Cloud Files provider. It creates
online-only files and a directory plus downloaded files and a downloaded
cloud directory; both walkers must agree, leave placeholders unavailable, and
produce zero data-fetch or directory-population callbacks. An ordinary content
read afterward is a positive control for the callback counter. The MFT fixture
uses the production pipeline with a test-only subtree admission; production
MFT selection remains volume-root-only.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features `
  cloud_files_fixture -- --ignored --nocapture

# Read-only existing-provider check; choose a small, already populated root
# containing unavailable direct children. Never creates or evicts user files.
$env:CEPA_CLOUD_FIXTURE_ROOT = 'C:\path\to\existing\cloud\folder'
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features `
  existing_cloud_placeholders -- --ignored --nocapture

# Read-only rejection test against an existing Google streaming volume.
$env:CEPA_GOOGLE_DRIVE_ROOT = 'G:\'
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features `
  google_drive_stream_volume -- --ignored --nocapture
```
