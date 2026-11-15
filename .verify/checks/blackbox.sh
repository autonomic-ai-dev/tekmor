#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$DIR/../.." && pwd)"
cargo build -q -p tekmor --manifest-path "$REPO_ROOT/Cargo.toml"
BIN="$REPO_ROOT/target/debug/tekmor"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export TEKMOR_HOME="$work/home"
repo="$work/repo"
mkdir -p "$repo/.verify" "$TEKMOR_HOME"
cd "$repo"
git init -q
git -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
printf '[verify]\ncommands = ["true", "false"]\n' > .verify/verify.toml
printf 'one\n' > a.txt
git add -A
git -c user.name=t -c user.email=t@t commit -q -m a

fail() {
  echo "blackbox FAILED: $1" >&2
  exit 1
}

expect_exit() {
  local want="$1" label="$2"
  shift 2
  set +e
  "$@" > "$work/out" 2>&1
  local got=$?
  set -e
  [ "$got" -eq "$want" ] || fail "$label: exit $got, want $want ($(head -c 300 "$work/out"))"
}

expect_text() {
  grep -q "$1" "$work/out" || fail "$2: missing '$1' in: $(head -c 300 "$work/out")"
}

expect_exit 1 "receipt before any run" "$BIN" verify receipt
expect_text "NOT VERIFIED" "receipt before any run"

expect_exit 0 "passing run" "$BIN" verify -- true
expect_exit 0 "receipt after passing run" "$BIN" verify receipt
expect_text "PASSED" "receipt after passing run"

expect_exit 1 "failing run" "$BIN" verify -- false
expect_exit 1 "receipt after failing run" "$BIN" verify receipt
expect_text "FAILED" "receipt after failing run"

printf 'two\n' >> a.txt
expect_exit 1 "receipt after an edit" "$BIN" verify receipt
expect_text "NOT VERIFIED" "receipt after an edit"

printf '[verify]\ncommands = ["true", "false"]\n' > .verify/verify.toml
expect_exit 1 "receipt after an allowlist edit" "$BIN" verify receipt
expect_text "NOT VERIFIED" "receipt after an allowlist edit"

expect_exit 0 "drift on a fresh repo" "$BIN" verify drift

printf 'let x = y.unwrap();\n' > b.rs
printf "[[rule]]\nname = \"no-unwrap\"\npattern = '\\.unwrap\\(\\)'\nbaseline = 0\n" > .verify/ratchet.toml
git add -A
expect_exit 1 "ratchet over baseline" "$BIN" verify ratchet
expect_text "over baseline" "ratchet over baseline"

echo "blackbox passed"
