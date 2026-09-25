# Windows cloud exclusion validation — G14W, 2026-09-25

Implemented on top of `585390a7ab573bbec51791d6d81ed8186a060f2f`.
The candidate is the uncommitted working tree identified by the 142 matching
source/build-input SHA-256 values in the [evidence ledger](2026-09-25-windows-cloud.json).
That ledger also retains binary hashes, every timing pair, cancellation runs,
the real-volume observation, and both cloud integration-test logs.

Host: G14W, Windows 11 Home 10.0.26200, x86-64, 16 logical CPUs;
Rust/Cargo 1.97.0 and Bun 1.3.14. Source and build outputs were isolated under
`C:\Users\G14\AppData\Local\Temp\cepa-windows-cloud-20260925`.
The candidate used a fresh `target` directory; the untouched baseline used a
separate `baseline` source export and fresh `baseline-target`. Subsequent edits
were rebuilt and hashes were compared with the local working tree.

## Correctness and safety

- **139 full-feature Rust tests passed**, plus strict all-target Clippy and
  formatting. **103 core Rust tests passed**, plus core all-target Clippy.
- Frontend checks passed with zero errors/warnings; **79 frontend tests passed**.
- Production Windows release build passed using
  `bun --bun run tauri build --no-bundle`. This is executable build evidence,
  not Windows WebView2 interaction or installer validation.
- The installed `just.exe` launcher was not executable. Validation expanded the
  existing `just check` recipes into the same Bun/Cargo commands instead.
- A connected, disposable Cloud Files provider created a remote file, a remote
  directory, a downloaded cloud file, a downloaded cloud directory with a local
  child, and an ordinary local file. Both Win32 traversal and the production MFT
  pipeline retained **3 files and 1 directory, 4,150 logical bytes and 4,160
  allocated bytes**, with **2 cloud exclusions and no unavailable items**.
- Both scanners produced **zero data-fetch or directory-population callbacks**.
  Placeholder roots and paths through unavailable ancestors were rejected.
  An on-disk-only query against an already-cloud-only held directory also caused
  no population callback. An ordinary content read afterward triggered **2
  callbacks**, confirming that the connected provider instrumentation worked.
  The provider deliberately fails fetches; the positive control downloads no
  content. Its sync root and disposable files are removed on scope exit.
- The MFT fixture exercises the same enumeration, classification, measurement,
  accounting, and completion code with test-only subtree admission. Separately,
  a production `auto` scan of `R:\` actually selected **MFT**: **41,477 files,
  1,706 directories, 96 duplicate hard links, zero unavailable items**.
- A read-only scan of the existing university OneDrive tree retained **1,595
  local files and 2,206 directories**, skipped **46,053 cloud-only entries**, and
  reported **zero unavailable items**. Totals were 55,018,467,188 logical bytes
  and 55,021,735,080 allocated bytes. The **2 directly sampled placeholders**
  retained their attributes and lengths. This is not an assertion that every
  excluded entry was independently checked afterward. The test took 0.92 s.
- Regression tests cover recall versus pin/unpin/EA flags, downloaded reparse
  classification, scoped thread-policy restoration, relative opens after parent
  rename/replacement, local provider-named files, NTFS hard-link accounting,
  counted UTF-16 directory records/malformed buffers, and eviction after MFT admission.

## Timing and cancellation

Seven alternating baseline/candidate pairs per fixture, each invocation with
one warmup followed by one measured scan. Wall time includes root resolution,
traversal, aggregation, and the initial bounded view. It excludes the separately
measured response serialization and snapshot release. These are one-machine,
warm-cache observations, not a general Windows speedup claim.

| Local fixture | Previous jwalk median | Protected Win32 median | Median paired change |
| --- | ---: | ---: | ---: |
| 10,001 files / 41 directories | 258.382 ms | 146.035 ms | -43.48% |
| 5,001 files / 1,010 directories | 161.379 ms | 200.004 ms | +24.21% |

The directory-heavy regression remains a tradeoff. The new walker performs
handle-relative validation and local-only enumeration, reports native allocation,
and supports NTFS hard-link deduplication and volume boundaries. The former
Windows jwalk path reported logical size as an allocation estimate and did not
provide those latter accounting guarantees. File counts, directory counts,
logical bytes, and unavailable-item counts matched on both timing fixtures.

An initial serial protected walker was rejected after measured regressions of
56–88%. The final implementation uses at most eight metadata workers with
32-entry handoffs, installs placeholder exposure separately on each worker,
and avoids file-only metadata queries for directories.

On the 100,001-file / 101-directory cancellation fixture, five requests occurred
while traversal was active, after 4,868–6,753 retained entries. Cancellation
latency was **0.683–0.802 ms**, median **0.705 ms**. The 100 ms progress cadence and
per-entry cancellation checks remain independent.

## Commands and evidence boundaries

Core and full checks use the repository's Cargo flags (`--all-targets`, strict
Clippy, with and without `--no-default-features`). Opt-in tests:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features `
  cloud_files_fixture -- --ignored --nocapture
$env:CEPA_CLOUD_FIXTURE_ROOT = 'C:\Users\G14\OneDrive - Georgia Institute of Technology'
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features `
  existing_cloud_placeholders -- --ignored --nocapture
```

The full scripts and raw logs are retained locally at
`/tmp/cepa-windows-cloud-20260925` and in the corresponding G14W directory.
`cepa_final.ps1` records the final check/build/fixture/benchmark sequence.
No standing G14W checkout or existing cloud file was modified. No commit or
publication was performed.

On macOS, 42 scanner tests, strict core Clippy, and frontend checks/tests passed.
The broader core suite hit `reads_uncompressed_and_compressed_decmpfs_file_state`:
`ditto --hfsCompression` produced a file reported as uncompressed. The identical
failure reproduced on a clean export of the original revision with a separate
fresh target directory; the Windows change does not fix that pre-existing issue.

Qualification is Windows Cloud Files on NTFS. This does not establish universal
protection for proprietary virtual drives/network filesystems, a cold-cache
performance result, Windows UI/assistive-technology coverage, or compression
mutation support. See [accounting semantics](../accounting.md#cloud-backed-storage-on-windows).

A later [Google Drive streaming test](2026-09-25-google-drive-windows.md) found
that its FAT32 virtual drive exposes ordinary attributes and virtual allocation:
the current scanner counted its entries with zero cloud exclusions. The tested
content cache stayed unchanged, but Google Drive streaming did not pass
local-only accounting qualification.
