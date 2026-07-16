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

Cepa provides read-only volume and file-state inspection. A writer must not ship
by manually constructing `com.apple.decmpfs` attributes or toggling
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

After each successful item Cepa queries compression state and the strongest
available allocation evidence again. After the job it rescans the smallest safe
common ancestor so the explorer does not display stale totals. Btrfs compressed
physical length remains a separate privileged evidence requirement; its ordinary
kernel block count is not exact. A successful API return without verified state
and readable original content is not a successful outcome.

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

The implemented estimator is explicit and per-file. Files up to 768 KiB are read
in full; larger files sample aligned 256 KiB ranges at the beginning, middle, and
end. Reads check cancellation every 64 KiB, open without following links, and
reject a size or Unix allocation that changed after scanning. Starting a new
estimate cancels superseded work, and internal generation tokens prevent a reused
wire request ID from clearing newer work. Each range is compressed independently
so unrelated ranges cannot create artificial cross-sample redundancy.

Windows samples with the exact 4 KiB-chunked LZNT1 codec. Btrfs samples with Zstd
level 3 in independent 128 KiB chunks, matching the filesystem's maximum
compression-extent size. macOS reports `zlib-proxy` because no supported writer
or target algorithm has been selected; its fidelity is always labeled `proxy`.
The result includes the codec, fidelity, sampled bytes, confidence, and lower and
upper allocated-savings bounds. Sparse files force the lower bound to zero.
The current version detects sparse allocation from scan totals but does not map
individual hole ranges, so it deliberately makes no minimum-savings claim for a
sparse file. No estimates are cached yet, which avoids violating the
identity-keyed cache contract above.

Linux Btrfs scans mark their allocation baseline as estimated. On the measured
fixture, both `st_blocks` and `statx` retained the 8 MiB referenced length after
Zstd reduced physical data to 256 KiB. The estimator can still describe likely
content savings, but validating its bounds against post-operation physical bytes
requires a privileged fixture; the ordinary scanner result is not that proof.

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

Step 1 is implemented at the protocol and backend level. Volume capability is
authorized by a completed scan ID and reports `inspectOnly`, `unsupported`, or
`unavailable` as data. Per-item inspection additionally requires an opaque node
ID and rejects directories, links, and other non-regular entries without
following them.

macOS queries `ATTR_VOL_CAPABILITIES` and reads `UF_COMPRESSED`; Windows queries
`GetVolumeInformationW` and `FSCTL_GET_COMPRESSION`; Linux identifies Btrfs with
`statfs` and reads `FS_IOC_GETFLAGS`. Btrfs results are explicitly scoped to
future writes because its inode flags do not prove existing extent state. The
macOS probes have been exercised against real uncompressed and `ditto`-compressed
APFS files plus a symlink-replacement fixture. Windows passes target-specific
compile checks and a native regular-file harness.

The Linux probe and inspector also pass a real Btrfs runtime fixture. On
2026-07-13, Rust 1.97.0 code ran against a 512 MiB loopback Btrfs filesystem on
kernel 6.8.0-124 with btrfs-progs 6.6.3. The repo-native fixture verified the
`inspectOnly` volume capability, all advertised algorithms, an always-false
writer flag, and both path and retained-handle inspection of explicit enabled,
explicit disabled, and inherited future-write policies. It also verified that
a replacement symlink is not followed, inspection does not change file contents,
and `jwalk` and `statx` produce identical accounting while both label allocated
size as estimated. The test remains ignored in the ordinary cross-filesystem
suite; on a writable mounted Btrfs directory, run:

```sh
just validate-btrfs-compression /mnt/btrfs
```

The runner creates and removes a uniquely named child fixture. Raw output is
preserved in
[`validation-results/2026-07-13-linux-btrfs-compression.txt`](validation-results/2026-07-13-linux-btrfs-compression.txt).
This closes the Step 1 Btrfs runtime gate only; it does not identify compressed
extents, measure estimator accuracy, or authorize mutation.

A separate encoded-extent observation explains the estimate label. An 8 MiB
dense file and its byte-identical Zstd rewrite both reported 16,384 512-byte
blocks through `stat`, while privileged Btrfs tree inspection measured the
rewrite at 262,144 physical bytes and FIEMAP marked its extents encoded. Raw
output is preserved in
[`validation-results/2026-07-13-linux-btrfs-compressed-allocation.txt`](validation-results/2026-07-13-linux-btrfs-compressed-allocation.txt).
FIEMAP exposes the encoded flag but no compressed physical length. The encoded
read ioctl returns raw compressed bytes but requires `CAP_SYS_ADMIN`; neither is
a suitable ordinary scan-time size query. Cepa therefore keeps the kernel block
count as a useful ranking estimate and does not claim exact Btrfs physical bytes.

Every platform reports `writerAvailable: false`. Inspection does not estimate
savings or infer state from logical and allocated bytes. Estimation is a separate
explicit request and neither operation authorizes mutation.

Step 1.5 implements a dormant, read-only `CompressionPlan` protocol. Preparation
accepts only a completed scan ID, opaque node ID, and requested operation. It
opens a regular file without following links, keeps that read-only file open as
the single active plan's identity anchor, checks scan size and exact allocation
where the scan backend provides it, and records an immutable platform-specific
metadata revision. Compression-state inspection during preparation uses the
same open file instead of reopening its path. The scanner retains a 24-byte Unix
revision plus one snapshot-level filesystem ID, or a 32-byte Windows revision,
when the backend supplies a nonzero stable file ID and exact timestamps. Unix
revisions include device, inode, modification time, and change time; Windows
revisions include volume serial, file ID, last-write time, and change time.
Preparation requires that retained scan revision to match the opened file, then
rechecks both the anchor and a fresh no-follow path binding before storing the
plan.

Revalidation first opens the current path without following links, snapshots the
retained file separately, and requires both revisions, sizes, allocations, and
link counts to agree with each other and the prepared revision. Preparation also
reads the complete retained file with positioned reads and stores a BLAKE3
digest. Revalidation recomputes it, with metadata snapshots before and after the
read, and rechecks the path binding afterward. The working buffer is fixed at
1 MiB, reads do not disturb the held handle's cursor, and a newer plan, new scan,
or Home transition cancels work between chunks. It returns `valid`, `changed`,
or `unavailable`; regular-file replacement, unlink, symlink replacement,
same-length mutation, a simulated metadata collision, positioned-read, and
cancellation fixtures cover those boundaries. Unix opens are nonblocking until
the regular-file type check completes, so replacing a scanned file with a FIFO
cannot stall the planning worker. The retained handle is shared only with an
already-running validation and otherwise closes with invalidation, so dormant
previews cannot accumulate open files.

Every preview currently contains a `writerUnavailable` blocker, no apply command
exists, and the UI intentionally exposes no dead-end planning action. The
retained scan snapshot rejects planning when a usable scan-time revision is
unavailable. The plan-time content digest does not turn scan metadata into a
content fingerprint: a same-clock-tick rewrite that occurs after scanning but
before the initial plan read may still be indistinguishable from the scanned
state. Hashing every file during traversal would violate the scanner's I/O and
performance contract. The identity anchor is also read-only; it proves that
plan validation can keep referring to the originally opened file, not that a
platform writer can mutate through that exact handle or prevent a change after
validation. A future writer must acquire the necessary rights without reopening
a mutable path and perform immediate byte-integrity verification before and
after the filesystem operation. This preview is not mutation authority.

The content-anchor candidate passed formatting, dependency-light check, strict
Clippy, and 75 tests on native Rust 1.97 Linux, with the real-Btrfs fixture
ignored. On macOS, all 46 frontend tests and 91 Rust library tests passed, one
file-manager-opening test remained intentionally ignored, and the optimized
desktop application built. The exact default-feature Windows target graph also
cross-compiled from macOS; native Windows runtime validation remains required
before treating its positioned-read path as proven. These checks strengthen
dormant plan validation only and do not satisfy the writable-backend gates
below.

The identity-anchor change was validated natively on macOS, Linux, and Windows.
The full macOS frontend/Rust suite and optimized desktop build passed. Rust
1.97.0 Linux formatting, check, warning-denied Clippy, 61 core tests, every
example test, and the FIFO replacement regression passed through the
dependency-light path. Rust 1.97.0 Windows formatting, check, warning-denied
Clippy, 51 core tests, every example test, and an optimized default-feature
build also passed. These checks prove the read-only handle lifecycle and native
API wiring; they do not satisfy the writer validation gates below.

Step 2 is implemented as a bounded, cancellable per-file estimator and candidate
UI. Deterministic tests cover sampling bounds, full small-file coverage,
compressible versus pseudo-random data, sparse-file conservatism, wire semantics,
and cancellation ownership. macOS runs the real proxy codec locally; the Windows
module and Linux platform code pass native isolated harnesses. Native estimator
accuracy on Btrfs remains a CI or external evidence gate; the Step 1 runtime
fixture above validates metadata inspection, not estimator fidelity.

1. Add capability and read-only state protocol on every platform; unsupported is
   a first-class result.
2. Add bounded local estimation and candidate UX without mutation.
3. Complete the mutation-capable held-handle and immediate writer-verification
   gate, then
   pilot explicit regular-file NTFS compression/decompression behind an
   experimental flag.
4. Add Btrfs future-write policy, then separately gate existing-extent rewrite.
5. Revisit a macOS writer only after the copy/replace research gates above pass.
6. Consider recursive plans and automation only after single-selection jobs have
   durable evidence and recovery semantics.

## Primary sources

- Microsoft: [`FSCTL_SET_COMPRESSION`](https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_set_compression),
  [`FSCTL_GET_COMPRESSION`](https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_get_compression),
  [`GetVolumeInformationW`](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationw),
  [`GetCompressedFileSizeW`](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-getcompressedfilesizew),
  [`GetFileInformationByHandleEx`](https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex),
  and [`FILE_BASIC_INFO`](https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_basic_info).
- Btrfs documentation: [Compression](https://btrfs.readthedocs.io/en/latest/Compression.html),
  [`btrfs-property`](https://btrfs.readthedocs.io/en/latest/btrfs-property.html),
  [`btrfs-filesystem defragment`](https://btrfs.readthedocs.io/en/latest/btrfs-filesystem.html#defragment),
  and the [`BTRFS_IOC_ENCODED_READ` contract](https://btrfs.readthedocs.io/en/latest/btrfs-ioctl.html).
- Linux kernel: [FIEMAP extent mapping](https://docs.kernel.org/filesystems/fiemap.html).
- Apple Foundation: [`NSURLVolumeSupportsCompressionKey`](https://developer.apple.com/documentation/foundation/nsurlvolumesupportscompressionkey).
- Local Apple evidence: `ditto(1)`, `copyfile(3)`, and the macOS 26.2 SDK
  Foundation/FSKit headers installed with Xcode.
