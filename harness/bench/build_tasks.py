import pathlib
import shutil
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from specs import SPECS  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
TASKS = HERE.parent / "tasks"

PROMPT = (
    "Issue: {issue}\n\n"
    "The code is in lib/{module}.py. Fix it so that `sh check.sh` passes. "
    "Keep the change minimal and do not edit check.sh or check.py. "
    "When you finish, run `sh check.sh` and state its exact final output.\n"
)

LOADER = '''import importlib.util
import pathlib
import sys

sys.dont_write_bytecode = True


def load(root):
    path = pathlib.Path(root) / "lib" / "{module}.py"
    spec = importlib.util.spec_from_file_location("{module}", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def run(fn, args):
    try:
        return fn(*args)
    except Exception as exc:
        return ("raises", type(exc).__name__)
'''

CHECK_PY = '''import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = {cases}


def main():
    here = pathlib.Path(__file__).resolve().parent
    fn = getattr(load(here), "{func}")
    failed = []
    for args, expected in CASES:
        got = run(fn, args)
        if got != expected:
            failed.append(f"{func}{{args!r}} returned {{got!r}}, expected {{expected!r}}")
    if failed:
        print("FAIL: %d of %d visible checks failed" % (len(failed), len(CASES)))
        for line in failed:
            print("  " + line)
        return 1
    print("PASS: %d of %d visible checks passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
'''

HIDDEN_PY = '''import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = {cases}


def main(root):
    fn = getattr(load(root), "{func}")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
'''

CHECK_SH = "python3 -I check.py\n"
ACCEPT_SH = 'python3 -I "$(dirname "$0")/hidden.py" "$1"\n'


def expected_for(reference_fn, args):
    try:
        return reference_fn(*args)
    except Exception as exc:
        return ("raises", type(exc).__name__)


def build_cases(spec, reference_fn, arg_list):
    return [(args, expected_for(reference_fn, args)) for args in arg_list]


def build(spec):
    ns = {}
    exec(spec["correct"], ns)
    reference_fn = ns[spec["func"]]
    vis = build_cases(spec, reference_fn, spec["vis"])
    hid = build_cases(spec, reference_fn, spec["hid"])

    old, new = spec["bug"]
    assert spec["correct"].count(old) >= 1, f"{spec['id']}: bug text not found"
    buggy = spec["correct"].replace(old, new, 1)
    assert buggy != spec["correct"], f"{spec['id']}: bug did not change the module"

    task = TASKS / spec["id"]
    if task.exists():
        shutil.rmtree(task)
    (task / "fixture" / "lib").mkdir(parents=True)
    (task / "fixture" / "lib" / f"{spec['module']}.py").write_text(buggy)
    (task / "fixture" / "check.sh").write_text(CHECK_SH)
    (task / "fixture" / "check_support.py").write_text(LOADER.format(module=spec["module"]))
    (task / "fixture" / "check.py").write_text(
        CHECK_PY.format(cases=repr(vis), func=spec["func"])
    )
    (task / "prompt.md").write_text(
        PROMPT.format(issue=spec["issue"], module=spec["module"])
    )
    (task / "check_support.py").write_text(LOADER.format(module=spec["module"]))
    (task / "hidden.py").write_text(
        HIDDEN_PY.format(cases=repr(hid), func=spec["func"])
    )
    (task / "accept.sh").write_text(ACCEPT_SH)
    (task / "reference.py").write_text(spec["correct"])
    return task


def main():
    ids = [s["id"] for s in SPECS]
    assert len(ids) == len(set(ids)), "duplicate task ids"
    assert len(SPECS) >= 50, f"need 50+ tasks, have {len(SPECS)}"
    for spec in SPECS:
        build(spec)
    print(f"wrote {len(SPECS)} tasks to {TASKS}")


if __name__ == "__main__":
    main()
