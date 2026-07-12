# Filesystem accounting semantics

Cepa reports a best-effort point-in-time view of a live directory tree. The
portable and native backends share these rules; an optimized backend must match
them before it can replace `jwalk` for a platform.

## Sizes and entry counts

- Logical size is the byte length reported for regular files. Directory,
  symbolic-link, and other entry types contribute zero direct bytes.
- Allocated size is physical blocks multiplied by 512 on Unix and the native
  allocation-size attribute on macOS. Platforms without an allocated-size API
  currently report logical size as an estimate and mark that limitation in the
  result protocol.
- Sparse files can therefore have a logical size larger than their allocated
  size. Cepa preserves both values instead of substituting one for the other.
- A directory's totals are the saturating sum of its accounted descendants.
  File and directory counts describe directory entries, not unique inodes.

The explorer defaults to space on disk. Switching to logical size requests a
new directory view from the retained Rust snapshot; ranking, the bounded top-500
list, recursive chart selection, aggregate remainder, percentages, and geometry
all use the selected metric. The summary retains both totals so the distinction
remains visible.

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
