set dotenv-load

export RUSTC_WRAPPER := ""
export CARGO_BUILD_RUSTC_WRAPPER := ""

# List available recipes.
default:
    @just --list

# Install frontend dependencies from the Bun lockfile.
install:
    bun install --frozen-lockfile

# Run the native Tauri application in development mode.
dev:
    bun run desktop:dev

# Run only the Vite frontend.
web:
    bun run dev

# Run all static checks and tests.
check: frontend-check frontend-test rust-fmt rust-check rust-clippy test

# Check scanner, accounting, and compression code without desktop UI libraries.
native-check: rust-fmt
    cargo check --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features
    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features -- -D warnings
    cargo test --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features

# Validate the Linux compression inspector against a real mounted Btrfs filesystem.
validate-btrfs-compression path:
    bash scripts/validate-btrfs-compression.sh "{{ path }}"

# Launch two short native sessions and verify window geometry restoration.
window-state-smoke:
    cargo run --manifest-path src-tauri/Cargo.toml --example window_state_smoke -- seed
    cargo run --manifest-path src-tauri/Cargo.toml --example window_state_smoke -- verify

# Check the Svelte and TypeScript frontend.
frontend-check:
    bun run check

# Run deterministic frontend unit tests.
frontend-test:
    bun run test:frontend

# Regenerate desktop and store icons from the shared vector source.
icons:
    bun run icons

# Check Rust formatting without changing files.
rust-fmt:
    cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Type-check the Rust backend.
rust-check:
    cargo check --manifest-path src-tauri/Cargo.toml

# Lint every Rust target and fail on warnings.
rust-clippy:
    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# Run Rust tests.
test:
    cargo test --manifest-path src-tauri/Cargo.toml --all-targets

# Generate a deterministic metadata-heavy benchmark fixture at a new path.
benchmark-fixture path directories="100" files_per_directory="100" logical_bytes_per_file="0":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example generate_scan_fixture -- "{{ path }}" "{{ directories }}" "{{ files_per_directory }}" "{{ logical_bytes_per_file }}"

# Benchmark a complete scan and snapshot pipeline (one warmup plus N runs).
benchmark-scan path iterations="5" backend="jwalk":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example scan_benchmark -- "{{ path }}" "{{ iterations }}" "{{ backend }}"

# Record one scan of a live tree without assuming a stable warmup workload.
observe-scan path backend="auto":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example scan_observation -- "{{ path }}" "{{ backend }}"

# Benchmark bounded current-folder search after one completed scan.
benchmark-search path query iterations="9" backend="auto" metric="allocated":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example search_benchmark -- "{{ path }}" "{{ query }}" "{{ iterations }}" "{{ backend }}" "{{ metric }}"

# Compare backend accounting on a quiescent directory tree.
validate-scan path left="jwalk" right="auto":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example scan_parity -- "{{ path }}" "{{ left }}" "{{ right }}"

# Measure asynchronous cancellation latency after a progress boundary.
benchmark-cancellation path backend="jwalk" iterations="9" after_entries="2048":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example scan_cancellation -- "{{ path }}" "{{ backend }}" "{{ iterations }}" "{{ after_entries }}"

# Build the frontend and native executable without packaging it.
build:
    bun run tauri build --no-bundle

# Build platform desktop bundles.
bundle:
    bun run tauri build
