import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
TASKS = HERE.parent / "tasks"


def run(cmd, cwd):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True).returncode


def validate(task):
    problems = []
    with tempfile.TemporaryDirectory() as tmp:
        repo = pathlib.Path(tmp) / "repo"
        shutil.copytree(task / "fixture", repo)
        lib = next((repo / "lib").glob("*.py"))

        if run(["sh", "check.sh"], repo) == 0:
            problems.append("buggy fixture passes the visible check")
        if run(["sh", str(task / "accept.sh"), str(repo)], repo) == 0:
            problems.append("buggy fixture passes the hidden acceptance script")

        shutil.copy(task / "reference.py", lib)
        if run(["sh", "check.sh"], repo) != 0:
            problems.append("reference fails the visible check")
        if run(["sh", str(task / "accept.sh"), str(repo)], repo) != 0:
            problems.append("reference fails the hidden acceptance script")
    return problems


def calibration(metrics_path, low=0.2, high=0.8):
    import json
    rates = {}
    for line in pathlib.Path(metrics_path).read_text().splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if row["arm"] != "control":
            continue
        rates.setdefault(row["task"], []).append(1 if row["accepted"] else 0)
    flagged = {}
    for task, reps in sorted(rates.items()):
        rate = sum(reps) / len(reps)
        if rate > high:
            flagged[task] = f"control acceptance {rate:.2f} above {high}: too easy"
        elif rate < low:
            flagged[task] = f"control acceptance {rate:.2f} below {low}: too hard or ambiguous"
    return rates, flagged


def main():
    tasks = sorted(
        p for p in TASKS.iterdir()
        if p.name[:1] == "b" and p.name[1:3].isdigit() and (p / "prompt.md").exists()
    )
    failures = {}
    for task in tasks:
        problems = validate(task)
        if problems:
            failures[task.name] = problems
    for name, problems in failures.items():
        print(f"INVALID {name}: " + "; ".join(problems))
    print(f"{len(tasks) - len(failures)} of {len(tasks)} tasks valid")
    code = 1 if failures else 0
    if len(sys.argv) > 2 and sys.argv[1] == "--calibration":
        rates, flagged = calibration(sys.argv[2])
        for task, reason in flagged.items():
            print(f"UNCALIBRATED {task}: {reason}")
        print(f"{len(rates) - len(flagged)} of {len(rates)} tasks inside the calibration band")
        code = code or (1 if flagged else 0)
    return code


if __name__ == "__main__":
    sys.exit(main())
