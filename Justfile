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
    {{ cargo_env }} cargo test --manifest-path src-tauri/Cargo.toml

# Build the frontend and native executable without packaging it.
build:
    {{ cargo_env }} bun run tauri build --no-bundle

# Build platform desktop bundles.
bundle:
    {{ cargo_env }} bun run tauri build
