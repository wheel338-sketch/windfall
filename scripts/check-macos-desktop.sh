#!/usr/bin/env bash
# Check-only foreign-target Rust validation, never a link/build workaround.
# objc2-exception-helper 0.1.1 explicitly supports DOCS_RS for a no-Clang
# non-linking check. Keep this variable out of ordinary builds and Cargo config.
set -euo pipefail
cd "$(dirname "$0")/.."
CARGO_TARGET_DIR=target/macos-desktop-typecheck DOCS_RS=1 cargo check --offline --target aarch64-apple-darwin -p windfall-desktop --all-targets "$@"
