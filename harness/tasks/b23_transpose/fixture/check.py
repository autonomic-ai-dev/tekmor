import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(([[1, 2], [3, 4]],), [[1, 3], [2, 4]]), (([[1, 2, 3]],), [[1], [2], [3]])]


def main():
    here = pathlib.Path(__file__).resolve().parent
    fn = getattr(load(here), "transpose")
    failed = []
    for args, expected in CASES:
        got = run(fn, args)
        if got != expected:
            failed.append(f"transpose{args!r} returned {got!r}, expected {expected!r}")
    if failed:
        print("FAIL: %d of %d visible checks failed" % (len(failed), len(CASES)))
        for line in failed:
            print("  " + line)
        return 1
    print("PASS: %d of %d visible checks passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
