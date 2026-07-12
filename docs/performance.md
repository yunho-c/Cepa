# Scanner performance

Cepa treats performance as measured behavior, not a design claim. This document
defines the scanner benchmark, records reproducible portable and native results,
and identifies what the numbers do and do not prove.

## Running the benchmark

Generate a metadata-heavy fixture at a new path. The generator refuses to touch
an existing path.

```sh
just benchmark-fixture /tmp/cepa-fixture 1000 100 0
```

The positional parameters are path, directory count, files per directory, and
logical bytes per file. Nonzero file sizes use sparse `set_len`, so they model
logical size without promising physical allocation. The generator adds a JSON
manifest inside the fixture; reported file counts therefore include one extra
file.

Run the complete portable pipeline in release mode:

```sh
just benchmark-scan /tmp/cepa-fixture 9 jwalk > /tmp/cepa-report.json
```

On macOS, pass `getattrlistbulk` instead of `jwalk` to compare the native
backend on the identical fixture. On Linux, pass `statx`. `auto` measures the
backend selected by the desktop application. The default remains `jwalk` so
historical portable baselines stay directly reproducible.

The harness performs one unmeasured warmup followed by the requested measured
runs. Progress is written to stderr and schema-versioned JSON to stdout. It
verifies that counts, byte totals, skipped work, hard-link deduplication, and
reported accounting semantics stay identical across runs.

Compare two backends on a quiescent tree:

```sh
just validate-scan /path/to/tree jwalk getattrlistbulk
# Linux:
just validate-scan /path/to/tree jwalk statx
```

The parity command compares every correctness-relevant `ScanResult` field,
prints a JSON report, and exits nonzero on a mismatch. Filesystem mutations
between its sequential scans can produce a legitimate mismatch and should be
eliminated before treating a failure as a backend bug.

Measure cancellation initiated by a separate thread after a progress boundary:

```sh
just benchmark-cancellation /tmp/cepa-fixture getattrlistbulk 9 2048
# Linux:
just benchmark-cancellation /tmp/cepa-fixture statx 9 2048
```

This performs one complete warmup, then reports scan elapsed time and the time
from cancellation request to scanner return for each cancelled run.

Each run measures:

- `traversalUs`: backend traversal, metadata reads, arena insertion, and path
  indexing.
- `aggregationUs`: bottom-up propagation of file and directory totals.
- `indexingUs`: post-aggregation index finalization. This is currently zero
  because the arena builds its indexes incrementally during traversal.
- `scannerElapsedMs`: the three scanner phases together.
- `initialViewMs`: bounded selection and materialization of the initial radial
  chart and top-500 list.
- `wallMs`: scanner plus initial view and small harness overhead.

The benchmark deliberately retains the snapshot until after timing, matching
the application, which needs it for drill-down.

## 2026-07-11 portable baseline

Environment: Apple M4 Pro, 14 logical CPUs, 48 GiB RAM, macOS 15.6, APFS-backed
`/private/tmp`, `aarch64`, release profile, `jwalk` backend. The APFS data volume
was at 95% capacity during measurement. Results are medians of nine warmed runs.

| Workload | Entries | Wall time | Entries/s | Traversal | Aggregation | Initial view |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Directory-rich: 10,000 leaf directories × 10 files | 110,101 | 141.55 ms | 777,818 | 140.46 ms | 1.02 ms | 0.17 ms |
| Wide: 100 leaf directories × 1,000 files | 100,102 | 94.16 ms | 1,063,138 | 93.31 ms | 0.64 ms | 0.13 ms |

The raw runs are preserved in
[`performance-results/2026-07-11-m4-pro-basename.csv`](performance-results/2026-07-11-m4-pro-basename.csv).

## 2026-07-11 macOS native comparison

The same machine, fixtures, release profile, one-run warmup, and nine-run
methodology were used to compare the portable and macOS-native backends. Counts,
logical bytes, allocated bytes, and skipped-entry counts matched exactly across
both implementations and every measured run.

The native traversal uses a bounded worker pool. It begins with four workers to
avoid APFS contention in trees dominated by small directories, then expands to
at most eight after observing a directory batch with at least 256 files. At
most twice the maximum worker count can be queued in either direction.

| Workload | Backend | Wall time | Entries/s | Traversal | Aggregation | Initial view |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Directory-rich | `jwalk` | 141.95 ms | 775,646 | 140.79 ms | 1.00 ms | 0.18 ms |
| Directory-rich | `getattrlistbulk` | 86.01 ms | 1,280,164 | 85.14 ms | 0.69 ms | 0.17 ms |
| Wide | `jwalk` | 90.65 ms | 1,104,318 | 89.82 ms | 0.67 ms | 0.13 ms |
| Wide | `getattrlistbulk` | 14.49 ms | 6,909,682 | 13.70 ms | 0.64 ms | 0.13 ms |

Against `jwalk`, median native wall time was 39.4% lower on the
directory-rich fixture and 84.0% lower on the wide fixture. Median throughput
was respectively 65.0% and 525.7% higher. Native wide runs varied from 13.43 to
45.69 ms, but the slowest native run remained faster than the fastest portable
run in this sample.

The raw runs are preserved in
[`performance-results/2026-07-11-m4-pro-native-comparison.csv`](performance-results/2026-07-11-m4-pro-native-comparison.csv).

## Real-tree parity and retained memory

A quiescent Cepa development checkout provided a non-synthetic APFS workload:
7.7 GiB containing `node_modules`, Rust build outputs, source, Git metadata,
generated frontend assets, and 29,929 duplicate hard links. The parity harness
reported exact agreement on 66,031 files, 2,888 directories, logical and
allocated bytes, skipped entries and filesystems, duplicate hard links, and all
three accounting-semantics flags.

One warmed measured scan per backend was also run under macOS
`/usr/bin/time -l`. These are memory observations, not a statistically robust
throughput comparison.

| Backend | Wall time | Peak RSS | Peak memory footprint |
| --- | ---: | ---: | ---: |
| `jwalk` | 210.48 ms | 59.19 MiB | 35.33 MiB |
| `getattrlistbulk` | 32.81 ms | 37.31 MiB | 27.67 MiB |

Native peak RSS was 37.0% lower in this observation. The exact workload and
measurements are preserved in
[`performance-results/2026-07-11-m4-pro-real-tree.csv`](performance-results/2026-07-11-m4-pro-real-tree.csv).

## Cancellation latency

Cancellation was requested from a separate thread at the first progress update
at or beyond 2,048 entries. Each backend received one complete warmup followed
by nine cancellation runs on both 100k-entry fixtures.

| Workload | Backend | Median latency | Maximum latency |
| --- | --- | ---: | ---: |
| Directory-rich | `jwalk` | 414 us | 859 us |
| Directory-rich | `getattrlistbulk` | 103 us | 232 us |
| Wide | `jwalk` | 998 us | 5,980 us |
| Wide | `getattrlistbulk` | 698 us | 813 us |

The raw measurements are preserved in
[`performance-results/2026-07-11-m4-pro-cancellation.csv`](performance-results/2026-07-11-m4-pro-cancellation.csv).

## Deterministic hard-link ownership check

After making hard-link byte ownership independent of traversal order, the local
Cepa checkout provided a hard-link-heavy validation workload: 77,283 entries,
36,141 duplicate hard links, and 9.14 GB allocated. A parity scan matched every
correctness-relevant field between `jwalk` and `getattrlistbulk`. One warmup and
nine measured release runs produced:

| Backend | Wall time | Entries/s | Traversal | Aggregation |
| --- | ---: | ---: | ---: | ---: |
| `jwalk` | 236.50 ms | 326,775 | 235.96 ms | 0.45 ms |
| `getattrlistbulk` | 37.52 ms | 2,059,779 | 37.00 ms | 0.46 ms |

The checkout contained more build artifacts than the earlier real-tree
baseline, so these numbers establish a new correctness-change baseline rather
than a like-for-like speedup or regression claim. The raw runs are preserved in
[`performance-results/2026-07-11-m4-pro-hardlink-ownership.csv`](performance-results/2026-07-11-m4-pro-hardlink-ownership.csv).

## Snapshot memory baseline

Peak resident memory was measured by running one warmup plus one measured scan
under macOS `/usr/bin/time -l`. The current arena retains one basename per node,
one root path, and compact node IDs for all relationships and navigation.

| Workload | Path map | Full-path arena | Basename arena | Total reduction |
| --- | ---: | ---: | ---: | ---: |
| Directory-rich | 108.36 MiB | 62.72 MiB | 35.69 MiB | 67.1% |
| Wide | 101.28 MiB | 67.38 MiB | 46.47 MiB | 54.1% |

The exact memory observations are preserved in
[`performance-results/2026-07-11-m4-pro-memory.csv`](performance-results/2026-07-11-m4-pro-memory.csv).

## Optimization history

### Bounded child ranking

The first measurement showed that eagerly sorting every directory's complete
child list dominated a wide scan. Cepa only displays the top 500 list entries
and top 16 chart segments, so the snapshot now keeps unsorted child indexes and
uses partial selection plus a bounded sort when a directory is opened.

On the same fixtures, the directly comparable internal phase medians changed as
follows:

| Workload | Scanner total | Indexing |
| --- | ---: | ---: |
| Directory-rich | 279 ms → 197 ms (-29%) | 99 ms → 25 ms (-75%) |
| Wide | 291 ms → 139 ms (-52%) | 166 ms → 14 ms (-92%) |

The pre-change harness discarded the snapshot before returning while the final
harness retains it and builds the initial view. For that reason, only internal
scanner phase timings—not the pre-change wall time—are used for this comparison.

The historical raw runs are preserved in
[`performance-results/2026-07-11-m4-pro.csv`](performance-results/2026-07-11-m4-pro.csv).

### Index-based arena snapshot

The initial path-map representation owned or cloned full paths in node keys,
parent links, child indexes, and aggregation work. The arena representation
stores nodes contiguously, shares one path allocation between each node and the
lookup index, and represents relationships with integer IDs. Reverse arena
iteration also replaces the depth-sort aggregation pass.

Compared with the bounded-ranking baseline above on identical fixtures:

| Workload | Wall time | Entries/s | Initial view |
| --- | ---: | ---: | ---: |
| Directory-rich | 200.73 → 136.59 ms (-32.0%) | 548,502 → 806,045 (+47.0%) | 3.11 → 0.78 ms (-74.8%) |
| Wide | 147.82 → 100.68 ms (-31.9%) | 677,192 → 994,217 (+46.8%) | 8.39 → 0.97 ms (-88.4%) |

The full-path arena runs are preserved in
[`performance-results/2026-07-11-m4-pro-arena.csv`](performance-results/2026-07-11-m4-pro-arena.csv).

### Basename retention and opaque node IDs

The retained snapshot no longer stores an absolute path for every entry. It
keeps each basename once and reconstructs only the current directory path when
needed. Directory navigation now uses bounds-checked opaque node IDs, so list,
chart, and breadcrumb payloads do not repeat full paths either. `jwalk`'s
depth-first iterator order supplies parent IDs through a depth stack without a
path lookup table.

Compared with the full-path arena:

| Workload | Wall time | Entries/s | Initial view | Peak RSS |
| --- | ---: | ---: | ---: | ---: |
| Directory-rich | 136.59 → 141.55 ms (+3.6%) | 806,045 → 777,818 (-3.5%) | 0.78 → 0.17 ms (-77.8%) | 62.72 → 35.69 MiB (-43.1%) |
| Wide | 100.68 → 94.16 ms (-6.5%) | 994,217 → 1,063,138 (+6.9%) | 0.97 → 0.13 ms (-86.8%) | 67.38 → 46.47 MiB (-31.0%) |

The directory-rich throughput tradeoff is retained intentionally: avoiding
repeated ancestor bytes materially improves the multi-million-entry memory
ceiling, while the measured regression is small and explicit.

## Linux native validation contract

Linux `auto` first attempts the native backend and falls back to `jwalk` when
`statx` is unavailable or does not report both basic metadata and mount IDs. The
native traversal reads 64 KiB `getdents64` buffers through `rustix::fs::RawDir`,
requests no-follow `statx` metadata relative to open directory descriptors, and
passes at most 512 entries per result batch. The task queue is bounded to twice
the worker count and the result queue to sixteen times the worker count, with the
pool capped at eight workers. Neither queue grows with the scanned tree.

Directories are opened relative to a retained parent descriptor only when a
worker schedules them. The opened descriptor's device, inode, and mount ID must
still match the discovery snapshot before traversal. A changed directory is
skipped, symlinks are not followed, and a different `stx_mnt_id` is recorded as
a skipped filesystem. Hard-link identity uses device plus inode, while physical
size uses `stx_blocks * 512`.

Linux-native unit tests cover automatic selection, portable parity, hard-link
ownership, no-follow symlinks, bounded progress cancellation, and file-type and
device mapping. CI additionally generates a 4,096-file fixture, adds hard-link
and external-symlink cases, runs the parity harness, and measures three native
cancellation runs. These are correctness and responsiveness gates, not a
throughput baseline. No Linux speedup is claimed until native release benchmarks
are recorded with the environment and raw results.

The syscall contracts are documented by the Linux man-pages for
[`getdents64`](https://man7.org/linux/man-pages/man2/getdents.2.html) and
[`statx`](https://man7.org/linux/man-pages/man2/statx.2.html); the safe wrappers
used here are documented by
[`rustix::fs::RawDir`](https://docs.rs/rustix/latest/rustix/fs/struct.RawDir.html)
and [`rustix::fs::statx`](https://docs.rs/rustix/latest/rustix/fs/fn.statx.html).

### 2026-07-11 Linux container correctness run

The complete Rust/Tauri test suite and strict Clippy passed in an arm64 Debian
Bookworm container with Rust 1.88.0, LinuxKit kernel 5.15.49, Docker Desktop
24.0.5, and a 4 KiB-block overlayfs fixture. This validates a real Linux syscall
path but not a representative native installation or storage device.

The debug-profile parity harness matched all correctness fields for 4,099 files,
33 directories, one duplicate hard link, a directory symlink outside the root,
16,777,411 logical bytes, and 8,192 allocated bytes. Three debug cancellation
runs returned in 271–334 us after the progress boundary. Those timings only
bound this fixture's responsiveness; they are not release throughput results.
The raw reports are preserved in
[`performance-results/2026-07-11-linux-container-statx-parity.json`](performance-results/2026-07-11-linux-container-statx-parity.json)
and
[`performance-results/2026-07-11-linux-container-statx-cancellation.json`](performance-results/2026-07-11-linux-container-statx-cancellation.json).

### 2026-07-12 native Linux ext4 validation and throughput

The current scanner module graph was compiled and run natively through the
isolated release harness on a 13th-generation Intel Core i9-13900K workstation
with 32 logical CPUs, 125 GiB RAM, Linux 6.8.0, Rust 1.97.0, and NVMe-backed
ext4. The harness passed all 38 scanner, compression, and plan tests. This is a
native backend result, but it excludes the Tauri shell and frontend.

The adversarial parity fixture contained 4,099 files, 33 directories, one hard
link, and a directory symlink outside the root. `jwalk` and `statx` agreed on
every accounting field. A separate user/mount namespace bound `/etc` beneath
the fixture on the same ext4 device; `statx` retained the mount-point node,
reported one skipped filesystem, and did not traverse it. The exact counters are
preserved in
[`performance-results/2026-07-12-linux-ext4-validation.csv`](performance-results/2026-07-12-linux-ext4-validation.csv).

The canonical fixtures received one warmup and nine measured release runs per
backend. The real-tree workload was the quiescent remote Cepa checkout after the
harness build, including Rust artifacts. Values below are medians.

| Workload | Entries | Backend | Wall time | Entries/s | Traversal |
| --- | ---: | --- | ---: | ---: | ---: |
| Directory-rich | 110,101 | `jwalk` | 26.19 ms | 4,205,000 | 25.58 ms |
| Directory-rich | 110,101 | `statx` | 33.66 ms | 3,271,000 | 33.02 ms |
| Wide | 100,102 | `jwalk` | 14.11 ms | 7,092,000 | 13.65 ms |
| Wide | 100,102 | `statx` | 19.34 ms | 5,177,000 | 18.78 ms |
| Real tree | 3,133 | `jwalk` | 2.45 ms | 1,280,000 | 2.43 ms |
| Real tree | 3,133 | `statx` | 2.75 ms | 1,141,000 | 2.71 ms |

On this warm ext4 sample, native wall time was 28.5% higher for the
directory-rich fixture, 37.0% higher for the wide fixture, and 12.2% higher for
the real tree. No Linux speedup is claimed. The raw runs are preserved in
[`performance-results/2026-07-12-linux-ext4-statx.csv`](performance-results/2026-07-12-linux-ext4-statx.csv).

Cancellation after the first progress boundary at or beyond 2,048 entries had
a 200 us median and 249 us maximum on the directory-rich fixture, and a 205 us
median and 233 us maximum on the wide fixture. Cancellation remained bounded
despite the native throughput deficit.

Two interleaved tuning experiments were rejected rather than optimized for one
shape. Expanding from eight to sixteen workers after a 256-entry result batch
reduced wide median wall time from 22.45 ms to 15.37 ms, but increased the
directory-rich median from 43.35 ms to 50.49 ms. Combining each small
directory's final result batch and completion message improved the wide median
by 3.2% but regressed the directory-rich median by 8.2%. The production
eight-worker scheduler remains unchanged. Raw trials are preserved in
[`performance-results/2026-07-12-linux-worker-scaling.csv`](performance-results/2026-07-12-linux-worker-scaling.csv)
and
[`performance-results/2026-07-12-linux-message-collapse.csv`](performance-results/2026-07-12-linux-message-collapse.csv).

An instrumented `strace -f -c` diagnostic is not valid for wall-time comparison,
but its call counts show where to investigate next. Both backends issued about
120,000 metadata syscalls on the directory-rich fixture. Native traversal made
28,266 `sched_yield` calls versus 6,950 for `jwalk`, consistent with scheduler
and channel coordination—not fewer metadata queries—being the next architectural
target. The counts are preserved in
[`performance-results/2026-07-12-linux-strace-counts.csv`](performance-results/2026-07-12-linux-strace-counts.csv).

#### Bounded result-queue backpressure

The native workers originally shared the same two-slots-per-worker bound for
directory tasks and result messages. Results are substantially burstier: every
small directory emits an entry batch and completion, while the main thread also
performs arena insertion, hard-link ownership, progress ranking, and child-task
scheduling. A capacity sweep compared two, four, eight, and sixteen result slots
per worker while keeping eight workers, the two-slots-per-worker task queue, and
512-entry batches unchanged.

The selected bound is sixteen result messages per worker: at most 128 queued
messages with the current eight-worker cap. A longer 15-round interleaved A/B
against the original bound produced these medians:

| Workload | Two slots/worker | Sixteen slots/worker | Change |
| --- | ---: | ---: | ---: |
| Directory-rich | 47.35 ms | 40.85 ms | -13.7% |
| Wide | 20.41 ms | 20.24 ms | -0.9% |

One warmed `/usr/bin/time -v` observation per variant found 15.0–16.0 MiB peak
RSS, with no monotonic increase from the larger queue and at most 768 KiB between
paired observations. This is process RSS rather than queue-owned memory, but it
rules out an obvious fixture-scale memory regression. The theoretical backlog
remains bounded to 65,536 entries before names and vector overhead, independent
of total scan size.

Twenty-one interleaved cancellation runs per workload also remained responsive:

| Workload | Bound | Median | p95 | Maximum |
| --- | --- | ---: | ---: | ---: |
| Directory-rich | Two | 223 us | 272 us | 302 us |
| Directory-rich | Sixteen | 208 us | 253 us | 267 us |
| Wide | Two | 209 us | 246 us | 246 us |
| Wide | Sixteen | 205 us | 252 us | 266 us |

Native release tests, adversarial `jwalk` parity, and same-device bind-mount
enforcement passed again with the selected bound. Instrumented syscall counts
showed `futex` calls falling from 2,674 to 2,449 while `sched_yield` calls were
effectively unchanged, so broader scheduler redesign remains separate future
work rather than a claim attached to this tuning.

Raw evidence is preserved in
[`performance-results/2026-07-12-linux-result-queue-scaling.csv`](performance-results/2026-07-12-linux-result-queue-scaling.csv),
[`performance-results/2026-07-12-linux-result-queue-final.csv`](performance-results/2026-07-12-linux-result-queue-final.csv),
[`performance-results/2026-07-12-linux-result-queue-cancellation.csv`](performance-results/2026-07-12-linux-result-queue-cancellation.csv),
[`performance-results/2026-07-12-linux-result-queue-memory.csv`](performance-results/2026-07-12-linux-result-queue-memory.csv),
and
[`performance-results/2026-07-12-linux-result-queue-strace.csv`](performance-results/2026-07-12-linux-result-queue-strace.csv).

### 2026-07-12 scan-time revision retention

Compression planning now retains a 32-byte file identity/revision field in each
scanner node and populates it for regular files. `Option<ScannedFileRevision>`
is also exactly 32 bytes through a nonzero file-ID niche, so unavailable
revisions need no additional tag. Native macOS and
Linux obtain the timestamps from their existing batched metadata records;
Windows adds one `FileBasicInfo` query to the already-open file-ID handle.

Fresh interleaved A/B runs compared commit `c857e86` with the revision-retaining
scanner. On the 100,001-file APFS fixtures, eleven paired warm runs found no
throughput regression: median native wall time changed from 140.24 to 134.14 ms
on the directory-rich fixture and from 172.74 to 171.82 ms on the wide fixture.
Five paired process-RSS observations on the directory-rich fixture increased
from a 34,668,544-byte median to 38,010,880 bytes, a 3,342,336-byte increase
consistent with the intentional 32-byte revision per file plus allocator noise.

The native Linux release harness passed 44 tests. On a fresh ext4 fixture with
100,001 files and 1,010 directories, nine warm `statx` runs had a 23.86 ms median
traversal and 25.12 ms median wall time; one `/usr/bin/time -v` observation used
18,712 KiB peak RSS. Rust 1.97 also advanced the full Tauri compile through the
dependency MSRV gate; it now stops at the host's missing `dbus-1.pc`, outside the
scanner module graph.

The native Windows release harness passed 35 tests. Eleven interleaved runs on
an 8,001-file NTFS fixture compared otherwise identical binaries before and
after the extra `FileBasicInfo` query. Median MFT traversal changed from 122.22
to 118.62 ms, so this sample likewise found no measurable regression. These are
warm synthetic results, not evidence about cold-cache or antivirus-heavy
systems. Summary samples are preserved in
[`performance-results/2026-07-12-scan-revision-impact.csv`](performance-results/2026-07-12-scan-revision-impact.csv).

### 2026-07-12 Windows NTFS MFT validation

The Windows backend was compiled and run natively over SSH on an AMD64 Windows
machine (`DESKTOP-1NSAB4F`) with Rust 1.97.0 GNU and an NTFS virtual disk. The
release fixture contained 8,192 empty files plus a nested three-file tree, one
hard link, and a junction targeting another volume. The volume also contained
the normal recycle-bin and system-volume-information directories. Both backends
listed the junction without following it. MFT hard-link name enumeration
restored both paths, assigned bytes to the lexicographically earlier path, and
charged the physical file once; the portable Windows backend reported both
paths but cannot deduplicate their bytes.

Across nine warm runs, median traversal time was 156,644 us for MFT and 304,161
us for `jwalk`; median complete harness time was 158,050 us and 304,570 us,
respectively. On this fixture, MFT traversal was 48.5% faster. Nine cancellation
runs requested cancellation after the 2,048-entry progress boundary and returned
in 638–1,124 us, with a 721 us median.

The backend deliberately rejects subfolder roots before enumeration. A measured
prototype that enumerated the system volume for a three-file subfolder took
27.9 seconds versus 1.5 ms for `jwalk`, demonstrating that whole-volume MFT cost
is unsuitable for arbitrary folder scans. Automatic selection therefore keeps
`jwalk` for Windows subfolders, non-NTFS volumes, and unavailable volume access.

Raw warm-run and cancellation values are preserved in
[`performance-results/2026-07-12-g14-windows-mft.csv`](performance-results/2026-07-12-g14-windows-mft.csv).
This is one warm-cache virtual disk and does not establish cold-cache or broad
hardware performance. The MFT control and record contracts follow Microsoft's
[`FSCTL_ENUM_USN_DATA`](https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_enum_usn_data),
[`USN_RECORD_V2`](https://learn.microsoft.com/windows/win32/api/winioctl/ns-winioctl-usn_record_v2),
and
[`FindFirstFileNameW`](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-findfirstfilenamew)
documentation.

## Interpretation and next measurements

Most results are warm-cache, synthetic metadata measurements on one machine;
the one real-tree observation is still a single local APFS checkout. They do
not measure cold storage, network volumes, antivirus interference, Tauri
serialization, frontend rendering, or other operating systems. The RSS
measurements include the benchmark process and allocator, not just
snapshot-owned bytes. These results are a regression baseline, not a universal
speed claim.

Before generalizing these results beyond the measured workloads, add:

- additional representative real directory trees and cold-cache runs;
- snapshot-owned bytes per entry and scaling beyond 100,000 entries;
- cancellation latency during deliberately long aggregation work;
- IPC serialization and first-render timing;
- broader Linux filesystem/hardware coverage, cold-cache throughput, and
  scheduler profiling without restricted performance counters;
- broader portable-versus-native parity, cold-cache throughput, and peak-memory
  scaling on representative Windows system volumes.
