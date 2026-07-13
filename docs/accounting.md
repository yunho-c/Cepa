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
- Allocated size is physical blocks multiplied by 512 on Unix, native allocation
  attributes on macOS, and `FILE_STANDARD_INFO::AllocationSize` in the Windows
  MFT backend. The portable Windows fallback reports logical size as an estimate
  and marks that limitation in the result protocol.
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

## Errors and concurrent changes

Permission failures, entries that disappear during traversal, and recoverable
metadata failures increment `skippedEntries`; they do not abort the entire
scan. A root that cannot be opened is a fatal error.

The filesystem remains live while Cepa scans it. Files created, removed, linked,
or resized during traversal can make the result differ from any single instant.
Cepa avoids undefined accounting and arithmetic overflow, but it does not claim
transactional snapshot semantics. For backend parity and performance evidence,
use a quiescent fixture.
