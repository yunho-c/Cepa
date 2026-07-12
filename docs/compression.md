# Transparent filesystem compression RFC

Status: proposed implementation contract; no compression mutation is currently
shipped. Last researched on 2026-07-11 against the primary sources listed below
and the macOS 26.2 SDK/man pages installed on the development machine.

## Product intent

Cepa should eventually let users reclaim physical storage without changing how
applications open their files. “Transparent compression” means a filesystem
stores file data compressed while ordinary reads still return the original
bytes. It does not mean creating archives, changing file formats, uploading
content, or promising that every filesystem can compress files.

The first release must optimize for data integrity and intelligible behavior,
not the largest optimistic savings number. Compression is always explicit and
local. Cepa must never silently enable a volume-wide policy, recursively rewrite
a directory, follow a link, or treat `logicalBytes - allocatedBytes` as an
estimate of compressibility.

## Shared semantics

### Capability is per volume

An operating-system name is not a capability. Cepa probes the volume containing
the selected scan root and returns one of:

- `unsupported`: the filesystem has no supported transparent-compression path;
- `inspectOnly`: Cepa can report state but has no safe writer;
- `futureWrites`: a persistent inode or directory policy can affect later
  writes, but existing extents are unchanged;
- `rewriteExisting`: the backend can rewrite existing file data transparently;
- `unavailable`: support may exist, but permissions, tools, mount options, or
  runtime facilities are missing.

The response also names the backend, algorithms, operation limits, and a human
readable reason. The UI only offers actions supported by the selected volume.

### Physical size is not compression state

Sparse holes, allocation-unit rounding, clones, deduplication, hard links, and
filesystem metadata all affect allocated size. The scanner's existing logical
and allocated metrics remain useful evidence, but compression state requires a
backend-specific query. Candidate estimates and completed results must keep
these concepts separate:

- `logicalBytes`: application-visible file length;
- `allocatedBytesBefore` and `allocatedBytesAfter`: measured physical storage;
- `compressionState`: backend-reported policy/format;
- `estimatedSavings`: a bounded content-sampling estimate with a confidence
  label, never a promise.

### Identity and path safety

The frontend sends completed scan IDs and opaque node IDs, never mutation paths.
Before every operation Rust reconstructs the path from the retained snapshot and
revalidates filesystem identity, file identity where available, type, size, and
modification time. Changed or missing entries are skipped with an explicit
outcome.

Compression never follows symbolic links or reparse points and never crosses the
scan's filesystem boundary. Hard-linked content is operated on once per stable
file identity; changing one link changes the shared file. Directories are not an
implicit recursive selection.

## Platform contracts

### Windows: NTFS pilot

Windows is the first writable backend candidate because it exposes documented
file controls:

- Probe `FILE_FILE_COMPRESSION` with `GetVolumeInformationW` rather than relying
  on the `NTFS` name alone.
- Read the per-stream format with `FSCTL_GET_COMPRESSION`.
- Apply `COMPRESSION_FORMAT_DEFAULT` or `COMPRESSION_FORMAT_NONE` with
  `FSCTL_SET_COMPRESSION` through `DeviceIoControl`.
- Measure physical storage with `GetCompressedFileSizeW`; its result also
  reflects sparse allocation, so it is not by itself proof of compression.

`FSCTL_SET_COMPRESSION` is synchronous, uses LZNT1 for the default format, and
documents a maximum uncompressed file size of 30 GB. Directory operations only
set the default state for subsequently created files; they do not compress the
directory's existing children. ReFS and several clustered/transparent-failover
SMB modes are unsupported. The backend must surface these limits during planning
and run the blocking control call off the Tauri async executor. Cancellation is
observed between files, never by abandoning a control call mid-operation.

The pilot operates on explicitly selected regular files only. Recursive
directory policy and network paths remain out of scope until they have dedicated
semantics and fixtures.

### Linux: Btrfs policy before rewrite

Btrfs supports ZLIB, LZO, and ZSTD transparent compression, but setting a mount
option or inode property affects newly written data; existing extents are left
untouched. Cepa therefore treats these as separate capabilities:

- `futureWrites`: set or clear the inode compression property with a direct,
  documented interface and report the selected algorithm;
- `rewriteExisting`: rewrite file extents with compression in bounded ranges,
  equivalent in effect to `btrfs filesystem defragment -c`.

Existing-data rewrite is the higher-risk operation. Btrfs documentation warns
that defragmentation can break reflinks and substantially increase space usage.
Before enabling it, Cepa must inspect extent sharing (for example via FIEMAP's
`FIEMAP_EXTENT_SHARED`) and reject shared extents by default. It must also reject
incompatible checksum/COW states, account for temporary free-space needs, and
step through bounded ranges so cancellation remains responsive. Merely spawning
`btrfs` and parsing localized command output is acceptable for a research spike,
not the production backend.

Other Linux filesystems remain `unsupported` until a separate, documented
contract exists. The Linux OS alone is never reported as Btrfs capability.

### macOS: inspect first, writer blocked on proof

Foundation exposes a read-only volume capability indicating support for
transparent decompression, and the installed `ditto(1)` documents
`--hfsCompression` for copy/extract operations onto supporting HFS+ or APFS
volumes. The public `copyfile(3)` interface documents clone, sparse-copy, and
no-follow behavior, but the inspected SDK does not expose a supported in-place
compression operation.

Cepa can first add read-only volume and file-state inspection. A writer must not
ship by manually constructing `com.apple.decmpfs` attributes or toggling
`UF_COMPRESSED`; those are implementation details, not a supported mutation
contract. A copy-compress-replace prototype using `ditto` is not production-safe
until it proves, on both APFS and HFS+:

- atomic replacement and crash behavior;
- preservation of ACLs, extended attributes, ownership, timestamps, forks, and
  quarantine metadata;
- correct handling or deliberate rejection of hard links and APFS clones;
- no-follow behavior for source and destination paths;
- predictable temporary-space requirements and rollback.

Until that evidence exists, macOS reports `inspectOnly`, not a writable feature.

## Rust architecture

Compression belongs beside scanning in Rust, behind a platform-neutral boundary;
it does not belong in Svelte components. The concrete names may evolve, but the
protocol needs these responsibilities:

```text
CompressionBackend
  probe(volume) -> CompressionCapability
  inspect(validated_file) -> CompressionState
  estimate(validated_file, algorithm, budget) -> SavingsEstimate
  apply(validated_file, operation, cancellation) -> ItemOutcome
  verify(validated_file, expected_identity) -> VerifiedState
```

A `CompressionPlan` is immutable and tied to a completed scan ID. It records the
selected node IDs, their expected identities and metadata, requested algorithm,
estimated read/write work, required free-space margin, unsupported/skipped items,
and estimate confidence. Applying a plan creates a job with a new ID; it cannot
silently absorb files added to a directory after planning.

Jobs stream bounded progress over a Tauri channel, use bounded worker and I/O
queues, and store one terminal outcome per item. Mutation parallelism defaults to
one because compression is CPU-, I/O-, and thermal-intensive; a backend may raise
that only from measurement. The bridge receives aggregate counters and bounded
recent outcomes, never an event for each data block.

After each successful item Cepa queries compression state and allocated size
again. After the job it rescans the smallest safe common ancestor so the explorer
does not display stale physical totals. A successful API return without verified
state and readable original content is not a successful outcome.

## Estimation

Estimation is a separate cancellable, read-only job. It samples bounded data
ranges locally and uses the exact target algorithm when a compatible userspace
implementation exists. Sampling must account for holes where the platform can
report them and cache results only by stable file identity, size, modification
time, algorithm, and estimator version.

Extension-based exclusions may avoid obviously pre-compressed formats, but they
cannot be the only decision. A low-confidence sample is labeled accordingly.
Forcing compression of incompressible data is not the default; Btrfs itself uses
heuristics and can mark files `NOCOMPRESS` after failed attempts.

## User experience

The action begins from explicit file selections. A preview shows:

- supported, skipped, changed, already-compressed, sparse, and shared-extent
  counts;
- logical and allocated bytes measured now;
- estimated savings as a range and confidence, not a single guaranteed value;
- expected reads/writes, algorithm, filesystem, and operation constraints;
- the consequences of cancellation and the fact that decompression may require
  additional free space.

Confirmation names the affected root and requires an affirmative action. During
work the UI has preparing, running, cancelling, partially completed, failed, and
verified-complete states. Cancellation stops scheduling new files and waits for
the current filesystem operation to return. Per-item errors remain reviewable;
they do not turn partial completion into an all-or-nothing fiction.

## Validation gates

No writable backend becomes automatic or leaves an experimental label until it
passes all applicable gates:

1. Byte-for-byte content and metadata verification across compress/decompress
   round trips, including empty, small, large, incompressible, sparse, hard-linked,
   cloned/reflinked, permission-restricted, open, and concurrently modified files.
2. No-follow and same-filesystem adversarial fixtures, including link replacement
   between plan and apply.
3. Cancellation and process-crash injection at every between-file/range boundary,
   with truthful partial outcomes after restart.
4. Low-free-space and quota tests proving that failures do not corrupt data or
   strand an unreported temporary copy.
5. Native filesystem tests: NTFS on Windows; Btrfs with shared and unshared extents
   on Linux; APFS and HFS+ for any future macOS writer.
6. Measured estimator error, throughput, CPU, peak memory, write amplification,
   cancellation latency, and post-operation allocated bytes on representative
   compressible and incompressible datasets.
7. End-to-end accessible UI validation for every preview, confirmation, progress,
   partial-failure, cancellation, and verified-complete state.

GitHub-hosted Linux runners normally do not provide a Btrfs test volume, and
compilation on another filesystem is not backend proof. Platform-specific test
infrastructure must record its filesystem and kernel/OS details with the result.

## Rollout order

1. Add capability and read-only state protocol on every platform; unsupported is
   a first-class result.
2. Add bounded local estimation and candidate UX without mutation.
3. Pilot explicit regular-file NTFS compression/decompression behind an
   experimental flag.
4. Add Btrfs future-write policy, then separately gate existing-extent rewrite.
5. Revisit a macOS writer only after the copy/replace research gates above pass.
6. Consider recursive plans and automation only after single-selection jobs have
   durable evidence and recovery semantics.

## Primary sources

- Microsoft: [`FSCTL_SET_COMPRESSION`](https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_set_compression),
  [`FSCTL_GET_COMPRESSION`](https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_get_compression),
  [`GetVolumeInformationW`](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationw),
  and [`GetCompressedFileSizeW`](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-getcompressedfilesizew).
- Btrfs documentation: [Compression](https://btrfs.readthedocs.io/en/latest/Compression.html),
  [`btrfs-property`](https://btrfs.readthedocs.io/en/latest/btrfs-property.html),
  and [`btrfs-filesystem defragment`](https://btrfs.readthedocs.io/en/latest/btrfs-filesystem.html#defragment).
- Linux kernel: [FIEMAP extent mapping](https://www.kernel.org/doc/html/latest/filesystems/fiemap.html).
- Apple Foundation: [`NSURLVolumeSupportsCompressionKey`](https://developer.apple.com/documentation/foundation/nsurlvolumesupportscompressionkey).
- Local Apple evidence: `ditto(1)`, `copyfile(3)`, and the macOS 26.2 SDK
  Foundation/FSKit headers installed with Xcode.
