# Google Drive streaming validation — G14W, 2026-09-25

**The current Windows implementation does not exclude cloud-only Google Drive
streaming entries on this host.** The scan completed without observed content-cache
growth, but counted virtual files and their provider-reported allocation as local
storage. This is a failed local-only accounting qualification, not a passing
Google Drive cloud-exclusion test.

## Environment and provenance

G14W exposes Google Drive at `G:` with filesystem name `FAT32`. Running
GoogleDriveFS processes reported versions `130.0.2.0` and `131.0.2.0`; this
observation does not establish which process owned the mount. Google's
[filesystem documentation](https://support.google.com/drive/answer/2375082?hl=en)
describes its virtual drive as FAT, independently of the NTFS content cache.

Used the unchanged release `scan_observation.exe` from the isolated build in
[the Windows validation report](2026-09-25-windows-cloud.md). Its SHA-256 is
`7a58ab3d52b5e53ddd17d97f8d63b89884d52cf83cd818f24a975b05c07a768c`.
A separate, bounded, one-directory metadata probe compiled a byte-identical copy
of production `windows_local.rs`, including its placeholder exposure, protected
root/child opens, and on-disk-only directory queries. The probe read no file
contents. The [evidence ledger](2026-09-25-google-drive-windows.json) records
binary/source hashes, aggregate metadata observations, and the scan result.

## Observations

- Protected enumeration succeeded at `G:\` and `G:\My Drive`; accepting the
  local-only query flag did not imply that Drive returned only resident entries.
- `My Drive` returned **775 direct entries, zero with recall/offline flags**.
  Its first 40 sampled entries also had no Cloud Files reparse tag. Sampled files
  exposed ordinary `FILE_ATTRIBUTE_NORMAL` and virtual allocation sizes.
- The existing-placeholder integration test at `G:\` failed its fixture
  precondition because there were no directly enumerated entries carrying recall
  flags. It did not reach its scan or before/after placeholder assertions.
- A production `auto` scan of `G:\My Drive\Colab Notebooks` selected `win32`:
  **760 files, 136 directories, 789,758,598 logical bytes, 789,851,648 allocated
  bytes, zero cloud exclusions, zero unavailable items**. It completed in
  **16,062.295 ms**. The result incorrectly advertised the provider's allocation
  as exact local on-disk usage (`allocatedSizeIsEstimate: false`).
- All 55 direct entries of that subtree lacked recall flags. Its first 40
  sampled entries had identical attributes, tags, logical lengths, and allocation
  before and after the scan.
- The inspected account's local `content_cache` contained **5 files totaling
  1,982,621 bytes** before and after. Its complete relative-name/length/mtime
  manifest was unchanged. No `ContentCachePath` override was present in the
  checked HKCU/HKLM Google DriveFS settings.

## Limits and follow-up

The unchanged cache provides evidence of no persistent content-cache growth in
the observed interval. It is not provider callback instrumentation or proof of
zero network traffic, transient downloads, or activity in another cache.
The probe does not independently identify which individual virtual files are
cached. File availability and sync settings were not changed, and no cloud file
contents were opened. The entire Google Drive tree was not scanned.

Google Drive streaming needs additional residency/accounting handling before
Cepa can claim local-only support for this setup. Do not infer support from a
successful protected API call, FAT32 filesystem name, or absence of recall bits;
ordinary local FAT volumes and downloaded provider files must remain usable.

Raw logs, probe source archive, and before/after manifests are retained under
`/tmp/cepa-windows-cloud-20260925/google-drive` and the corresponding isolated
G14W `google-drive` directory. Raw item names are omitted from the checked-in
ledger. No production code was changed for this follow-up test.
