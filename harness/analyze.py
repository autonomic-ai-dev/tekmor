import json
import pathlib
import random
import statistics
import sys
from collections import defaultdict

BOOTSTRAPS = 5000
SEED = 20261117
ARMS = ("control", "scaffold", "tekmor")
COMPARISONS = (("tekmor", "control"), ("tekmor", "scaffold"), ("scaffold", "control"))


def load(folder):
    path = pathlib.Path(folder) / "metrics.jsonl"
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if not rows:
        sys.exit(f"no runs in {path}")
    return rows


def by_task_arm(rows):
    table = defaultdict(lambda: defaultdict(list))
    for row in rows:
        table[row["task"]][row["arm"]].append(1 if row["accepted"] else 0)
    return table


def task_diff(arms, a, b):
    x, y = arms.get(a, []), arms.get(b, [])
    if not x or not y:
        return None
    return sum(x) / len(x) - sum(y) / len(y)


def pooled(tasks_arms, a, b, rng=None):
    diffs = []
    for arms in tasks_arms:
        if rng is not None:
            arms = {arm: [rng.choice(reps) for _ in reps] for arm, reps in arms.items()}
        d = task_diff(arms, a, b)
        if d is not None:
            diffs.append(d)
    return statistics.fmean(diffs) if diffs else None


def bootstrap(table, a, b):
    rng = random.Random(SEED)
    tasks = list(table)
    draws = []
    for _ in range(BOOTSTRAPS):
        sample = [table[t] for t in rng.choices(tasks, k=len(tasks))]
        value = pooled(sample, a, b, rng)
        if value is not None:
            draws.append(value)
    draws.sort()
    return draws[int(0.025 * len(draws))], draws[int(0.975 * len(draws)) - 1]


def verdict(lo, hi, a, b):
    if lo > 0:
        return f"{a} accepts more than {b} (interval above zero)"
    if hi < 0:
        return f"{a} accepts less than {b} (interval below zero)"
    return f"no detectable difference between {a} and {b} at this sample size"


def mechanism(rows, arm):
    mine = [r for r in rows if r["arm"] == arm]
    n = len(mine)
    if n == 0:
        return None
    blocked = [r for r in mine if r["stop_blocks"] + r["stop_handbacks"] > 0]
    return {
        "runs": n,
        "accepted": sum(r["accepted"] for r in mine),
        "median_check_runs": statistics.median(r["check_runs"] for r in mine),
        "claimed_pass": sum(r["claimed_pass"] for r in mine),
        "unverified_claims": sum(r["unverified_claim"] for r in mine),
        "runs_with_gate_block": len(blocked),
        "gate_blocks_in_accepted_runs": sum(
            1 for r in blocked if r["accepted"]
        ),
        "passed_receipts": sum("PASSED" in r["receipt"] for r in mine),
        "leak_refs": sum(r["leak_refs"] for r in mine),
        "agent_errors": sum(r["agent_error"] for r in mine),
        "median_turns": statistics.median(r["turns"] for r in mine),
        "median_cost": statistics.median(r["cost_usd"] for r in mine),
        "total_cost": sum(r["cost_usd"] for r in mine),
    }


def main():
    all_rows = load(sys.argv[1])
    rows = [r for r in all_rows if r["leak_refs"] == 0]
    table = by_task_arm(rows)
    tasks = sorted(table)
    print(f"runs: {len(all_rows)}   excluded for hidden-file references: {len(all_rows) - len(rows)}   analysed: {len(rows)}   tasks: {len(tasks)}")
    print("effect = mean over tasks of (accepted rate in A minus accepted rate in B), 95% cluster bootstrap")
    for a, b in COMPARISONS:
        effect = pooled(list(table.values()), a, b)
        if effect is None:
            continue
        lo, hi = bootstrap(table, a, b)
        print(f"  {a} - {b}: {effect:+.3f}  [{lo:+.3f}, {hi:+.3f}]  -> {verdict(lo, hi, a, b)}")
    print()
    print("mechanism (per arm, all runs)")
    for arm in ARMS:
        m = mechanism(all_rows, arm)
        if m is None:
            continue
        print(
            f"  {arm:9s} runs={m['runs']} accepted={m['accepted']} "
            f"median_check_runs={m['median_check_runs']} claimed_pass={m['claimed_pass']} "
            f"unverified_claims={m['unverified_claims']} runs_with_gate_block={m['runs_with_gate_block']} "
            f"blocks_in_accepted_runs={m['gate_blocks_in_accepted_runs']} PASSED_receipts={m['passed_receipts']} "
            f"leak_refs={m['leak_refs']} agent_errors={m['agent_errors']}"
        )
        print(
            f"  {'':9s} median_turns={m['median_turns']} median_cost={m['median_cost']:.4f} "
            f"total_cost=${m['total_cost']:.2f}"
        )
    print()
    header = f"{'task':18s}" + "".join(f"{arm:>10s}" for arm in ARMS)
    print(header)
    for task in tasks:
        cells = []
        for arm in ARMS:
            reps = table[task].get(arm, [])
            cells.append(f"{sum(reps) / len(reps):10.2f}" if reps else f"{'-':>10s}")
        print(f"{task:18s}" + "".join(cells))


if __name__ == "__main__":
    main()
