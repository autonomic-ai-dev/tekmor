import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(('',), []), (('7',), [7]), (('4-6, 9',), [4, 5, 6, 9]), (('1-1,3-4',), [1, 3, 4]), (('10-12',), [10, 11, 12])]


def main(root):
    fn = getattr(load(root), "parse_ranges")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
