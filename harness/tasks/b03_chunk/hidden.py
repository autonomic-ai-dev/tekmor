import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(([], 3), []), (([1], 5), [[1]]), (([1, 2, 3, 4], 4), [[1, 2, 3, 4]]), (([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], 3), [[0, 1, 2], [3, 4, 5], [6, 7, 8], [9]]), (('abcde', 2), ['ab', 'cd', 'e'])]


def main(root):
    fn = getattr(load(root), "chunk")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
