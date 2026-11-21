#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPS="${REPS:-10}"
PAR="${PAR:-4}"
CEILING_USD="${CEILING_USD:-60}"
export RESULTS_DIR="${RESULTS_DIR:?set RESULTS_DIR, for example results/bench-v1}"
case "$RESULTS_DIR" in /*) ;; *) RESULTS_DIR="$HERE/$RESULTS_DIR" ;; esac
export RESULTS_DIR
mkdir -p "$RESULTS_DIR"

spent() {
  if [ -s "$RESULTS_DIR/metrics.jsonl" ]; then
    jq -s '[.[].cost_usd] | add // 0' "$RESULTS_DIR/metrics.jsonl"
  else
    echo 0
  fi
}

tasks=$(cd "$HERE/tasks" && ls -d ${TASK_GLOB:-b[0-9][0-9]_*} | sort)
echo "tasks: $(echo "$tasks" | wc -l | tr -d ' ')  reps: $REPS  parallel: $PAR  ceiling: \$$CEILING_USD"

for rep in $(seq 1 "$REPS"); do
  total=$(spent)
  if awk -v t="$total" -v c="$CEILING_USD" 'BEGIN { exit !(t >= c) }'; then
    echo "stopped before rep $rep: recorded cost \$$total reached the ceiling \$$CEILING_USD"
    exit 0
  fi
  echo "rep $rep (recorded cost so far \$$total)"
  for task in $tasks; do
    for arm in ${ARMS:-control scaffold tekmor}; do
      echo "$task $arm $rep"
    done
  done | xargs -P "$PAR" -L 1 "$HERE/run.sh" >> "$RESULTS_DIR/run.log" 2>&1 || true
done
echo "done: $(spent) recorded cost, results in $RESULTS_DIR"
