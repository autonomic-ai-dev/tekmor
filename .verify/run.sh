#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$DIR/.." && pwd)"
cd "$REPO_ROOT"

echo "==> [tekmor] Starting verification suite..."

echo "==> cargo check"
cargo check -p tekmor

echo "==> tekmor-core boundary"
.verify/checks/core_boundary.sh

echo "==> tekmor-core unit tests"
cargo test -p tekmor-core

cargo build -q -p tekmor

echo "==> feature maps match the code and the allowlist"
./target/debug/tekmor verify drift

echo "==> lint ratchet"
./target/debug/tekmor verify ratchet

echo "==> black-box check of the built binary"
sh .verify/checks/blackbox.sh

echo "==> tekmor tests"
cargo test -p tekmor -- --test-threads=1

echo "==> [tekmor] Verification completed successfully (exit 0)."
