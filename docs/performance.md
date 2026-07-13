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
- `aggregationUs`: bottom-up propagation of file and directory totals.
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
were clean and all 34 frontend tests passed under Bun 1.2.21. On a fresh
4,099-file ext4 fixture, `jwalk` and `statx` matched all accounting fields,
including one deduplicated hard link and a directory symlink that was not
followed. Three `statx` cancellations returned in 70–155 us, and the new
single-run observation harness completed the same fixture through `statx`. A
desktop-feature Cargo check still stops specifically at the absent system
`dbus-1.pc`; the compiler update therefore closes the Rust MSRV question but
does not supply Linux desktop development packages. Smoke values are preserved
in
[`performance-results/2026-07-13-linux-rust197-smoke.csv`](performance-results/2026-07-13-linux-rust197-smoke.csv).

The native Windows release harness passed 35 tests. Eleven interleaved runs on
an 8,001-file NTFS fixture compared otherwise identical binaries before and
after the extra `FileBasicInfo` query. Median MFT traversal changed from 122.22
to 118.62 ms, so this sample likewise found no measurable regression. These are
warm synthetic results, not evidence about cold-cache or antivirus-heavy
systems. Summary samples are preserved in
[`performance-results/2026-07-12-scan-revision-impact.csv`](performance-results/2026-07-12-scan-revision-impact.csv).

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

## Interpretation and next measurements

Most results are warm-cache, synthetic metadata measurements on one machine;
the one real-tree observation is still a single local APFS checkout. They do
not measure cold storage, network volumes, antivirus interference, native IPC
transport, production platform WebViews, or other operating systems. The RSS
measurements include the benchmark process and allocator, not just
snapshot-owned bytes. These results are a regression baseline, not a universal
speed claim.

Before generalizing these results beyond the measured workloads, add:

- additional representative real directory trees and cold-cache runs;
- snapshot-owned bytes per entry and scaling beyond 100,000 entries;
- cancellation latency during deliberately long aggregation work;
- native Tauri IPC transport and production first-render timing on each platform
  WebView;
- broader Linux filesystem/hardware coverage, cold-cache throughput, and
  scheduler profiling without restricted performance counters;
- broader portable-versus-native parity, cold-cache throughput, and peak-memory
  scaling on representative Windows system volumes.
