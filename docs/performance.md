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

- `traversalMs`: `jwalk`, metadata reads, and entry collection.
- `aggregationMs`: bottom-up propagation of file and directory totals.
- `indexingMs`: construction of the snapshot's parent-to-child index.
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

| Workload | Entries | Wall time | Entries/s | Traversal | Aggregation | Indexing | Initial view |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Directory-rich: 10,000 leaf directories × 10 files | 110,101 | 200.73 ms | 548,502 | 128 ms | 43 ms | 25 ms | 3.11 ms |
| Wide: 100 leaf directories × 1,000 files | 100,102 | 147.82 ms | 677,192 | 96 ms | 27 ms | 14 ms | 8.39 ms |

The raw runs are preserved in
[`performance-results/2026-07-11-m4-pro.csv`](performance-results/2026-07-11-m4-pro.csv).

## Optimization established by the baseline

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

## Interpretation and next measurements

These are warm-cache, synthetic metadata results on one machine. They do not
measure cold storage, network volumes, antivirus interference, Tauri
serialization, frontend rendering, peak memory, cancellation latency, or other
operating systems. They are a regression baseline, not a universal speed claim.

Before declaring a backend faster, compare identical semantics and add:

- representative real directory trees and cold-cache runs;
- peak resident memory and bytes of snapshot storage per entry;
- cancellation latency under traversal and aggregation load;
- IPC serialization and first-render timing;
- portable-versus-native parity and throughput on each target platform.
