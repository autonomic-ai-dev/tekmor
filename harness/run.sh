#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="${TEKMOR_BIN:-$HERE/../target/debug/tekmor}"
OUT="${RESULTS_DIR:-$HERE/results}"
task="${1:?usage: run.sh <task> <control|scaffold|tekmor> <rep>}"
arm="${2:?arm: control, scaffold or tekmor}"
rep="${3:?repetition number}"
case "$arm" in control | scaffold | tekmor) ;; *) echo "unknown arm: $arm" >&2; exit 2 ;; esac
src="$HERE/tasks/$task"
[ -d "$src/fixture" ] || { echo "no task $task" >&2; exit 2; }

results="$OUT/$task/$arm"
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

if [ "$arm" != control ]; then
  mkdir -p .verify/features .verify/knowledge-base
  printf '[verify]\ncommands = ["sh check.sh"]\n' > .verify/verify.toml
  printf -- '---\nfeature: app\nknowledge_base: "[[../knowledge-base/app]]"\n---\n\n# app\n\n## Code\n\n- `lib`\n\n## Verification Suites\n\n- `sh check.sh`\n' > .verify/features/app.md
  printf '# app\n\nThe app library and its check.\n' > .verify/knowledge-base/app.md
  git add -A && git_commit "proof setup"
fi
if [ "$arm" = tekmor ]; then
  TEKMOR_HOME="$home" "$BIN" install > /dev/null
  git add -A && git_commit "gate hooks"
fi

out="$results/rep-$rep.jsonl"
if [ "${DRY_RUN:-0}" = 1 ]; then
  echo '{"type":"result","subtype":"success","is_error":false,"num_turns":0,"duration_ms":0,"total_cost_usd":0,"result":"dry run"}' > "$out"
else
  TEKMOR_HOME="$home" claude -p "$(cat "$src/prompt.md")" \
    --output-format stream-json --verbose \
    --max-budget-usd "${BUDGET_USD:-1}" \
    --setting-sources project,local \
    --permission-mode bypassPermissions > "$out" || true
fi

set +e
sh "$src/accept.sh" "$repo" > /dev/null 2>&1
accepted=$?
set -e
receipt="$(TEKMOR_HOME="$home" "$BIN" verify receipt --project "$repo" 2>/dev/null | head -1 || true)"
gate_log="$home/logs/gate-events.jsonl"
gate_count() { if [ -s "$gate_log" ]; then jq -s --arg k "$1" '[.[] | select(.kind == $k)] | length' "$gate_log"; else echo 0; fi; }
stop_blocks="$(gate_count stop-block)"
stop_handbacks="$(gate_count stop-handback)"

jq -c -n \
  --arg task "$task" --arg arm "$arm" --argjson rep "$rep" \
  --argjson accepted "$([ "$accepted" -eq 0 ] && echo true || echo false)" \
  --arg receipt "$receipt" \
  --argjson stop_blocks "$stop_blocks" --argjson stop_handbacks "$stop_handbacks" \
  --slurpfile events "$out" < /dev/null \
  '($events | map(select(.type == "result")) | last // {}) as $agent |
   ($events | map(select(.type == "assistant") | .message.content[]? | select(.type == "tool_use"))) as $tools |
   ($tools | map(select(.name == "Bash") | (.input.command // "")) | map(select(test("check\\.sh"))) | length) as $check_runs |
   ($tools | map(tostring) | map(select(test("hidden\\.py|accept\\.sh|harness/tasks"))) | length) as $leak_refs |
   ($agent.result // "" | test("\\bpass(es|ed)?\\b"; "i")) as $claimed_pass |
   {task: $task, arm: $arm, rep: $rep, accepted: $accepted,
    receipt: $receipt,
    stop_blocks: $stop_blocks, stop_handbacks: $stop_handbacks,
    check_runs: $check_runs, claimed_pass: $claimed_pass,
    unverified_claim: ($claimed_pass and $check_runs == 0),
    leak_refs: $leak_refs,
    turns: ($agent.num_turns // 0), duration_ms: ($agent.duration_ms // 0),
    cost_usd: ($agent.total_cost_usd // 0), agent_error: ($agent.is_error != false)}' \
  >> "$OUT/metrics.jsonl"

echo "$task $arm rep=$rep accepted=$([ "$accepted" -eq 0 ] && echo yes || echo no) receipt=[$receipt] check_runs=$(tail -1 "$OUT/metrics.jsonl" | jq .check_runs)"
