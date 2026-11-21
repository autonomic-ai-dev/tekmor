#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$DIR/.." && pwd)"
cd "$REPO_ROOT"

echo "==> [tekmor] Starting verification suite..."

echo "==> cargo clippy (CI gate)"
cargo clippy -p tekmor --all-targets -- -D warnings

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
bash .verify/checks/blackbox.sh

echo "==> harness trap tasks are valid"
python3 -B harness/bench/validate_traps.py

echo "==> tekmor tests"
cargo test -p tekmor -- --test-threads=1

echo "==> [tekmor] Verification completed successfully (exit 0)."
