# Scanner performance

Cepa treats performance as measured behavior, not a design claim. This document
defines the portable scanner benchmark, records the first reproducible baseline,
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
just benchmark-scan /tmp/cepa-fixture 9 > /tmp/cepa-report.json
```

The harness performs one unmeasured warmup followed by the requested measured
runs. Progress is written to stderr and schema-versioned JSON to stdout. It
verifies that counts and byte totals stay identical across runs.

Each run measures:

- `traversalUs`: `jwalk`, metadata reads, arena insertion, and path indexing.
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
| Directory-rich: 10,000 leaf directories × 10 files | 110,101 | 136.59 ms | 806,045 | 134.89 ms | 0.91 ms | 0.78 ms |
| Wide: 100 leaf directories × 1,000 files | 100,102 | 100.68 ms | 994,217 | 98.96 ms | 0.71 ms | 0.97 ms |

The raw runs are preserved in
[`performance-results/2026-07-11-m4-pro-arena.csv`](performance-results/2026-07-11-m4-pro-arena.csv).

### Snapshot memory

Peak resident memory was measured by running one warmup plus one measured scan
under macOS `/usr/bin/time -l`. The arena stores every full path once behind a
shared `Arc<Path>` and uses compact node IDs for parents and children.

| Workload | Path-map snapshot | Arena snapshot | Reduction |
| --- | ---: | ---: | ---: |
| Directory-rich | 108.36 MiB | 62.72 MiB | 42.1% |
| Wide | 101.28 MiB | 67.38 MiB | 33.5% |

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

## Interpretation and next measurements

These are warm-cache, synthetic metadata results on one machine. They do not
measure cold storage, network volumes, antivirus interference, Tauri
serialization, frontend rendering, cancellation latency, or other operating
systems. The RSS measurements include the benchmark process and allocator, not
just snapshot-owned bytes. These results are a regression baseline, not a
universal speed claim.

Before declaring a backend faster, compare identical semantics and add:

- representative real directory trees and cold-cache runs;
- snapshot-owned bytes per entry and scaling beyond 100,000 entries;
- cancellation latency under traversal and aggregation load;
- IPC serialization and first-render timing;
- portable-versus-native parity and throughput on each target platform.
