import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(([], 1), -1), (([4], 4), 0), (([4], 5), -1), (([1, 2], 2), 1), (([2, 4, 6, 8, 10], 2), 0)]


def main(root):
    fn = getattr(load(root), "bsearch")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
