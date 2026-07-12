set dotenv-load

cargo_env := "env RUSTC_WRAPPER= CARGO_BUILD_RUSTC_WRAPPER="

# List available recipes.
default:
    @just --list

# Install frontend dependencies from the Bun lockfile.
install:
    bun install --frozen-lockfile

# Run the native Tauri application in development mode.
dev:
    {{ cargo_env }} bun run desktop:dev

# Run only the Vite frontend.
web:
    bun run dev

# Run all static checks and tests.
check: frontend-check rust-fmt rust-check test

# Check the Svelte and TypeScript frontend.
frontend-check:
    bun run check

# Check Rust formatting without changing files.
rust-fmt:
    {{ cargo_env }} cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Type-check the Rust backend.
rust-check:
    {{ cargo_env }} cargo check --manifest-path src-tauri/Cargo.toml

# Run Rust tests.
test:
    {{ cargo_env }} cargo test --manifest-path src-tauri/Cargo.toml --all-targets

# Generate a deterministic metadata-heavy benchmark fixture at a new path.
benchmark-fixture path directories="100" files_per_directory="100" logical_bytes_per_file="0":
    {{ cargo_env }} cargo run --release --manifest-path src-tauri/Cargo.toml --example generate_scan_fixture -- "{{path}}" "{{directories}}" "{{files_per_directory}}" "{{logical_bytes_per_file}}"

# Benchmark the complete portable scan and snapshot pipeline (one warmup plus N runs).
benchmark-scan path iterations="5":
    {{ cargo_env }} cargo run --release --manifest-path src-tauri/Cargo.toml --example scan_benchmark -- "{{path}}" "{{iterations}}"

# Build the frontend and native executable without packaging it.
build:
    {{ cargo_env }} bun run tauri build --no-bundle

# Build platform desktop bundles.
bundle:
    {{ cargo_env }} bun run tauri build
