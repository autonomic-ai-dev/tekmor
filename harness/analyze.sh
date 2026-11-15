#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
file="$HERE/results/metrics.jsonl"
[ -s "$file" ] || { echo "no runs recorded in $file" >&2; exit 1; }
jq -s -r '
  def median(f): (map(f) | sort) as $v | if ($v|length) == 0 then null
                 else (($v[(($v|length)-1)/2|floor] + $v[(($v|length))/2|floor]) / 2) end;
  group_by([.task, .arm]) | .[] |
  "\(.[0].task) \(.[0].arm) n=\(length) accepted=\((map(select(.accepted))|length))/\(length) " +
  "median_turns=\(median(.turns)) median_cost_usd=\(median(.cost_usd)) median_stop_events=\(median(.stop_events))"
' "$file"
