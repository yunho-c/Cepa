# Cepa Agent Guide

## Product direction

Cepa is a cross-platform, desktop-only disk space analyzer. Its goal is to make
understanding and reclaiming storage fast enough, clear enough, and pleasant
enough that users spend less time manually cleaning up or moving files.

The two primary product requirements are:

1. Maximize scan and analysis performance without compromising correctness or
   making the application difficult to use.
2. Deliver an exquisite, minimal interface with the restraint, hierarchy, and
   polish of shadcn/ui.

Transparent filesystem compression is a future product capability. Do not
assume its semantics, platform coverage, safety model, or user experience until
those have been designed and documented.

## Current state and roadmap

The repository contains the first portable scanner milestone. It can select and
scan a directory with `jwalk`, stream bounded progress over a Tauri channel,
cancel active work, retain an in-memory result snapshot for drill-down, and
render coordinated radial and list views in Svelte.

The intended scanning architecture is:

- `jwalk` as the implemented portable fallback and behavioral reference.
- `getattrlistbulk` for an optimized macOS implementation.
- Master File Table (MFT) traversal for an optimized Windows implementation.
- `statx`-based traversal for an optimized Linux implementation.

Keep roadmap items described as planned until the code and validation exist.
Do not present compilation, UI wiring, or a mocked scan as proof of real
filesystem performance.

## Technology and architecture

Cepa uses Tauri 2, Rust, Svelte 5, TypeScript, Tailwind CSS 4,
shadcn-svelte, Bun, and `just`.

Keep filesystem traversal, metadata collection, aggregation, and other
performance-sensitive work in Rust. Keep Svelte focused on presentation and
interaction; business logic and scanning logic do not belong in components.

When implementing scanning:

- Put traversal implementations behind one backend-neutral interface so the
  portable and platform-specific backends produce compatible results.
- Isolate platform-specific code. Every supported platform must retain a
  portable fallback when its optimized backend is unavailable or unsuitable.
- Stream bounded progress and incremental results to the UI. Do not require a
  complete scan before showing useful information, and do not flood the Tauri
  bridge with an event for every filesystem entry.
- Make scans cancellable and keep cancellation responsive during traversal,
  aggregation, and transport.
- Treat permission errors, disappearing entries, symlinks, hard links, mount
  boundaries, sparse files, and apparent versus allocated size as explicit
  correctness decisions. Document the chosen behavior and test it.
- Do not silently follow links or cross filesystem boundaries. Any such policy
  must be deliberate, visible to the caller, and consistent across backends.
- Keep wire types stable and compact. Avoid serializing internal traversal
  structures directly into frontend-facing APIs.

## Performance expectations

Performance claims require measurements. Establish a representative baseline,
record the environment and dataset shape, and compare like-for-like behavior.
Separate cold startup, traversal throughput, aggregation cost, bridge/update
cost, and UI rendering responsiveness when diagnosing performance.

In hot paths, pay particular attention to unnecessary allocations, path and
string conversions, repeated metadata syscalls, synchronization contention,
serialization volume, and overly frequent frontend updates. Prefer bounded
parallelism and bounded queues. Faster traversal must not cause unbounded memory
growth, nondeterministic accounting, or sluggish cancellation.

Use the portable backend as the behavioral reference for optimized backends.
Add parity tests for shared semantics and platform-specific tests for native
behavior. Keep deterministic microbenchmarks separate from end-to-end scans,
and preserve benchmark evidence when an optimization drives a design change.

## UI and interaction standards

Build on the existing shadcn-svelte components and neutral design tokens in
`src/app.css`. Favor restrained color, strong typography, deliberate spacing,
clear hierarchy, and information-dense layouts that remain calm under large or
rapidly changing datasets.

Use DaisyDisk's primary exploration pattern as the layout direction: a
hierarchical pie chart on the left and an interactive file-and-directory list
on the right. Treat the two views as one coordinated navigator—hover,
selection, drill-down, breadcrumbs, and the current path should stay in sync so
users can move fluidly between spatial and textual exploration.

Every workflow should have intentional empty, loading, partial-result, error,
cancelled, and completed states. Preserve keyboard navigation, visible focus,
semantic controls, readable contrast, and reduced-motion usability. Progressive
updates should feel smooth without hiding freshness or blocking interaction.

Avoid decorative complexity, excessive animation, generic dashboard layouts,
and bespoke controls when an existing shadcn-svelte primitive fits. Any
shadcn-svelte component may be used where it improves the experience, including
the shadcn-svelte LayerChart integration for the hierarchical visualization.
Add UI components through the repository's configured shadcn-svelte setup and
preserve the aliases and styling conventions in `components.json`.

## Repository map

Start with these files:

- `README.md`: current setup and developer workflow.
- `src/App.svelte`: scan workflow and coordinated storage explorer.
- `src/lib/scanner.ts`: frontend scan protocol types and formatters.
- `src/app.css`: Tailwind setup and the shared shadcn-svelte theme tokens.
- `src/lib/components/ui/`: reusable shadcn-svelte UI primitives.
- `src-tauri/src/lib.rs`: Tauri commands and active/completed scan lifecycle.
- `src-tauri/src/scanner.rs`: portable traversal, aggregation, and view snapshots.
- `src-tauri/Cargo.toml` and `package.json`: Rust and frontend dependencies.
- `Justfile`: canonical development, checking, building, and bundling commands.

As the application grows, prefer small modules with clear ownership over adding
scan, state, and visualization logic directly to the current entry files.

## Development workflow

Use the repository workflows rather than inventing parallel command sequences:

```sh
just install   # install frontend dependencies from the Bun lockfile
just dev       # run the native Tauri application
just web       # run only the Vite frontend
just check     # frontend diagnostics, Rust formatting, checks, and tests
just build     # build the frontend and native executable without packaging
just bundle    # produce platform desktop bundles
```

The native recipes deliberately clear configured Rust compiler wrappers so a
machine-level `sccache` configuration cannot block Cargo. Prefer these recipes
when validating Rust or Tauri work.

## Change and validation discipline

- Inspect the relevant implementation, manifests, and existing conventions
  before changing architecture or adding dependencies.
- Keep changes narrow. Preserve unrelated worktree changes and generated files,
  and do not reformat unrelated code.
- Add focused Rust tests for meaningful scanner, accounting, protocol, and
  error-handling logic. Add frontend checks or tests when interaction or state
  behavior becomes nontrivial.
- For scanning changes, exercise real filesystem fixtures covering ordinary
  trees and the relevant edge cases. A successful build alone is not evidence
  that traversal or accounting works.
- For performance changes, report the baseline, comparison, workload, and
  measurement method. Do not claim a speedup from intuition or a synthetic test
  that measures different behavior.
- Run `just check` for normal code changes. Run the narrowest relevant checks
  while iterating, then the full suite before handoff when feasible.
- Report exactly what was validated and distinguish static checks, mocked or
  fixture-based tests, real local scans, platform-specific validation, and
  end-to-end application proof.

When a requested change conflicts with correctness, portability, user safety,
or measured performance, surface the tradeoff explicitly instead of silently
choosing one priority.
