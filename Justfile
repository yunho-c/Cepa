set dotenv-load

export RUSTC_WRAPPER := ""
export CARGO_BUILD_RUSTC_WRAPPER := ""

# Prefer Apple's system bundle tools and seal local macOS apps with an ad-hoc
# identity. A supplied Developer ID identity still takes precedence.
bundle-environment := if os() == "macos" {
    'PATH="/usr/bin:$PATH" APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:--}"'
} else {
    ''
}

# List available recipes.
default:
    @just --list

# Install frontend dependencies from the Bun lockfile.
install:
    bun install --frozen-lockfile

# Run the native Tauri application in development mode.
dev:
    bun --bun run desktop:dev

# Run only the Vite frontend.
web:
    bun --bun run dev

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

# Exercise the production WebView and IPC scan flow at the supported minimum size.
native-scan-smoke path cancellation_path="" failure_path="":
    bun --bun run build
    if [ -n "{{ failure_path }}" ]; then test -n "{{ cancellation_path }}" || { echo "failure_path requires cancellation_path" >&2; exit 2; }; cargo run --release --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol --example native_scan_smoke -- "{{ path }}" "{{ cancellation_path }}" "{{ failure_path }}"; elif [ -n "{{ cancellation_path }}" ]; then cargo run --release --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol --example native_scan_smoke -- "{{ path }}" "{{ cancellation_path }}"; else cargo run --release --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol --example native_scan_smoke -- "{{ path }}"; fi

# Run the production WebView scan proof under isolated Linux display/session buses.
native-scan-smoke-linux path cancellation_path="" failure_path="":
    dbus-run-session -- xvfb-run -a just native-scan-smoke "{{ path }}" "{{ cancellation_path }}" "{{ failure_path }}"

# Check the Svelte and TypeScript frontend.
frontend-check:
    bun --bun run check

# Run deterministic frontend unit tests.
frontend-test:
    bun run test:frontend

# Regenerate desktop and store icons from the shared vector source.
icons:
    bun --bun run icons

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

# Alternate two backends over stable pairs and reject accounting drift.
benchmark-compare path left="jwalk" right="auto" pairs="9":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example scan_comparison -- "{{ path }}" "{{ left }}" "{{ right }}" "{{ pairs }}"

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

# Measure asynchronous cancellation inside bottom-up scan aggregation.
benchmark-aggregation-cancellation nodes="1000000" iterations="9" after_nodes="500001":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example aggregation_cancellation -- "{{ nodes }}" "{{ iterations }}" "{{ after_nodes }}"

# Measure complete-file plan hashing and cancellation at its bounded chunk boundary.
benchmark-content-integrity path iterations="5" cancel_after_bytes="":
    cargo run --release --manifest-path src-tauri/Cargo.toml --no-default-features --example content_integrity_benchmark -- "{{ path }}" "{{ iterations }}" {{ if cancel_after_bytes == "" { "" } else { quote(cancel_after_bytes) } }}

# Compare cached and per-call compact count formatting with alternating order.
benchmark-count-format iterations="9" operations="10000":
    bun --bun scripts/benchmark-count-format.ts "{{ iterations }}" "{{ operations }}"

# Build the frontend and native executable without packaging it.
build:
    bun --bun run tauri build --no-bundle

# Require a built Linux desktop executable to survive an isolated display/session window.
smoke-linux-desktop executable="src-tauri/target/release/cepa" survival_seconds="8":
    bash scripts/smoke-linux-desktop.sh "{{ executable }}" "{{ survival_seconds }}"

# Validate the metadata and installed-file surface of completed Linux bundles.
validate-linux-bundles bundle_root="src-tauri/target/release/bundle":
    bash scripts/validate-linux-bundles.sh "{{ bundle_root }}"

# Validate the metadata, code seal, and mounted payload of completed macOS bundles.
validate-macos-bundles bundle_root="src-tauri/target/release/bundle":
    bash scripts/validate-macos-bundles.sh "{{ bundle_root }}"

# Validate the metadata and executable payload of completed Windows installers.
validate-windows-bundles bundle_root="src-tauri/target/release/bundle":
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts/validate-windows-bundles.ps1 "{{ bundle_root }}"

# Build platform desktop bundles.
bundle:
    {{ bundle-environment }} bun --bun run tauri build
