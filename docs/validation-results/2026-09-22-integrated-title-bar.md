# Integrated title bar validation

Validated locally on macOS on 2026-09-22, in the `Cepa-title-bar` worktree.

## Implementation

- A 40-pixel drag region shares the app background and reserves content space.
- macOS uses Tauri's overlay title bar with native traffic lights. Windows and
  Linux remove decorations before startup placement and use webview controls.
- The existing geometry persistence flags and startup visibility ordering remain
  unchanged. The geometry smoke uses the same decoration setup as production.
- Browser-only `titlebar=macos|windows|linux` previews cannot manage real windows.
- Window controls become inert while the folder-drop overlay owns interaction.

## Results

- Svelte/TypeScript: no errors or warnings; 79 frontend tests passed.
- Rust: 135 tests passed, one existing ignored test; formatting and type-check passed.
- `just check` stops at existing `needless_bool` in `scanner.rs` and
  `chunks_exact_to_as_chunks` in `scanner/mft.rs`. The remaining test recipe was
  run separately. Clippy passes with only those two lints allowed; scanner source
  was not changed.
- Fixed the native smoke validator's existing complete-report fixture, which
  lacked the already-required `metricDetailsStayedOpen` field. Added checks
  that reject a missing title bar or content overlapping its reserved area.
- Isolated `just window-state-smoke` passed both launches:
  `saved=900x650@120,140 restored=900x650@120,140`.
- Production-protocol native smoke passed at 620 × 480 with ordinary-directory,
  25,000-file cancellation, and regular-file failure fixtures. The report confirms
  title-bar clearance, no horizontal overflow, no page errors, metric switching,
  keyboard navigation, zero-match recovery, snapshot release (scan ID 3), and a
  second successful scan. An initial run during concurrent compilation timed out
  at metric switching; repeating the same executable and fixtures passed. These
  runs are correctness evidence, not timing measurements.
- Sixteen browser previews covered landing, completion, scanning, and picker
  failure at 620 × 480 and 880 × 620 in both appearances. All retained title-bar
  clearance, visible headings and manual-path recovery, two visible ranked rows
  where applicable, and no horizontal overflow or page errors. Compact failure
  content can scroll vertically. Windows preview maximize/restore changed its
  accessible label while retaining focus; drop preview made the controls inert.
- `just build` passed. A local ad-hoc-signed `.app` was built with Apple's system
  bundle tools first in PATH and inspected through native desktop automation.
  Traffic lights remained native; title-bar dragging and double-click
  maximize/restore were exercised. The first packaging attempt without the
  repository's system-tool PATH convention failed at `xattr`; using that
  convention succeeded.

## Scope

Rust builds used this worktree's own `src-tauri/target` for the final validation.
Browser screenshots and the preview report are in the ignored
`output/playwright/` directory. Windows and Linux were visually previewed but
not run on native hosts. No notarization, installer, or public-release claim is
made by this validation.
