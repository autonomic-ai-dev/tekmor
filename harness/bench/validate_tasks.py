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


def main():
    failures = {}
    tasks = sorted(
        p for p in TASKS.iterdir()
        if p.name[:1] == "b" and p.name[1:3].isdigit() and (p / "prompt.md").exists()
    )
    for task in tasks:
        problems = validate(task)
        if problems:
            failures[task.name] = problems
    for name, problems in failures.items():
        print(f"INVALID {name}: " + "; ".join(problems))
    print(f"{len(tasks) - len(failures)} of {len(tasks)} tasks valid")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
