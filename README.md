# Cepa

Cepa is a desktop-only application foundation built with Bun, Tauri 2, Rust,
Svelte 5, Tailwind CSS 4, and shadcn-svelte.

The starter screen contains one complete frontend-to-native example: submitting
the form invokes the typed `greet` command in Rust and renders its response.

## Prerequisites

- [Bun](https://bun.sh/)
- A Rust toolchain managed by [rustup](https://rustup.rs/)
- The [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/)
  for your desktop platform

## Development

```sh
bun install
bun run desktop:dev
```

`bun run dev` starts only the Vite frontend. Use `bun run desktop:dev` for the
native application and Rust command bridge.

## Checks and builds

```sh
bun run check
bun run build
cargo check --manifest-path src-tauri/Cargo.toml
bun run tauri build --no-bundle
```

If this machine's configured `sccache` wrapper blocks Cargo, clear it for the
command:

```sh
RUSTC_WRAPPER= CARGO_BUILD_RUSTC_WRAPPER= cargo check --manifest-path src-tauri/Cargo.toml
```

Desktop bundle generation is available through `bun run tauri build`. Mobile
targets are intentionally not initialized or configured.
