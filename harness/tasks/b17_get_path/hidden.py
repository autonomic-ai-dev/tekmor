import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(({'a': 1}, 'a'), 1), (({'a': {'b': {'c': 2}}}, 'a.b.c'), 2), (({'a': 1}, 'a.b', 'x'), 'x'), (({}, 'z', 5), 5), (({'a': 0}, 'a.b', 9), 9)]


def main(root):
    fn = getattr(load(root), "get_path")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
