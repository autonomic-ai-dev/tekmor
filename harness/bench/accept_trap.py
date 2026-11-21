import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

TIMEOUT_SECONDS = 60


def run_suite(lib, tests, extra=None):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        if not lib.is_dir():
            return 1, "no lib/ directory"
        shutil.copytree(lib, root / "lib", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        if tests is None:
            (root / "tests").mkdir()
            (root / "tests" / "__init__.py").write_text("")
        else:
            shutil.copytree(tests, root / "tests", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        if extra is not None:
            shutil.copy(extra, root / "tests" / "test_hidden_issue.py")
        env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
        try:
            result = subprocess.run(
                [sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests", "-t", "."],
                cwd=root,
                capture_output=True,
                text=True,
                env=env,
                timeout=TIMEOUT_SECONDS,
            )
        except subprocess.TimeoutExpired:
            return 1, f"timed out after {TIMEOUT_SECONDS}s"
        lines = [line for line in result.stderr.splitlines() if line.strip()]
        return result.returncode, lines[-1] if lines else ""


def accept(task, repo):
    return run_suite(repo / "lib", task / "fixture" / "tests", task / "hidden.py")


def main():
    task, repo = pathlib.Path(sys.argv[1]).resolve(), pathlib.Path(sys.argv[2]).resolve()
    code, last = accept(task, repo)
    print(("accepted: " if code == 0 else "rejected: ") + last)
    return 0 if code == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
