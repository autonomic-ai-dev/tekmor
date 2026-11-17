import csv, json, pathlib, sys

rollouts = pathlib.Path(sys.argv[1])
graded_path = pathlib.Path(sys.argv[2])
out = csv.writer(sys.stdout, delimiter="\t", lineterminator="\n")
out.writerow(["task", "arm", "verdict", "turns", "cost_usd", "stop_blocks", "tekmor_verify_calls", "patch_bytes"])
for line in graded_path.read_text().splitlines():
    if not line.strip():
        continue
    task, arm, verdict = line.split()[:3]
    d = rollouts / task / arm / "rep-1"
    records = [json.loads(l) for l in (d / "transcript.jsonl").read_text().splitlines() if l.strip()]
    results = [r for r in records if r.get("type") == "result"]
    turns = sum(r.get("num_turns", 0) for r in results)
    cost = results[-1].get("total_cost_usd", 0) if results else 0
    commands = [c.get("input", {}).get("command", "")
                for r in records if r.get("type") == "assistant"
                for c in (r.get("message", {}).get("content") or [])
                if c.get("type") == "tool_use" and c.get("name") == "Bash"]
    verify = sum("tekmor verify" in c for c in commands)
    gate = d / "gate-events.jsonl"
    blocks = sum(1 for l in gate.read_text().splitlines() if l.strip() and json.loads(l).get("kind") == "stop-block") if gate.exists() else 0
    patch = (d / "patch.diff").stat().st_size
    out.writerow([task, arm, verdict, turns, round(cost, 4), blocks, verify, patch])
