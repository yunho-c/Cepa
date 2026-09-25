# Google Drive streaming guard — G14W, 2026-09-25

Checkpoint `f202b8f` preserves the initial Windows cloud-placeholder implementation
and the [Google Drive accounting failure](2026-09-25-google-drive-windows.md).
This follow-up prevents the virtual stream from being scanned as local storage.
It excludes the entire streaming namespace; it does not selectively retain cached
files through `G:`. Mirrored folders and physical cache files remain scannable.

## Implementation

`windows_streams.rs` enumerates live filesystem driver objects and asks Windows
whether each recognized Google Drive driver is attached to the opened volume.
On G14W, `\FileSystem\googledrivefs31931` was attached to `G:` and absent from
the queried `C:` and `U:` handles. The implementation recognizes unversioned and
numeric-suffix driver names, rather than hard-coding that version.

The [driver-path query](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_fs_driver_path_information)
is filesystem-independent and does not send an IRP to the provider. The check
runs after a metadata-only volume open and before resolving descendant components,
querying their metadata, or enumerating directories. It is shared by root
validation, Auto, Win32, the Jwalk compatibility selector, and MFT. Confirmed
streaming volumes are omitted from storage discovery. Probe failures remain
visible to normal validation, and scans never retry without the check.

This uses no Google account credentials, private cache database, folder-name
blacklist, drive-letter assumption, volume-label test, or FAT32 exclusion. No
Google sync or availability setting is changed. The selected-root error directs
the user to the local disk containing the cache or a mirrored folder.

## Validation

The existing streaming volume, its `My Drive` child, and a nonexistent child
were rejected by validation and all four scan selectors with **zero progress
events**. Rejecting the nonexistent descendant with the same provider error also
checks that the volume guard precedes child resolution. The same ignored test
passed through a disposable `subst` drive alias, which was removed afterward.
Storage discovery excluded the stream. The local-file regression counted both
`Google Drive` and `OneDrive` directories normally and retained deterministic
hard-link accounting.

The rebuilt release observation tool rejected the previously miscounted
`G:\My Drive\Colab Notebooks` with exit code 2 and the provider explanation,
without returning a result snapshot. A scan of the physical Google content
cache on `C:` retained **5 files and 68 directories, 1,982,621 logical bytes,
2,027,520 allocated bytes**, and **zero unavailable items**. File count and
logical bytes matched an independent local directory-metadata inventory.

The real OneDrive regression still retained **1,595 local files and 2,206
directories**, skipped **46,053 cloud entries**, and reported **zero unavailable
items**; both sampled placeholders retained their attributes and lengths.
The connected Cloud Files fixture still matched on Win32/MFT: **3 files, 1
directory, 4,150 logical bytes, 4,160 allocated bytes**, and **2 cloud exclusions**.
Both scans caused **zero fetch/population callbacks**; the ordinary-read positive
control triggered **2 callbacks**.

The inspected Google content-cache manifest (relative names, lengths, mtimes)
was unchanged across the provider tests. This is not a network trace or Google
callback instrumentation. Qualification covers the observed Google driver on
G14W, not every provider or future driver naming scheme. No Windows WebView2
interaction or installer testing is claimed.

Windows checks passed **142 full-feature Rust tests**, **106 core Rust tests**,
strict all-target Clippy in both modes, formatting, frontend diagnostics with
zero errors/warnings, and **79 frontend tests**. The Windows production release
build passed with `bun --bun run tauri build --no-bundle`; executable hashes are
in the ledger. The non-executable `just.exe`
launcher was bypassed by expanding its Cargo/Bun recipes. macOS checks passed
**42 scanner tests and 7 storage-discovery tests**, strict core Clippy, and the
frontend diagnostics/tests. The previously documented unrelated macOS compression
fixture failure was not rerun or changed in this follow-up.

Build inputs and final check results are recorded in the
[evidence ledger](2026-09-25-google-drive-guard.json). Raw artifacts are kept in
`/tmp/cepa-windows-cloud-20260925/google-fix` and the corresponding isolated G14W
directory. The original source/build tree was reused with changed inputs rebuilt;
the manifest was compared against the remote source before validation. No standing
G14W checkout was changed.
