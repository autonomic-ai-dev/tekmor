#!/bin/sh
set -eu

TEKMOR=${TEKMOR:-tekmor}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export TEKMOR_HOME="$work/home"
mkdir -p "$work/repo"
cd "$work/repo"

git init -q -b main
git config user.email demo@example.invalid
git config user.name demo
mkdir -p .verify
printf '[verify]\ncommands = ["sh check.sh"]\n' > .verify/verify.toml
printf 'echo checks ok\n' > check.sh
printf 'def greet():\n    return "hi"\n' > app.py
git add -A && git commit -q -m baseline

echo "1. Run the allowlisted check:"
"$TEKMOR" verify -- sh check.sh
"$TEKMOR" verify receipt | head -1 || true

echo
echo "2. Edit the code after the pass:"
printf '# edited after the pass\n' >> app.py
"$TEKMOR" verify receipt | head -1 || true

echo
echo "The receipt for the earlier tree no longer applies. Run the check again to get a new one."
