import json
import pathlib
import random
import statistics
import sys
from collections import defaultdict

BOOTSTRAPS = 5000
SEED = 20261117


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


def task_diff(arms):
    c, t = arms.get("control", []), arms.get("tekmor", [])
    if not c or not t:
        return None
    return sum(t) / len(t) - sum(c) / len(c)


def pooled(table, rng, resample):
    diffs = []
    for task in table:
        arms = table[task]
        if resample:
            arms = {
                arm: [rng.choice(reps) for _ in reps] for arm, reps in arms.items()
            }
        d = task_diff(arms)
        if d is not None:
            diffs.append(d)
    return statistics.fmean(diffs) if diffs else None


def bootstrap(table):
    rng = random.Random(SEED)
    tasks = list(table)
    draws = []
    for _ in range(BOOTSTRAPS):
        sample = {}
        for i, task in enumerate(rng.choices(tasks, k=len(tasks))):
            sample[f"{task}#{i}"] = table[task]
        value = pooled(sample, rng, resample=True)
        if value is not None:
            draws.append(value)
    draws.sort()
    lo = draws[int(0.025 * len(draws))]
    hi = draws[int(0.975 * len(draws)) - 1]
    return lo, hi


def arm_summary(rows, arm):
    mine = [r for r in rows if r["arm"] == arm]
    n = len(mine)
    return {
        "runs": n,
        "accepted": sum(1 for r in mine if r["accepted"]),
        "passed_receipts": sum(1 for r in mine if "PASSED" in r["receipt"]),
        "agent_errors": sum(1 for r in mine if r["agent_error"]),
        "median_turns": statistics.median(r["turns"] for r in mine) if mine else None,
        "median_cost": statistics.median(r["cost_usd"] for r in mine) if mine else None,
        "total_cost": sum(r["cost_usd"] for r in mine),
        "median_stop_events": statistics.median(r["stop_events"] for r in mine) if mine else None,
    }


def verdict(lo, hi):
    if lo > 0:
        return "tekmor improves acceptance (CI excludes 0)"
    if hi < 0:
        return "tekmor reduces acceptance (CI excludes 0)"
    return "no detectable difference at this sample size (CI includes 0)"


def main():
    rows = load(sys.argv[1])
    table = by_task_arm(rows)
    effect = pooled(table, None, resample=False)
    lo, hi = bootstrap(table)

    tasks = sorted(table)
    complete = [t for t in tasks if task_diff(table[t]) is not None]
    print(f"runs: {len(rows)}   tasks: {len(tasks)}   tasks with both arms: {len(complete)}")
    print(f"primary: mean per-task difference in accepted rate (tekmor - control)")
    print(f"  effect {effect:+.3f}   95% cluster bootstrap CI [{lo:+.3f}, {hi:+.3f}]")
    print(f"  verdict: {verdict(lo, hi)}")
    print()

    for arm in ("control", "tekmor"):
        s = arm_summary(rows, arm)
        print(
            f"{arm:8s} runs={s['runs']:4d} accepted={s['accepted']:4d} "
            f"PASSED_receipts={s['passed_receipts']:4d} agent_errors={s['agent_errors']:3d} "
            f"median_turns={s['median_turns']} median_cost={s['median_cost']:.4f} "
            f"total_cost=${s['total_cost']:.2f} median_stop_events={s['median_stop_events']}"
        )
    c, t = arm_summary(rows, "control"), arm_summary(rows, "tekmor")
    if c["median_cost"]:
        print(f"median cost ratio (tekmor / control): {t['median_cost'] / c['median_cost']:.2f}")
    print()

    print(f"{'task':18s} {'control':>9s} {'tekmor':>9s} {'diff':>7s}")
    for task in tasks:
        arms = table[task]
        cr = sum(arms.get("control", [])) / max(1, len(arms.get("control", [])))
        tr = sum(arms.get("tekmor", [])) / max(1, len(arms.get("tekmor", [])))
        d = task_diff(arms)
        print(f"{task:18s} {cr:9.2f} {tr:9.2f} {d if d is None else round(d, 2):>7}")


if __name__ == "__main__":
    main()
