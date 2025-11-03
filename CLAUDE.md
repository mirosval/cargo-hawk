# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Cargo Hawk is a Rust TUI (Terminal User Interface) application built with `ratatui` and `watchexec`. The project is in early stages with minimal implementation.

## Development Environment

This project uses Nix flakes for reproducible development environments. Enter the development shell with:

```bash
nix develop
```

The development environment includes:
- Rust toolchain (stable channel, defined in `rust-toolchain.toml`)
- `cargo-watch` for automated rebuilds
- `cargo-outdated` for dependency checking
- `cargo-udeps` for finding unused dependencies
- `rust-analyzer` for IDE support

## Common Commands

### Development Workflow

```bash
# Primary development loop - watches for changes and runs check/test/run
make dev

# Build the project
cargo build

# Run the application
cargo run

# Run tests
cargo test

# Check code without building
cargo check
```

### Dependency Management

```bash
# Check for outdated dependencies
make outdated
# Or: cargo outdated -R

# Find unused dependencies (requires nightly)
make unused
# Or: cargo +nightly udeps

# Update dependencies
make update
# Or: cargo update
```

### Cleanup

```bash
make clean
# Or: cargo clean
```

## Architecture

The project currently has a minimal structure:
- Single source file: `src/main.rs` (basic "Hello, world!" placeholder)
- Dependencies: `ratatui` (v0.29.0) for TUI, `watchexec` (v8.0.1) for file watching

The architecture is not yet defined as the project is in initial setup phase.

## Build Configuration

- **Rust Edition**: 2021
- **Toolchain**: Stable channel with `rust-src` and `rust-analyzer` components
- **Package Name**: Currently set to "project-name" (placeholder in Cargo.toml:2)
