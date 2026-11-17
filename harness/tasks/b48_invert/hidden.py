import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(({},), {}), (({'x': 2},), {2: ['x']}), (({'a': 1, 'b': 2, 'c': 1},), {1: ['a', 'c'], 2: ['b']}), (({'p': 0, 'q': 0, 'r': 0},), {0: ['p', 'q', 'r']})]


def main(root):
    fn = getattr(load(root), "invert")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
