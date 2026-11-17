import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(('1.0.0.0',), 16777216), (('0.0.1.0',), 256)]


def main():
    here = pathlib.Path(__file__).resolve().parent
    fn = getattr(load(here), "ip_to_int")
    failed = []
    for args, expected in CASES:
        got = run(fn, args)
        if got != expected:
            failed.append(f"ip_to_int{args!r} returned {got!r}, expected {expected!r}")
    if failed:
        print("FAIL: %d of %d visible checks failed" % (len(failed), len(CASES)))
        for line in failed:
            print("  " + line)
        return 1
    print("PASS: %d of %d visible checks passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
