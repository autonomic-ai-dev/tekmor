import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(([],), []), (([(5, 6)],), [(5, 6)]), (([(1, 4), (0, 4)],), [(0, 4)]), (([(1, 4), (2, 3)],), [(1, 4)]), (([(1, 2), (3, 4)],), [(1, 2), (3, 4)]), (([(1, 2), (2, 3)],), [(1, 3)]), (([(0, 2), (2, 5), (5, 6)],), [(0, 6)])]


def main(root):
    fn = getattr(load(root), "merge_intervals")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
