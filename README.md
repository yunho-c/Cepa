# Cepa

Cepa is a desktop-only application foundation built with Bun, Tauri 2, Rust,
Svelte 5, Tailwind CSS 4, and shadcn-svelte.

The starter screen contains one complete frontend-to-native example: submitting
the form invokes the typed `greet` command in Rust and renders its response.

## Prerequisites

- [Bun](https://bun.sh/)
- A Rust toolchain managed by [rustup](https://rustup.rs/)
- [just](https://just.systems/) for the repository workflows
- The [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/)
  for your desktop platform

## Development

```sh
just install
just dev
```

`just web` starts only the Vite frontend. Use `just dev` for the native
application and Rust command bridge. Run `just` to list every available recipe.

## Checks and builds

```sh
just check
just build
```

`just check` runs Svelte diagnostics, Rust formatting checks, `cargo check`, and
the Rust tests. Native recipes clear the machine's configured `sccache` wrapper
for the command so it cannot block Cargo.

```sh
just bundle
```

`just bundle` generates the platform desktop bundles. Mobile targets are
intentionally not initialized or configured.
