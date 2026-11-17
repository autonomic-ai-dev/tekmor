import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [((5,), ['1', '2', 'Fizz', '4', 'Buzz']), ((15,), ['1', '2', 'Fizz', '4', 'Buzz', 'Fizz', '7', '8', 'Fizz', 'Buzz', '11', 'Fizz', '13', '14', 'FizzBuzz'])]


def main():
    here = pathlib.Path(__file__).resolve().parent
    fn = getattr(load(here), "fizz")
    failed = []
    for args, expected in CASES:
        got = run(fn, args)
        if got != expected:
            failed.append(f"fizz{args!r} returned {got!r}, expected {expected!r}")
    if failed:
        print("FAIL: %d of %d visible checks failed" % (len(failed), len(CASES)))
        for line in failed:
            print("  " + line)
        return 1
    print("PASS: %d of %d visible checks passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
