#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="${TEKMOR_BIN:-$HERE/../target/debug/tekmor}"
task="${1:?usage: run.sh <task> <control|tekmor> <rep>}"
arm="${2:?arm: control or tekmor}"
rep="${3:?repetition number}"
case "$arm" in control | tekmor) ;; *) echo "unknown arm: $arm" >&2; exit 2 ;; esac
src="$HERE/tasks/$task"
[ -d "$src/fixture" ] || { echo "no task $task" >&2; exit 2; }

results="$HERE/results/$task/$arm"
mkdir -p "$results"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
home="$work/home"; repo="$work/repo"
mkdir -p "$home" "$repo"
cp -R "$src/fixture/." "$repo/"
git_commit() { git -c user.name=harness -c user.email=harness@local commit -q -m "$1"; }

cd "$repo"
git init -q
git add -A && git_commit baseline

if [ "$arm" = tekmor ]; then
  mkdir -p .verify/features .verify/knowledge-base
  printf '[verify]\ncommands = ["sh check.sh"]\n' > .verify/verify.toml
  printf -- '---\nfeature: app\nknowledge_base: "[[../knowledge-base/app]]"\n---\n\n# app\n\n## Code\n\n- `lib`\n\n## Verification Suites\n\n- `sh check.sh`\n' > .verify/features/app.md
  printf '# app\n\nThe app library and its check.\n' > .verify/knowledge-base/app.md
  git add -A && git_commit "proof setup"
  TEKMOR_HOME="$home" "$BIN" install > /dev/null
  git add -A && git_commit "gate hooks"
fi

out="$results/rep-$rep.json"
if [ "${DRY_RUN:-0}" = 1 ]; then
  echo '{"type":"result","subtype":"success","is_error":false,"num_turns":0,"duration_ms":0,"total_cost_usd":0,"result":"dry run"}' > "$out"
else
  TEKMOR_HOME="$home" claude -p "$(cat "$src/prompt.md")" \
    --output-format json \
    --max-budget-usd "${BUDGET_USD:-1}" \
    --permission-mode bypassPermissions > "$out" || true
fi

set +e
sh "$src/accept.sh" "$repo" > /dev/null 2>&1
accepted=$?
set -e
receipt="$(TEKMOR_HOME="$home" "$BIN" verify receipt --project "$repo" 2>/dev/null | head -1 || true)"
stop_events="$(grep -c 'stop' "$home/logs/gate-events.jsonl" 2>/dev/null || echo 0)"

jq -c -n \
  --arg task "$task" --arg arm "$arm" --argjson rep "$rep" \
  --argjson accepted "$([ "$accepted" -eq 0 ] && echo true || echo false)" \
  --arg receipt "$receipt" --argjson stop_events "$stop_events" \
  --slurpfile agent "$out" \
  '{task: $task, arm: $arm, rep: $rep, accepted: $accepted,
    receipt: $receipt, stop_events: $stop_events,
    turns: ($agent[0].num_turns // 0), duration_ms: ($agent[0].duration_ms // 0),
    cost_usd: ($agent[0].total_cost_usd // 0), agent_error: ($agent[0].is_error != false)}' \
  >> "$HERE/results/metrics.jsonl"

echo "$task $arm rep=$rep accepted=$([ "$accepted" -eq 0 ] && echo yes || echo no) receipt=[$receipt]"
