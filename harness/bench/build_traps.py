import pathlib
import shutil
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from traps import TRAPS  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
TASKS = HERE.parent / "tasks"

PROMPT = "Issue: {issue}\n\nThe code is in lib/. Fix the issue and keep the change minimal.\n"

CHECK_SH = (
    "#!/bin/sh\n"
    "cd \"$(dirname \"$0\")\" || exit 1\n"
    "if python3 -B -m unittest discover -s tests -t . 2>&1; then echo PASS; else echo FAIL; exit 1; fi\n"
)

ACCEPT_SH = 'exec python3 -B "$(dirname "$0")/../../bench/accept_trap.py" "$(dirname "$0")" "$1"\n'


def patched(files, patch):
    out = {}
    for path, change in patch.items():
        if isinstance(change, tuple):
            old, new = change
            if files[path].count(old) != 1:
                raise SystemExit(f"patch for {path} must match exactly once: {old!r}")
            out[path] = files[path].replace(old, new)
        else:
            out[path] = change
    return out


def write_tree(root, files):
    for path, text in files.items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)


def build(spec):
    task = TASKS / spec["id"]
    if task.exists():
        shutil.rmtree(task)
    files = {"lib/__init__.py": "", "tests/__init__.py": "", "check.sh": CHECK_SH, **spec["files"]}
    write_tree(task / "fixture", files)
    write_tree(task / "naive", patched(files, spec["naive"]))
    write_tree(task / "reference", patched(files, spec["reference"]))
    (task / "hidden.py").write_text(spec["hidden"])
    (task / "accept.sh").write_text(ACCEPT_SH)
    (task / "prompt.md").write_text(PROMPT.format(issue=spec["issue"]))


def main():
    ids = [spec["id"] for spec in TRAPS]
    if len(ids) != len(set(ids)):
        raise SystemExit("duplicate trap ids")
    for spec in TRAPS:
        build(spec)
    print(f"built {len(TRAPS)} trap tasks in {TASKS}")


if __name__ == "__main__":
    main()
