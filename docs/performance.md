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

For a direct optimization comparison, prefer the paired harness over two
separate benchmark commands:

```sh
just benchmark-compare /tmp/cepa-fixture jwalk getattrlistbulk 9 \
  > /tmp/cepa-comparison.json
# Linux:
just benchmark-compare /tmp/cepa-fixture jwalk statx 9
```

It warms both requested backends once, verifies warmup parity, then alternates
which backend runs first in each measured pair. Every measured result must still
match the frozen warmup accounting. Schema-versioned JSON includes both absolute
backend summaries and `(right - left) / left` percentage changes for every pair;
a negative value means the right backend was faster. The report also keeps
snapshot payload, initial response, aggregation, and release measurements so a
traversal improvement cannot silently hide a retained-state regression.

Alternating order reduces systematic first/second cache bias; it does not make
an active machine quiet, cool the CPU, stabilize a changing tree, or turn
warm-cache data into cold-storage evidence. Inspect the paired range and system
load as well as the median. The command rejects two requests that resolve to the
same implementation, including an `auto` backend that resolves to the explicitly
named backend on that host. If `auto` resolves differently after warmup, the
affected measured pair also fails instead of mixing implementations in one
summary.

Live system volumes may change during the warmup and must not bypass that
stability check. Record an explicitly single-run observation instead:

```sh
just observe-scan /path/to/live-volume auto > /tmp/cepa-observation.json
```

The observation harness performs no warmup and makes no repeatability claim. It
reports the complete result, initial-view and response measurements, environment,
and wall time for that one workload snapshot. Compare counts alongside timing
when using several observations of a changing volume.

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
- `aggregationUs`: bottom-up propagation of file and directory totals plus
  release of excess retained child-index capacity.
- `indexingUs`: post-aggregation index finalization. This is currently zero
  because the arena builds its indexes incrementally during traversal.
- `scannerElapsedMs`: the three scanner phases together.
- `initialViewMs`: bounded selection and materialization of the initial radial
  chart and top-500 list.
- `initialResponseBytes`: compact JSON size of the exact initial scan-response
  shape before Tauri transport.
- `initialResponseSerializationUs`: `serde_json` serialization of that response;
  this excludes native IPC framing, transfer, and frontend parsing.
- `initialListItems` and `initialChartItems`: the recursive wire-node counts that
  make response-size comparisons auditable.
- `snapshotRetainedBytes`: capacity-aware payload bytes owned by the retained
  snapshot, including the node arena, names, child-ID buffers, and root path but
  excluding allocator bookkeeping and the separately materialized initial view.
- `snapshotBytesPerEntry`: retained payload divided by the completed file and
  directory count. Empty roots report zero.
- `snapshotReleaseMs`: synchronous destructor time for the retained snapshot
  after the separately owned result and initial response view are released.
- `wallMs`: scanner plus initial view and small harness overhead.

The benchmark deliberately retains the snapshot until after timing, matching
the application, which needs it for drill-down.
That lifetime now ends explicitly when the user returns Home: a matching scan-ID
discard detaches the retained snapshot, cancels related search and estimate
work, and invalidates any compression plan before the frontend leaves the
result. A blocking-pool task keeps the detached owner until temporary clones
held by already-running work are gone, then performs the final destructor away
from the Tauri command thread. Regressions cover stale-ID isolation, immediate
state detachment, in-flight ownership, and the final drop thread. This is an
ownership-boundary check, not a claim about when the operating system will
reduce process RSS.

## 2026-07-13 snapshot release baseline

Environment: Intel Core i9-13900K, 32 logical CPUs, 125 GiB RAM, Linux 6.8,
Rust 1.97.0, NVMe-backed ext4, `x86_64`, release profile, `statx` backend. A
fresh wide fixture contained 1,000,003 entries and retained 157,606,577 bytes
of capacity-aware snapshot payload. Results are nine runs after one warmup.

Synchronous snapshot destruction took 12.10–15.13 ms, with a 13.20 ms median.
That is enough work to avoid placing the destructor directly on a UI-facing
command path. Cepa now removes the state owner in constant time and schedules
final release on the blocking pool; if a search, view, or estimate already owns
a clone, the release task waits without making the detached scan available to
new requests.

The measurement isolates Rust destructor work. It is not a WebView frame-time
measurement, an RSS-reclamation measurement, or evidence that release cost
scales linearly for every directory shape. Raw runs are preserved in
[`performance-results/2026-07-13-linux-snapshot-release.csv`](performance-results/2026-07-13-linux-snapshot-release.csv).

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

### 2026-07-13 hard-link path comparison reuse

Deterministic ownership compares the complete relative paths of duplicate hard
links. The original comparison allocated two ancestor vectors for every
duplicate name. The scanner now retains two node-ID scratch buffers for the
duration of one scan, clears them between comparisons, and grows them only when
a deeper path requires more capacity. They are traversal-only state and do not
enter the retained snapshot.

The Rust 1.97.0 ext4 workstation compared the committed baseline and candidate
as separate release binaries. Fifteen single-run pairs alternated execution
order; each invocation performed its own unmeasured warmup first. Values below
are medians.

| Workload | Backend | Baseline traversal | Reused buffers | Change | Improved pairs |
| --- | --- | ---: | ---: | ---: | ---: |
| 50,302-entry controlled tree, 50,000 duplicate links | `jwalk` | 10.287 ms | 9.402 ms | -8.6% | 13/15 |
| 50,302-entry controlled tree, 50,000 duplicate links | `statx` | 11.159 ms | 10.770 ms | -3.5% | 12/15 |
| 435,960-entry development workspace, 41,660 duplicate links | `jwalk` | 123.688 ms | 119.122 ms | -3.7% | 14/15 |
| 435,960-entry development workspace, 41,660 duplicate links | `statx` | 157.468 ms | 150.376 ms | -4.5% | 10/15 |

Baseline and candidate workload/accounting fields matched for every comparison.
The controlled tree also retained exact `jwalk`/`statx` parity for file and
directory counts, bytes, duplicate ownership, skipped work, and accounting
flags. The candidate's real-workspace `statx` sample contained one 210.60 ms
outlier, so the result supports eliminating repeated allocation rather than a
universal traversal-speed claim. Raw runs are preserved in
[`performance-results/2026-07-13-hardlink-path-scratch.csv`](performance-results/2026-07-13-hardlink-path-scratch.csv).

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

#### Automatic-backend tradeoff and lazy paths

A fresh 15-round interleaved comparison on the same native ext4 workstation
confirmed that `jwalk` still has the warm traversal advantage. Its median was
17.62 ms versus 26.48 ms for `statx` on the 101,011-entry directory-rich fixture,
and 16.78 ms versus 18.63 ms on the 100,102-entry wide fixture. Accounting was
identical, so wall time alone would favor the portable backend.

That is not the whole production tradeoff. Five warmed process observations
put median `jwalk` peak RSS at 48,964 KiB and 43,196 KiB on the two fixtures,
versus 18,348 KiB and 19,992 KiB for `statx`. Across 21 cancellation runs,
`jwalk` median/p95/max latency was 543/2,883/2,951 us on the directory-rich
fixture and 2,706/4,425/4,697 us on the wide fixture. Native medians were
190 and 203 us, with maxima of 235 and 241 us. Linux automatic selection
therefore remains `statx`: switching for warm throughput would more than double
peak RSS and materially loosen cancellation on these large synthetic trees.

Profiling the native ingest path then exposed avoidable work independent of the
scheduler. Every file name was cloned and joined to its parent path even though
only child directories and bounded progress reports need a full path. File names
now move directly into the retained arena; directory names are cloned once for
their traversal task, and ordinary file paths are built only when a progress
update is emitted.

Fifteen interleaved A/B rounds found median native traversal falling from 22.56
to 21.13 ms (-6.3%) on the directory-rich fixture and from 20.06 to 17.88 ms
(-10.9%) on the wide fixture. Median wall time improved by 5.5% and 9.7%.
Five post-change RSS observations remained 18.0–18.8 MiB for the directory-rich
fixture and 19.0–22.5 MiB for the wide fixture. Median cancellation remained
193 and 187 us; one directory-rich run reached 1,038 us, while the other 20
were at or below 237 us.

A separate 15-round prototype doubled result batches from 512 to 1,024 entries.
It regressed median traversal by 2.9% and 5.9%, so the production bound remains
512. Raw samples for the backend decision, rejected batch size, lazy-path A/B,
RSS, and cancellation are preserved in
[`performance-results/2026-07-12-linux-lazy-paths.csv`](performance-results/2026-07-12-linux-lazy-paths.csv).

#### Worker-local directory buffers and a representative mixed tree

The earlier real-tree sample contained only 3,133 entries. A broader read-only
run used the same ext4/i9-13900K workstation and Rust 1.97.0 against a quiescent
development workspace containing 353,405 files and 42,628 directories. The
396,033 retained entries included 41,865 duplicate hard links and occupied
approximately 72.8 GiB on disk. `jwalk` and `statx` agreed exactly on file and
directory counts, logical and allocated bytes, duplicate links, skipped entries,
skipped filesystems, and accounting flags.

One avoidable allocation remained in the native path: every directory task
allocated and released a fresh 64 KiB `getdents64` buffer. `RawDir` only borrows
the buffer while that task is being read, so each worker now allocates one buffer
before its task loop and reuses it for every directory. The existing eight-worker
cap bounds these buffers to 512 KiB regardless of tree size.

Fifteen interleaved A/B rounds on each canonical fixture and 31 on the mixed
workspace produced these medians:

| Workload | Metric | Per-directory allocation | Worker-local reuse | Change |
| --- | --- | ---: | ---: | ---: |
| Directory-rich | Wall time | 43.45 ms | 43.25 ms | -0.5% |
| Directory-rich | Traversal | 42.02 ms | 42.46 ms | +1.1% |
| Wide | Wall time | 19.35 ms | 19.44 ms | +0.4% |
| Wide | Traversal | 17.80 ms | 18.12 ms | +1.8% |
| Mixed workspace | Wall time | 164.87 ms | 154.78 ms | -6.1% |
| Mixed workspace | Traversal | 161.77 ms | 151.38 ms | -6.4% |

The canonical changes are inside the run-to-run variation and are not claimed as
speedups. The mixed workload has 42,628 directories and showed the expected
benefit from avoiding repeated buffer allocation. Five paired process runs put
median native peak RSS at 83,736 KiB before and 83,924 KiB after, a 188 KiB
increase (+0.2%). These are process-level observations; the worker buffers
themselves remain fixed by the existing concurrency cap.

Twenty-one cancellation runs per variant and workload remained bounded. The
selected implementation had a 118 us median and 218 us maximum on the
directory-rich fixture, and a 152 us median and 219 us maximum on the mixed
workspace. Post-change parity passed again for both canonical fixtures and the
mixed tree. Native formatting, check, strict Clippy, and all 56 dependency-light
tests also passed on the Rust 1.97.0 Linux host.

For backend context, separate nine-run sequential measurements put the current
native mixed-workspace median at 140.31 ms versus 114.07 ms for `jwalk`; native
is still 23.0% slower and no throughput advantage is claimed. Five warmed
process observations put median peak RSS at 83,924 KiB for native versus
195,748 KiB for `jwalk`, 57.1% lower. This larger real workload therefore
supports the existing automatic-backend tradeoff: bounded memory and tight
cancellation remain the reason for selecting `statx`, not warm traversal speed.

Raw scan, cancellation, memory, and parity observations are preserved in
[`performance-results/2026-07-13-linux-directory-buffer-reuse.csv`](performance-results/2026-07-13-linux-directory-buffer-reuse.csv),
[`performance-results/2026-07-13-linux-directory-buffer-cancellation.csv`](performance-results/2026-07-13-linux-directory-buffer-cancellation.csv),
[`performance-results/2026-07-13-linux-real-workspace.csv`](performance-results/2026-07-13-linux-real-workspace.csv),
[`performance-results/2026-07-13-linux-real-workspace-memory.csv`](performance-results/2026-07-13-linux-real-workspace-memory.csv),
and
[`performance-results/2026-07-13-linux-real-workspace-parity.csv`](performance-results/2026-07-13-linux-real-workspace-parity.csv).

#### Rejected entry-buffer and shallow task-batching prototypes

A fresh Rust 1.97.0 ext4 pass tested two remaining sources of scheduler-adjacent
overhead. Reducing the first result-vector reservation from 512 to 32 entries
regressed 15-run median wall time by 18.8% on the 110,101-entry
directory-rich fixture, 10.5% on the 100,102-entry wide fixture, and 6.9% on a
21,990-entry checkout. A bounded eight-vector recycle pool still regressed the
directory-rich median by 9.5%; its 1.6% wide improvement and 0.4% checkout
improvement were not enough to justify another synchronization path. Both
allocation prototypes were removed.

Pairing two directory tasks per queue message initially looked promising on the
synthetic fixtures, but alternating-order batches exposed a 20.2% checkout
regression. Restricting pairs to small, high-fanout siblings preserved a 13.3%
directory-rich improvement but regressed the wide and checkout medians by 2.4%
and 7.1%. An explicit single-or-pair message removed most non-batched overhead,
but then the directory-rich median was 6.0% slower and the other workloads were
flat. The task scheduler therefore remains unchanged.

These experiments reinforce that `sched_yield` counts alone do not identify a
safe optimization. Any future scheduler redesign needs representative mixed-tree
evidence in the first comparison, not only canonical synthetic fixtures. The
aggregate trials are preserved in
[`performance-results/2026-07-13-linux-rejected-scheduler-prototypes.csv`](performance-results/2026-07-13-linux-rejected-scheduler-prototypes.csv).

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
consistent with the intentional 32-byte revision field per scanner node plus
allocator noise.

The dependency-light production-core path passed check, Clippy with warnings
denied, and 46 tests under Rust 1.97.0. It compiles the same scanner,
compression, and benchmark modules as the app while disabling only the default
Tauri `desktop` feature. The benchmark recipes now use this path, so native
backend validation no longer depends on GTK/WebKit development packages. It is
not evidence that the Tauri shell builds or launches.

On a fresh 4,099-file adversarial ext4 fixture, `jwalk` and `statx` again matched
all accounting fields, including one deduplicated hard link and a directory
symlink that was listed but not followed. Nine asynchronous `statx` cancellation
runs returned in 56–102 us, with an 84 us median. These small warm runs validate
behavior and cancellation wiring, not representative throughput. Raw values are
preserved in
[`performance-results/2026-07-12-linux-native-core.csv`](performance-results/2026-07-12-linux-native-core.csv).
The same dependency-light check and warning-denied Clippy path passed 37 tests
on native Windows with Rust 1.97.0, including the MFT parser, NTFS path policy,
and exact LZNT1 estimator coverage.

On a separate fresh ext4 fixture with
100,001 files and 1,010 directories, nine warm `statx` runs had a 23.86 ms median
traversal and 25.12 ms median wall time; one `/usr/bin/time -v` observation used
18,712 KiB peak RSS. Rust 1.97 also advanced the full Tauri compile through the
dependency MSRV gate; it now stops at the host's missing `dbus-1.pc`, outside the
scanner module graph.

The current tree was revalidated on that native Linux host after its compiler
was updated to Rust 1.97.0 on 2026-07-13. Dependency-light formatting, check,
warning-denied Clippy, and 61 library/example tests passed; Svelte diagnostics
were clean and all 35 frontend tests passed under Bun 1.2.21. On a fresh
4,099-file ext4 fixture, `jwalk` and `statx` matched all accounting fields,
including one deduplicated hard link and a directory symlink that was not
followed. Three `statx` cancellations returned in 70–155 us, and the new
single-run observation harness completed the same fixture through `statx`. A
desktop-feature Cargo check still stops specifically at the absent system
`dbus-1.pc`; the compiler update therefore closes the Rust MSRV question but
does not supply Linux desktop development packages. Smoke values are preserved
in
[`performance-results/2026-07-13-linux-rust197-smoke.csv`](performance-results/2026-07-13-linux-rust197-smoke.csv).

### 2026-07-16 Rust 1.97 Linux desktop validation

An exact archive of commit `67e2e54` was validated on the same Ubuntu 22.04
workstation with Rust 1.97.0 and Bun 1.2.21. Its Git tree hash matched the local
source before validation. Because the host does not permit non-interactive
system package installation, GTK, WebKitGTK 4.1, AppIndicator, librsvg, and DBus
development packages from its configured Ubuntu-compatible repositories were
extracted into a disposable user-local sysroot; the existing checkout and
system installation were not modified.

Frontend diagnostics and all 45 frontend tests passed. Dependency-light
formatting, all-target check, warning-denied Clippy, and 71 core tests passed
with the real-Btrfs fixture ignored. With the desktop feature enabled, all-target
check and warning-denied Clippy passed, followed by 89 library tests, four Tauri
configuration tests, and the example-target tests; the real-Btrfs and
file-manager-opening tests were the two intentional ignores. This closes the
earlier `dbus-1.pc` compile blocker for the source-identical desktop graph.

The production frontend and optimized x86-64 Linux executable also built. The
19,583,240-byte ELF had no unresolved dynamic libraries under the disposable
runtime. It then remained alive for eight seconds under isolated Xvfb and DBus
sessions. The sysroot-only run used an unprivileged mount namespace to expose
WebKitGTK's package helper directory at its compiled system path. A portal FUSE
warning from the host session did not terminate the process. The new
`smoke-linux-desktop.sh` runner subsequently passed against the same executable
with a three-second window, and its early-exit and invalid-duration paths failed
as intended; `just` itself was validated by local recipe expansion because it is
not installed on that host.

This is native compile, link, test, production-build, and headless startup
evidence. A source-identical follow-up at commit `f182585` also built and
structurally validated the DEB, RPM, and AppImage. The DEB and RPM payloads
extracted to 19,583,240-byte x86-64 executables with no unresolved libraries
under the disposable runtime; both survived eight-second headless launches.
The 83,237,368-byte AppImage included the WebKitGTK helper executables and
survived the same launch window with no sysroot library path. The host account's
seven-digit UID exceeded the six-character owner field in the first DEB's `ar`
header, so that package was correctly rejected and rebuilt inside an
unprivileged user namespace before validation. The host's absent WebKitGTK 4.1
installation was supplied at its ordinary runtime path through an unprivileged
overlay while packaging; neither accommodation modified the host.

The follow-up closes structural Linux package validation and portable AppImage
startup for this source. It is not proof of physical desktop interaction, a
real folder picker, installed DEB/RPM lifecycle behavior, signatures, or broader
Linux hardware/filesystem coverage. CI now repeats the AppImage smoke after
package validation. The exact results are preserved in
[`performance-results/2026-07-16-linux-desktop-validation.csv`](performance-results/2026-07-16-linux-desktop-validation.csv).
Package results and digests are preserved separately in
[`performance-results/2026-07-16-linux-bundle-validation.csv`](performance-results/2026-07-16-linux-bundle-validation.csv).

The native Windows release harness passed 35 tests. Eleven interleaved runs on
an 8,001-file NTFS fixture compared otherwise identical binaries before and
after the extra `FileBasicInfo` query. Median MFT traversal changed from 122.22
to 118.62 ms, so this sample likewise found no measurable regression. These are
warm synthetic results, not evidence about cold-cache or antivirus-heavy
systems. Summary samples are preserved in
[`performance-results/2026-07-12-scan-revision-impact.csv`](performance-results/2026-07-12-scan-revision-impact.csv).

### 2026-07-13 compact Unix scan revisions

Unix backends enforce one filesystem boundary, so repeating the filesystem ID
inside every retained revision was unnecessary. The snapshot now stores that ID
once and retains a 24-byte file ID plus modification/change revision in each
eligible node. Compression-target resolution reconstructs the original exact
32-byte `ScannedFileRevision`; an unexpected second filesystem identity loses
its revision instead of inheriting the wrong one. Windows keeps the full
revision because its portable fallback does not expose the same cheap root
filesystem identity.

The benchmark schema advanced to version 6 and now measures retained snapshot
payload directly. On a fresh 101,011-entry directory-rich APFS fixture, the
capacity-aware retained payload fell from 20,376,415 to 19,327,855 bytes: a
1,048,560-byte or 5.146% reduction, equal to 10.381 bytes per completed entry
after arena capacity is included. Eleven alternating-order warm pairs measured
29.98 ms baseline and 29.24 ms compact median wall time, so this run found no
throughput regression.

Peak process RSS did not mirror the retained-payload improvement: its median
rose from 35,487,744 to 37,896,192 bytes (6.8%), while individual baseline runs
ranged from 35.0 to 44.9 MB. RSS includes transient traversal allocations,
threads, allocator behavior, and the harness, so this result does not support a
peak-memory reduction claim. The optimization is retained for its directly
measured post-scan payload reduction. Accounting fields were identical across
the baseline and compact binaries. Raw paired values are preserved in
[`performance-results/2026-07-13-macos-compact-revisions.csv`](performance-results/2026-07-13-macos-compact-revisions.csv).

The current tree passed the full macOS frontend and Rust suite, a release Tauri
build, and real `jwalk`/`getattrlistbulk` parity on the 101,011-entry fixture.
Rust 1.97.0 Linux formatting, check, warning-denied Clippy, all 58 core tests,
and every example test passed through the dependency-light path. That native
run also verifies that `statx` major/minor device identity reconstructs the same
revision as a later no-follow standard metadata snapshot. Windows passed the
same exact-revision regression and a default-feature release build; its retained
revision layout is unchanged.

### 2026-07-13 compact retained parent IDs

Every retained node previously stored its optional parent as `Option<usize>`.
Because every bit pattern is a valid `usize`, that optional value occupies two
machine words. The arena now stores the parent index plus one in a
`NonZeroUsize`, preserving zero as the root's `None` niche. The wire node ID,
child indexes, path reconstruction, and accounting contracts are unchanged. A
layout regression requires the optional encoded parent to remain exactly one
machine word and checks round trips through the largest usable index.

The controlled release comparison ran on the same 64-bit Rust 1.97.0 Linux
environment described above, using NVMe-backed ext4 and the `statx` backend.
Both fresh metadata-heavy fixtures contained empty files plus the fixture
manifest. Each binary performed one warmup followed by five measured runs.

| Fixture | Entries | Baseline payload | Compact payload | Reduction |
| --- | ---: | ---: | ---: | ---: |
| 100 directories x 1,000 files | 100,102 | 19,099,252 B | 18,050,676 B | 1,048,576 B (5.49%) |
| 500 directories x 2,000 files | 1,000,506 | 157,423,128 B | 149,034,520 B | 8,388,608 B (5.33%) |

The reduction is exactly eight bytes for every reserved node-arena slot at
both scales. Capacity-aware bytes per completed entry fell from 190.80 to
180.32 in the smaller fixture and from 157.34 to 148.96 in the million-entry
fixture. All benchmark workload and accounting fields were identical between
the two binaries. The five-run wall-time samples overlapped and were not
interleaved, so they support neither a speedup nor a regression claim. Process
RSS was not measured; this is retained-payload evidence only. Raw runs are in
[`performance-results/2026-07-13-linux-compact-parent-ids.csv`](performance-results/2026-07-13-linux-compact-parent-ids.csv).

The exact candidate passed the full macOS `just check` suite and a release
Tauri build. On Linux, Rust 1.97.0 passed formatting, 67 dependency-light
library tests plus four doc tests, and warning-denied all-target Clippy. On
Windows, Rust 1.97.0 passed 56 dependency-light native tests plus four doc
tests, warning-denied all-target Clippy, and a default-feature release build.

### 2026-07-12 Windows NTFS MFT validation

The Windows backend was compiled and run natively over SSH on an AMD64 Windows
machine with Rust 1.97.0 GNU and an NTFS virtual disk. The
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

### 2026-07-13 Windows MFT measurement streaming

The original Windows traversal retained a `WindowsMeasurement` slot for every
primary file until all file-ID workers finished, then copied those measurements
into the node arena. On a large volume this duplicated the scanner's dominant
per-file state. The revised traversal builds the deterministic node arena first
and applies measurements as workers return bounded 256-item batches. At the
eight-worker cap, the synchronized channel holds at most 16 batches, or 4,096
measurements. Hard-link candidates are sorted by node ID before name recovery,
preserving deterministic lexicographic ownership independently of worker order.

One release observation per implementation scanned a changing, approximately
1 TB NTFS system volume on a Ryzen 9 5900HS system with 16 logical CPUs, about
40 GB RAM, 4 KiB clusters, a 5.28 GB MFT, Windows build 26200, and Rust 1.97.0.
An external PowerShell sampler polled working set every 100 ms. The baseline
retained 5,563,351 entries and peaked at 2,816,573,440 bytes; bounded batching
retained 5,567,232 entries and peaked at 2,139,058,176 bytes. That is a 24.1%
peak-working-set reduction despite the later observation containing 0.07% more
entries. The live workload changed between runs, so its roughly 326-second scan
times are not a controlled speed comparison.

A separate quiescent NTFS virtual disk supplied the controlled comparison. Its
8,197 files, five directories, 16,788,236 logical bytes, 12,560 allocated bytes,
one deduplicated hard link, and zero skipped entries matched exactly in every
warm run before and after the change. Across nine runs, median wall time fell
from 194.48 ms to 145.58 ms (25.1%), and median traversal fell from 192,815 us
to 144,161 us (25.2%). Nine asynchronous cancellations requested after the
2,048-entry boundary returned in 1,062–2,082 us, with a 1,249 us median. Native
formatting, checks, warning-denied Clippy, and all 54 library/example tests also
passed on Windows. These warm results cover one machine and do not establish
cold-cache or broad hardware performance.

Raw evidence is preserved in
[`performance-results/2026-07-13-windows-real-volume.csv`](performance-results/2026-07-13-windows-real-volume.csv),
[`performance-results/2026-07-13-windows-mft-streaming.csv`](performance-results/2026-07-13-windows-mft-streaming.csv),
and
[`performance-results/2026-07-13-windows-mft-cancellation.csv`](performance-results/2026-07-13-windows-mft-cancellation.csv).

### 2026-07-14 Windows MFT node-arena preallocation

After MFT enumeration and subtree ordering, the native backend knows exactly
how many primary records it will place in the retained node arena. Letting that
`Vec` grow geometrically left a large power-of-two tail resident for the life of
the completed scan. The selected implementation uses a fallible exact reserve
before arena construction, so primary insertion performs no capacity growth and
needs no post-scan shrink or full-arena copy.

Hard-link names are recovered only after streamed file measurement, so primary
preallocation alone was unsafe. On the 100,115-entry fixture, adding just one
hard-link alias made that prototype grow from its exact primary capacity to a
29,534,465-byte retained payload. The final implementation sums each measured
file's possible extra names, checks cancellation, and makes one fallible exact
reserve before alias ingestion. The same one-alias fixture retained 15,918,825
bytes with exact accounting, versus 20,128,705 bytes for the committed
geometric-growth baseline. This upper bound may reserve names that are later
filtered or unavailable, but it prevents per-alias growth and remains below the
baseline on the measured fixture.

The controlled comparison used the existing 20 GB NTFS virtual disk on the
Ryzen 9 5900HS Windows host, Rust 1.97.0, and exact baseline/candidate release
binaries. Every invocation performed one warmup and one measured MFT scan; pair
order alternated.

| Workload | Pairs | Retained bytes, before | Retained bytes, after | Reduction | Median paired wall delta | Paired wall range |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 10,115 entries | 15 | 2,461,117 | 1,608,669 | 852,448 (34.6%) | -0.712 ms | -6.662 to +6.606 ms |
| 100,115 entries | 9 | 20,128,685 | 15,918,669 | 4,210,016 (20.9%) | -56.473 ms | -168.917 to +505.671 ms |

Nine of fifteen small-fixture pairs and six of nine large-fixture pairs favored
the candidate. One large candidate run was delayed by 506 ms, making traversal
noise much larger than any arena-construction effect; no throughput improvement
is claimed. Five additional processes sampled working set every 10 ms. Median
peak working set was 35,536,896 bytes before and 35,414,016 bytes after, an
effectively flat 122,880-byte difference. The directly measured retained
payload reduction therefore does not establish a whole-process peak-memory
reduction.

On the final 100,116-entry hard-link fixture, nine asynchronous MFT
cancellations returned in 6,557–10,485 us with a 6,865 us median. Requests were
observed at the next bounded progress batch, after 8,558, 10,606, or 12,654
entries. Exact-source
Windows formatting, 62 dependency-light library tests, four configuration
tests, warning-denied all-target Clippy, and a default-feature release build
passed. Raw observations are preserved in
[`performance-results/2026-07-14-windows-mft-arena-pairs.csv`](performance-results/2026-07-14-windows-mft-arena-pairs.csv),
[`performance-results/2026-07-14-windows-mft-arena-memory.csv`](performance-results/2026-07-14-windows-mft-arena-memory.csv),
and
[`performance-results/2026-07-14-windows-mft-arena-validation.csv`](performance-results/2026-07-14-windows-mft-arena-validation.csv).

## Current-folder search

Search operates on a completed in-memory snapshot and scans every direct child
before retaining the metric-ranked top 500 in a bounded heap. Regex compilation,
Unicode-aware case-insensitive matching, ranking, and conversion of retained
items are included in the reported time. The Tauri command runs this work on a
blocking worker, and the frontend waits 180 ms after the latest keystroke before
issuing a request. A replacement query, cleared field, navigation, or new scan
actively cancels the backend search; the retained-snapshot loop checks for
cancellation at bounded intervals. Sequence checks still prevent a stale result
from replacing newer UI state if cancellation races with completion.

On the same Apple M4 Pro development machine, a generated APFS directory with
100,000 direct empty files was scanned once and then searched nine times after
one warmup. A query matching every file had a 16.48 ms median and returned the
bounded top 500. A selective query matching one file had a 1.82 ms median. These
numbers exclude IPC and rendering, but the browser workflow separately verified
loading, below-cutoff results, no-match and error states, stale-response
suppression, metric changes, navigation reset, keyboard dismissal, and the
620-pixel responsive layout. Raw runs are preserved in
[`performance-results/2026-07-12-directory-search.csv`](performance-results/2026-07-12-directory-search.csv).

Use the checked-in benchmark against other directory shapes with:

```sh
just benchmark-search /path/to/folder query 9 auto allocated
```

## Bounded chart wire and first render

The list was already capped at 500 direct children, but the original chart bound
was only per directory. A three-level hierarchy could therefore multiply the
16-child limit into thousands of serialized nodes even when most deep arcs were
too small to be useful. Chart materialization now shares a deterministic global
512-node budget across the existing top-16-per-directory, three-level traversal.
Each visited level reserves its ranked siblings before descending, and an
aggregate item retains the logical and allocated bytes of omitted siblings. A
synthetic unit fixture verifies the global count and recursively checks that any
materialized child ring still covers its parent's complete byte total.

The Rust baseline used the Apple M4 Pro development machine, release `jwalk`,
one warmup, and nine measured scans of an APFS fixture containing 5,219
non-root directories in a 17-way, three-level hierarchy. The response benchmark
serializes the same `scanId`, `result`, and `view` shape returned by the Tauri
command, but it does not measure IPC framing or transport.

| Metric | Per-directory bound | Global 512-node bound | Change |
| --- | ---: | ---: | ---: |
| Recursive chart nodes | 4,641 | 512 | -89.0% |
| Initial response bytes | 443,439 | 51,449 | -88.4% |
| Median response serialization | 359 us | 52 us | -85.5% |
| Median initial-view construction | 0.371 ms | 0.044 ms | -88.0% |

Frontend rendering was measured separately in the development browser at the
configured 880 by 620 launch viewport. The deterministic `?mock=stress` workflow
contains all 500 list rows and 512 drawable SVG paths. Its marker begins when
the mocked scan response reaches Svelte and ends after the next painted frame;
it excludes scan and IPC time. Nine fresh runs changed from 141.0-166.8 ms with
a 149.5 ms median to 94.4-106.4 ms with a 98.6 ms median after applying
`content-visibility: auto` and a stable 61-pixel intrinsic block size to each
row, a 34.0% median reduction. All 500 rows remain in the DOM, the 31,000-pixel
scroll extent is unchanged, and an explicit row-500 interaction verified focus,
scrolling, inspector expansion, and final selected-row visibility. This follows
the [CSS Containment contract](https://www.w3.org/TR/css-contain-2/#using-cv-auto)
that `auto` content remains available to focus, find, and other user-agent
features while offscreen rendering may be skipped. WebKit added
[`content-visibility` in Safari 18](https://webkit.org/blog/15443/news-from-wwdc24-webkit-in-safari-18-beta/#content-visibility);
older engines ignore the optimization and retain the existing layout.

Raw observations are preserved in
[`performance-results/2026-07-13-chart-wire-budget.csv`](performance-results/2026-07-13-chart-wire-budget.csv)
and
[`performance-results/2026-07-13-frontend-render-containment.csv`](performance-results/2026-07-13-frontend-render-containment.csv).
The browser result is a frontend regression baseline, not native WebKit,
WebView2, or WebKitGTK proof.

### 2026-07-13 adaptive very-wide-directory ranking

Opening a directory previously cloned its complete child-ID vector before a
single partial selection retained the top 500 list rows. The work was linear,
but the temporary allocation also grew linearly: 500,000 direct children need
about 3.8 MiB of IDs, and a 5.57-million-child directory would need about
42.5 MiB before names or response materialization.

The selected implementation keeps that single-partition path while the clone is
at most 2 MiB. Beyond the cutoff, it repeatedly reduces a 16 × limit buffer to
the requested limit. The top-500 path therefore uses an 8,000-ID buffer, about
64 KiB on a 64-bit target, while retaining linear selection work and the exact
metric, secondary-size, name, and node-ID ordering. A synthetic regression
compares more than two complete windows with a full sort under both size metrics.

The main comparison used a quiescent, directly wide ext4 folder with 500,000
empty files on the native i9-13900K/Rust 1.97.0 Linux host. Each of 15
alternating-order rounds ran the release `statx` benchmark's warmup and one
measured scan. Values below are medians:

| Variant | Initial view | Full wall time | Response bytes | Retained snapshot |
| --- | ---: | ---: | ---: | ---: |
| Full child-ID clone | 16.78 ms | 496.08 ms | 63,402 | 78,803,311 bytes |
| Adaptive 16 × limit selection | 10.71 ms | 485.13 ms | 63,402 | 78,803,311 bytes |

Initial-view latency fell 36.2%. The 2.2% wall-time change is not presented as a
scan-throughput improvement because traversal dominates it and varied between
runs. Five paired process observations were likewise flat at 98,016 versus
98,244 KiB median peak RSS. Process RSS includes the retained arena and traversal
workers, so the memory result does not demonstrate a whole-process reduction;
the defensible bound is the ranking vector's directly constrained capacity.

Window tuning was interleaved separately on a 250,000-file ext4 folder. A
two-window buffer regressed median initial-view time to 8.88 ms, versus 6.42 ms
for the full clone, while the selected 16 × limit buffer reached 4.93 ms. On a
200,000-file APFS folder, however, the always-bounded 16 × limit prototype was
3.78 ms versus 3.36 ms for the full clone. This cross-platform result motivated
the adaptive 2 MiB cutoff instead of forcing either strategy on every folder.
A nine-round recheck of the final adaptive binary on that APFS fixture measured
3.41 ms versus 3.51 ms for the baseline, confirming that the below-cutoff branch
retains the original selection behavior. Full scan wall time was too variable on
the 97%-full development volume to compare.

Raw trials are preserved in
[`performance-results/2026-07-13-directory-ranking-linux-buffer-scaling.csv`](performance-results/2026-07-13-directory-ranking-linux-buffer-scaling.csv),
[`performance-results/2026-07-13-directory-ranking-linux-adaptive.csv`](performance-results/2026-07-13-directory-ranking-linux-adaptive.csv),
[`performance-results/2026-07-13-directory-ranking-linux-memory.csv`](performance-results/2026-07-13-directory-ranking-linux-memory.csv),
[`performance-results/2026-07-13-directory-ranking-macos-tuning.csv`](performance-results/2026-07-13-directory-ranking-macos-tuning.csv),
[`performance-results/2026-07-13-directory-ranking-macos-adaptive.csv`](performance-results/2026-07-13-directory-ranking-macos-adaptive.csv),
and
[`performance-results/2026-07-13-directory-ranking-macos-memory.csv`](performance-results/2026-07-13-directory-ranking-macos-memory.csv).

### 2026-07-13 aggregation cancellation and abandoned-arena release

Traversal progress cannot deterministically request cancellation after discovery
has ended, so the ordinary cancellation harness did not cover bottom-up
aggregation. The new `aggregation_cancellation` harness constructs a synthetic
million-node arena, runs the same production aggregation loop, and asks a
separate thread to cancel immediately after the first polling boundary beyond
500,001 processed nodes. The loop polls every 2,048 nodes; every measured run
therefore processed exactly 2,048 additional nodes before observing the request.
The benchmark waits for background reclamation before starting its next run, but
reports that work separately from foreground response latency.

The shared completion path now emits one bounded `finishing` progress update
before aggregation begins. That phase keeps cancellation visibly available
during long retained-arena passes. It replaces the former post-aggregation final
progress update, so successful completion does not send two adjacent terminal
payloads before the bounded result response. This is a transport and interaction
contract, not a claim that aggregation itself became faster. Long aggregation
passes refresh the same bounded payload from the existing 2,048-node checkpoints
at no more than the shared 100 ms progress cadence. The event rate is therefore
time-bounded rather than proportional to retained arena size.

The first macOS pass exposed that cancellation detection was already bounded,
but returning still synchronously destroyed the abandoned arena. Moving only
that cancelled arena's destruction to a named background thread changed the
nine-run median foreground cancellation latency from 23,394 us to 3,050 us, an
87.0% reduction. The post-change macOS range was 63–14,217 us; background
reclamation had a 6,001 us median. The wide spread makes this responsiveness
evidence, not a stable aggregation-throughput comparison.

An exact-source Rust 1.97.0 Linux run used the same release workload. Its nine
foreground cancellations were 46–192 us with a 60 us median, and background
release was 11,595 us median. Formatting, the focused synchronous and
separate-thread regressions, and warning-denied Clippy across all dependency-
light targets also passed there.

This synthetic flat arena isolates aggregation arithmetic, polling, thread
handoff, and destruction. It does not measure traversal, filesystem behavior,
the Tauri bridge, or process-RSS reclamation. Foreground cancellation can now
finish before memory is reclaimed, so callers must not interpret command return
as an RSS boundary. Raw runs, including the macOS before/after comparison, are
preserved in
[`performance-results/2026-07-13-aggregation-cancellation.csv`](performance-results/2026-07-13-aggregation-cancellation.csv).

### 2026-07-14 retained child-index compaction

Directory child-ID vectors grow geometrically during traversal, so their
capacity can exceed their final length for the entire retained-snapshot
lifetime. The selected implementation calls `shrink_to_fit` while the existing
reverse aggregation pass already has each directory hot, avoiding a second
linear walk over a potentially multi-million-node arena. The root is compacted
after that loop. Existing cancellation checkpoints run before compaction at
each polling boundary, and one final check precedes root compaction.

The comparison used exact baseline and candidate binaries with alternating
order. Each process performed its own warmup followed by one measured scan. The
APFS run used an Apple M4 Pro and `getattrlistbulk`; the ext4 run used the native
i9-13900K Rust 1.97.0 host and `statx`. Both fixtures contained 100,001 files.
The directory-rich shape had 10,100 directories and 110,101 total entries; the
wide shape had 101 directories and 100,102 total entries.

| Platform and shape | Pairs | Retained bytes, before | Retained bytes, after | Reduction | Median aggregation delta | Median wall delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| macOS APFS, directory-rich | 11 | 18,773,279 | 18,270,663 | 502,616 (2.68%) | +0.154 ms | +1.345 ms |
| macOS APFS, wide | 11 | 18,050,701 | 18,031,261 | 19,440 (0.11%) | +0.016 ms | +0.897 ms |
| Linux ext4, directory-rich | 9 | 18,773,251 | 18,270,635 | 502,616 (2.68%) | +0.174 ms | -3.313 ms |
| Linux ext4, wide | 9 | 18,050,673 | 18,031,233 | 19,440 (0.11%) | +0.125 ms | +1.543 ms |

Paired wall deltas ranged from -12.03 to +20.48 ms on the directory-rich APFS
fixture, -15.69 to +54.88 ms on wide APFS, -8.62 to +18.73 ms on directory-rich
ext4, and -3.95 to +4.84 ms on wide ext4. The traversal noise is much larger
than the measured aggregation cost, so these data support the retained-payload
reduction but no scan-throughput claim. Snapshot release deltas were likewise
small and mixed. `snapshotRetainedBytes` excludes allocator bookkeeping, and no
process RSS or peak-memory measurement was made.

Formatting, the focused compaction and cancellation tests, and warning-denied
dependency-light Clippy passed from the exact candidate source on Rust 1.97.0
Linux and Windows. The exact Windows source also completed a default-feature
release build. Raw paired observations are preserved in
[`performance-results/2026-07-14-child-index-compaction.csv`](performance-results/2026-07-14-child-index-compaction.csv).

### 2026-07-16 plan content-integrity throughput and cancellation

An exact candidate based on commit `7ab6635` added
`content_integrity_benchmark`, which runs the production BLAKE3
positioned-read loop used by dormant compression plans. It reports complete-pass
time separately from externally requested cancellation latency and bytes read
after the request. The `just benchmark-content-integrity` recipe accepts a
regular file, iteration count, and optional cancellation byte threshold.

Release measurements used a 512 MiB zero-filled fixture, one complete warmup,
and seven measured complete/cancellation pairs. Cancellation was requested after
256 MiB. The working buffer and cancellation polling interval were both 1 MiB.
The macOS fixture was created with `mkfile`; the Linux fixture with `truncate`,
so the Linux file was sparse. Both were warm page-cache and CPU/hash-path
measurements, not cold-media throughput or evidence for arbitrary storage.

On an Apple M4 Pro MacBook Pro with 48 GB RAM, macOS 15.6, and Rust
1.94.0-nightly, the complete pass had a 238,887 us median and 2,143 MiB/s median.
Cancellation returned in 467–581 us with a 496 us median. On the 32-thread
Core i9-13900K Linux host, kernel 6.8.0-124, and Rust 1.97.0, the complete pass
had a 164,032 us median and 3,121 MiB/s median. Cancellation returned in
337–353 us with a 344 us median. Every cancellation run read exactly one
additional 1 MiB chunk after the request; none exceeded the production bound.

These results support the fixed buffer and polling interval for explicit
single-file planning on the two measured systems. They do not bound a blocked
or slow filesystem syscall, cold disk latency, network storage, multi-gigabyte
interactive progress requirements, peak RSS beyond the known 1 MiB userspace
buffer, or native Windows behavior. Raw trials are preserved in
[`performance-results/2026-07-16-content-integrity.csv`](performance-results/2026-07-16-content-integrity.csv).

Rust 1.97.0 subsequently exposed a scheduling race in the benchmark harness: a
small, cached fixture could finish before the controller thread published its
cancellation request. The harness now holds the worker at the requested
observed chunk until the controller publishes cancellation. This
makes the requested byte boundary deterministic and isolates the production
loop's acknowledgement and return latency from controller scheduling. The
measurements above predate that rendezvous and must not be compared directly
with current cancellation latency or bytes-after-request results. The current
harness still does not measure UI dispatch, a request arriving during a blocked
filesystem read, or worst-case scheduler delay.

The exact rendezvous candidate based on `b0350c6` passed the complete
dependency-light native check on the same 32-thread Linux host with Rust 1.97.0.
Repeating the 512 MiB sparse, warm-cache workload for seven pairs produced a
164,302 us complete-pass median (3,116 MiB/s). Cancellation acknowledgement
ranged from 27 to 210 us with a 38 us median, and every run stopped at the exact
256 MiB request boundary. These cancellation results describe only the
rendezvoused production-loop return path under that cache state. Raw trials are
preserved in
[`performance-results/2026-07-16-content-integrity-rendezvous.csv`](performance-results/2026-07-16-content-integrity-rendezvous.csv).

### 2026-07-16 frontend compact-count formatting

An exact candidate based on `612c3da` changed the shared `formatCount` helper
to retain one locale-aware compact `Intl.NumberFormat` instead of constructing a
formatter for every label. The new `just benchmark-count-format` harness warms
both implementations, alternates which runs first over nine pairs, performs
10,000 formats per run, and rejects any checksum difference.

On the Apple M4 Pro host with Bun 1.3.14, per-call construction had a 158,895 us
median and the shared formatter a 2,495 us median, a 98.4% reduction in this
isolated workload. On the 32-thread Linux host with Bun 1.2.21, the respective
medians were 146,083 and 1,903 us, a 98.7% reduction. Every run produced the
same checksum. Raw trials are preserved in
[`performance-results/2026-07-16-frontend-count-format.csv`](performance-results/2026-07-16-frontend-count-format.csv).

This evidence supports reusing the formatter used by scan progress, completed
summaries, and directory descriptions. It does not establish an equivalent
whole-render improvement: the browser's bounded 500-row/511-segment stress view
remained dominated by DOM and SVG work, with same-session seven-run medians of
119.7 ms before and 116.6 ms after. Those browser observations were sequential,
not alternating pairs, so they are treated only as a no-regression check rather
than a UI-speed claim. Neither measurement covers native webview bridge cost,
filesystem scanning, or cold application startup.

### 2026-07-16 production macOS WebView and IPC smoke

An exact candidate based on `6a1b1a8` added `native_scan_smoke`. The Tauri
example reuses the production builder, plugins, bundled frontend, custom
protocol, startup ordering, and command handler. It isolates window state under
a smoke-only filename, sets the actual window to the supported 620×480 minimum,
then injects a same-document controller after the packaged page reports ready.
That controller submits the ordinary manual-path form, waits for the real scan
response and two painted frames, switches the size metric, navigates into a
directory, and performs the ordinary debounced folder search. Rust validates
the returned report and removes the isolated state file.

Seven separate release-process launches scanned the same warm APFS fixture with
2,503 files and 103 child directories. Median initial-page readiness was
228.90 ms (199.13–251.76 ms), and median process start through the complete
scan/metric/navigation/search report was 734.94 ms (693.58–794.69 ms).
Scan submission through painted result had a 35 ms median and 33–122 ms range.
Metric switching had a 32 ms median, directory navigation 69 ms, and search
219 ms; search includes the intentional 180 ms debounce. Every launch disclosed
the `macOS native` backend, rendered one radial-map Tab stop and two list Tab
stops, changed from one allocated chart segment to 21 logical segments without
a page error, found the dynamically selected row, and had no horizontal
overflow.

An exact 32-directory by 32-file sparse fixture also exposed and now guards a
metric-switch failure: repeated aggregate wedges previously shared a Svelte
key, aborting the reactive update. Sunburst segments now carry stable unique
keys, a focused frontend regression covers repeated aggregate labels, and the
production smoke independently bounds both the initial and logical charts.

These runs prove the programmatically driven production macOS WebView, bundled
asset protocol, Tauri IPC commands, real native scanner, painted result, and
minimum-size invariants on this one warm fixture. They are not a throughput
benchmark and do not exercise physical input, the native folder picker,
drag-and-drop, installed-package launch, cold storage, or another platform's
WebView. Raw runs are preserved in
[`performance-results/2026-07-16-macos-native-webview.csv`](performance-results/2026-07-16-macos-native-webview.csv).

### 2026-07-16 production Linux WebView and IPC smoke

The same exact `a0d6edd` harness was built on the Rust 1.97.0 Ubuntu 22.04
workstation against a disposable user-local WebKitGTK 4.1 development/runtime
sysroot. WebKit's helper directory was exposed only inside an unprivileged mount
namespace, and the application ran under isolated Xvfb and DBus sessions. The
standing checkout and system installation were unchanged.

Seven separate release-process launches scanned one 1,056-entry ext4 fixture:
32 directories containing 32 sparse 4 KiB files each. Median initial-page
readiness was 182.79 ms (166.06–217.83 ms), and median process start through the
complete report was 882.26 ms (864.78–967.64 ms). Scan submission through the
painted result had a 190 ms median, metric switching 47 ms, directory navigation
59 ms, and debounced search 208 ms. Every run selected `Linux native`, retained
one chart and two list Tab stops, rendered 272 allocated and 16 logical chart
segments within the 512-node bound, found the selected row, emitted no page
error, and had no horizontal overflow at 620×480.

This closes production WebKitGTK/IPC interaction for this fixture and host; it
is not physical-input, native-picker, installed-package, cold-cache, or general
Linux performance evidence. CI now runs the same harness after the Linux native
build using its provisioned WebKitGTK packages and isolated display/session
buses. Raw runs are preserved in
[`performance-results/2026-07-16-linux-native-webview.csv`](performance-results/2026-07-16-linux-native-webview.csv).

### 2026-07-16 native keyboard interaction smoke

A follow-up candidate based on `4f1f3b3` strengthened the same production
WebView harness beyond counting Tab stops. At 620×480 it now moves chart focus
with ArrowRight and Home, moves primary list focus with ArrowDown, preserves the
Reveal action kind while moving that focus with ArrowDown, and opens the first
chart folder with Enter before performing search.

One release run passed in the macOS WebKit view with the `macOS native` backend,
and one passed in Linux WebKitGTK with `Linux native`. Both retained one chart
and two list Tab stops, completed every focus and activation assertion, emitted
no page error, and had no horizontal overflow. The Linux run used the same
disposable WebKitGTK sysroot and unprivileged mount-namespace method as the
seven-run observation above, without disabling WebKit's sandbox. These are
programmatically dispatched production-WebView keyboard events, not physical
keyboard, screen-reader, or broader assistive-technology certification. The
exact validation record is preserved in
[`validation-results/2026-07-16-native-keyboard-webviews.txt`](validation-results/2026-07-16-native-keyboard-webviews.txt).

### 2026-07-16 same-process scan lifecycle smoke

A follow-up candidate based on `960d56b` extends the production harness through
the lifecycle after search. It activates Home, waits for the landing heading to
own focus, directly verifies that the first completed scan ID can no longer open
a directory, and submits the same fixture again. The second scan must complete
in the same process, restore focus to its result heading, render nonempty bounded
rows and chart nodes, and select the same native backend.

The flow passed once in macOS WebKit with `macOS native` and once in Linux
WebKitGTK with `Linux native`. The macOS process completed both scans and all
preceding interactions in 1,006 ms; the Linux process completed them in
1,291 ms. These single warm observations are runtime correctness evidence, not
comparative performance results. The Linux run again used the disposable
WebKitGTK sysroot and unprivileged helper mount without disabling WebKit's
sandbox. The exact assertions and scope are preserved in
[`validation-results/2026-07-16-native-scan-lifecycle.txt`](validation-results/2026-07-16-native-scan-lifecycle.txt).

### 2026-07-16 production scan-cancellation transport

Extending the production WebView harness to activate Stop exposed a transport
defect that the scanner-only cancellation benchmarks could not observe.
`scan_directory` kept its request-response IPC open until traversal and
aggregation finished. In macOS WebKit, a later `cancel_scan` invocation was
serialized behind that open command: the UI changed to `Stopping…`, but Rust did
not receive the cancellation command before the scan completed.

The command now sends the new scan ID immediately and returns that same ID as
its acknowledgement. Started, bounded progress, completed response, and failed
terminal state are ordered on the existing Tauri channel. The completed snapshot
is installed before its terminal event is sent; if that send fails, the matching
snapshot is detached and its final owner is released on the blocking pool. A
focused serialization regression also guards camel-case `scanId` fields for the
enum channel contract.

The native harness now accepts a separate cancellation fixture and requires an
enabled Stop control, the `Scan stopped.` landing state, focus restoration to
that notice, and an available recovery scan entry point before continuing
through the ordinary scan, keyboard, search, discard, stale-ID, and rescan flow.
A requested 25,000-directory fixture was too fast to expose Stop reliably on
the measured Linux host, so CI uses 100,000 requested leaf directories with one
empty file each. Fixture creation took 1.57 seconds there.

Three production macOS WebView runs on that larger fixture acknowledged and
rendered cancellation in 50–125 ms. One production Linux WebKitGTK run under
Xvfb, DBus, and the same unprivileged helper namespace used by the earlier smoke
completed it in 147 ms. All four restored cancellation focus and recovery,
reported no page error, and subsequently completed the full lifecycle. The
Linux candidate was compiled with Rust 1.97.0. These are end-to-end warm runtime
correctness observations on two hosts, not filesystem throughput benchmarks or
upper bounds for blocked syscalls. Exact assertions and environment boundaries
are preserved in
[`validation-results/2026-07-16-native-scan-cancellation.txt`](validation-results/2026-07-16-native-scan-cancellation.txt).

A follow-up audit found that the cancellation preflight advanced the completed
scan from ID 1 to ID 2, while the lifecycle controller still tried to reopen ID
1 after Home. That ID was already unavailable because it belonged to the
cancelled scan, so the assertion could pass without proving that Home released
the completed snapshot. The harness now derives the expected completed ID,
reports it, reopens exactly that ID, and requires Rust-side report validation to
match it. Production macOS WebKit passed with discarded ID 2 after cancellation
and ID 1 without the preflight; production Linux WebKitGTK passed with discarded
ID 2. All three then completed the second scan with restored result focus. This
strengthens lifecycle correctness evidence; it does not change scanner
performance results.

### 2026-07-16 macOS real-tree refresh

The current `ace070f` source (tree `a936fd1`) was remeasured on the same Apple
M4 Pro class of machine under macOS 15.6 with 48 GiB RAM, 14 logical CPUs, Rust
1.94.0-nightly, and an APFS data volume at 94% capacity. Each throughput result
uses one warmup followed by seven measured release scans. These are warm-cache
observations, not cold-storage claims.

Two distinct, quiescent real-tree shapes were compared. The build checkout had
221,920 retained entries, 8,541 directories, 28 GiB of source and build output,
and 127,217 duplicate hard links. The source registry had 63,904 entries, 10,395
directories, 1.6 GiB of dependency sources, and no duplicate hard links. The
parity harness matched file and directory counts, logical and allocated bytes,
unavailable and filesystem-boundary counts, duplicate hard links, and every
accounting-semantics flag on both trees.

| Workload | Backend | Median wall | Range | Median entries/s | Change |
| --- | --- | ---: | ---: | ---: | ---: |
| Build checkout | `jwalk` | 867.70 ms | 809.08–1,147.98 ms | 255,758 | reference |
| Build checkout | `getattrlistbulk` | 136.41 ms | 134.39–210.96 ms | 1,626,848 | 84.3% lower |
| Source registry | `jwalk` | 98.50 ms | 90.82–101.75 ms | 648,747 | reference |
| Source registry | `getattrlistbulk` | 72.50 ms | 65.90–81.84 ms | 881,460 | 26.4% lower |

Nine asynchronous cancellations on the larger tree requested cancellation at
the first progress boundary at or beyond 2,048 entries. Native median latency
was 1,693 us with a 2,339 us maximum, versus 9,773 us and 11,690 us for `jwalk`.
The native median was 82.7% lower. This bounds userspace response after the
observed progress event; it does not bound a blocked filesystem syscall.

Three one-warmup/one-measured process observations under `/usr/bin/time -l`
reported a median 81.5 MiB native maximum RSS versus 160.7 MiB for `jwalk`, and
a median 63.8 MiB native peak memory footprint versus 95.6 MiB. The benchmark's
capacity-aware retained snapshot was identical between backends at 43,316,312
bytes, or 195.19 bytes per entry. Process memory includes the harness, allocator,
threads, and transient traversal state, so this is not a retained-arena-only
claim.

The earlier eight-initial-worker experiment on a 110,101-entry synthetic APFS
fixture was invalidated when unrelated compiler and simulation workloads began
saturating the host. A clean rerun based on `215d8d9` built separate release
binaries for the unchanged four-worker policy and the isolated eight-worker
candidate. Each process warmed the filesystem before one measured scan, the
first variant alternated for 31 pairs, every workload/accounting field matched,
and a per-pair guard aborted if the interfering workloads reappeared.

Four initial workers had a 114.67 ms median and 118.90 ms p95 wall time. Eight
workers had a 115.54 ms median and 130.43 ms p95. The eight-worker candidate was
1.26% slower at the paired median and lost 22 of 31 pairs; paired changes ranged
from -3.22% to +17.16%. Traversal, not aggregation or snapshot release, accounted
for the difference. In 21 alternating cancellation pairs at exactly 2,048
entries, four workers returned in 87 us median and 143 us maximum versus 119 us
and 232 us for eight. Seven alternating `/usr/bin/time -l` pairs reported a
35.94 MiB median maximum RSS and 27.08 MiB peak footprint for four workers
versus 37.09 MiB and 28.34 MiB for eight.

The eight-initial-worker candidate is therefore rejected. Keep the bounded
four-worker start and conditional expansion to eight unless a materially
different scheduler design is evaluated across representative shapes. The
clean throughput, cancellation, and process-memory trials are preserved in
[`performance-results/2026-07-16-macos-worker-rerun.csv`](performance-results/2026-07-16-macos-worker-rerun.csv),
[`performance-results/2026-07-16-macos-worker-cancellation.csv`](performance-results/2026-07-16-macos-worker-cancellation.csv),
and
[`performance-results/2026-07-16-macos-worker-memory.csv`](performance-results/2026-07-16-macos-worker-memory.csv).

Raw stable samples and the original invalid scheduler record are preserved in
[`performance-results/2026-07-16-macos-real-tree-refresh.csv`](performance-results/2026-07-16-macos-real-tree-refresh.csv).

## Interpretation and next measurements

Most results are warm-cache, synthetic metadata measurements on one machine;
the real-tree observations still cover only two shapes on one local APFS
machine. They do
not measure cold storage, network volumes, antivirus interference, native IPC
transport, production platform WebViews, or other operating systems. The RSS
measurements include the benchmark process and allocator, not just
snapshot-owned bytes. These results are a regression baseline, not a universal
speed claim.

Before generalizing these results beyond the measured workloads, add:

- additional representative real directory trees and cold-cache runs;
- snapshot-owned bytes and process-RSS scaling on representative million-entry
  trees with diverse names and directory shapes;
- native Tauri IPC transport and production first-render timing on each platform
  WebView;
- broader Linux filesystem/hardware coverage, cold-cache throughput, and
  scheduler profiling without restricted performance counters;
- broader portable-versus-native parity, cold-cache throughput, and peak-memory
  scaling on representative Windows system volumes.
