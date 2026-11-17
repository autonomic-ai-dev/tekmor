import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [((0, 0, 10, 100, 0), 0.0), ((0, 0, 10, 100, 10), 100.0), ((2, 4, 4, 8, 3), 6.0), ((0, 5, 2, 5, 1), 5.0), ((1, 1, 3, 1, 2), 1.0)]


def main(root):
    fn = getattr(load(root), "lerp")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
