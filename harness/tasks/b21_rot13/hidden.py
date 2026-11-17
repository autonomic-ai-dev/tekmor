import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(('',), ''), (('ABC',), 'NOP'), (('Hello, World!',), 'Uryyb, Jbeyq!'), (('nop',), 'abc'), (('zZ',), 'mM')]


def main(root):
    fn = getattr(load(root), "rot13")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
