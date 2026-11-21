import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
TASKS = HERE.parent / "tasks"
sys.path.insert(0, str(HERE))
from accept_trap import run_suite  # noqa: E402
from build_traps import main as build_all  # noqa: E402


def visible_passes(repo):
    return subprocess.run(["sh", "check.sh"], cwd=repo, capture_output=True, text=True).returncode == 0


def accepted(task, repo):
    return subprocess.run(["sh", str(task / "accept.sh"), str(repo)], capture_output=True, text=True).returncode == 0


def issue_fixed(task, repo):
    return run_suite(repo / "lib", None, task / "hidden.py")[0] == 0


def variant(task, tmp, name):
    repo = pathlib.Path(tmp) / name
    shutil.copytree(task / "fixture", repo)
    if name != "buggy":
        shutil.copytree(task / name, repo, dirs_exist_ok=True)
    return repo


def validate(task):
    problems = []
    with tempfile.TemporaryDirectory() as tmp:
        buggy = variant(task, tmp, "buggy")
        naive = variant(task, tmp, "naive")
        reference = variant(task, tmp, "reference")
        if not visible_passes(buggy):
            problems.append("buggy fixture fails the visible suite (the bug must be invisible to it)")
        if issue_fixed(task, buggy):
            problems.append("buggy fixture passes the issue cases")
        if accepted(task, buggy):
            problems.append("buggy fixture is accepted")
        if not issue_fixed(task, naive):
            problems.append("naive fix does not fix the reported issue")
        if visible_passes(naive):
            problems.append("naive fix passes the visible suite (no regression for a check to catch)")
        if accepted(task, naive):
            problems.append("naive fix is accepted")
        if not visible_passes(reference):
            problems.append("reference fails the visible suite")
        if not accepted(task, reference):
            problems.append("reference is not accepted")
    return problems


def main():
    build_all()
    tasks = sorted(p for p in TASKS.iterdir() if p.name[:1] == "t" and p.name[1:3].isdigit())
    failures = {task.name: problems for task in tasks if (problems := validate(task))}
    for name, problems in failures.items():
        print(f"INVALID {name}: " + "; ".join(problems))
    print(f"{len(tasks) - len(failures)} of {len(tasks)} trap tasks valid")
    return 1 if failures or not tasks else 0


if __name__ == "__main__":
    sys.exit(main())
