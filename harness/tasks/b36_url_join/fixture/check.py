import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(('http://x.com/', '/a'), 'http://x.com/a'), (('http://x.com', 'b'), 'http://x.com/b')]


def main():
    here = pathlib.Path(__file__).resolve().parent
    fn = getattr(load(here), "url_join")
    failed = []
    for args, expected in CASES:
        got = run(fn, args)
        if got != expected:
            failed.append(f"url_join{args!r} returned {got!r}, expected {expected!r}")
    if failed:
        print("FAIL: %d of %d visible checks failed" % (len(failed), len(CASES)))
        for line in failed:
            print("  " + line)
        return 1
    print("PASS: %d of %d visible checks passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
