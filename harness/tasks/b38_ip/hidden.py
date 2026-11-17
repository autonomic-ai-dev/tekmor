import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [(('0.0.0.0',), 0), (('255.255.255.255',), 4294967295), (('10.0.0.1',), 167772161), (('192.168.1.1',), 3232235777), (('1.2.3.4.5',), ('raises', 'ValueError')), (('256.0.0.0',), ('raises', 'ValueError'))]


def main(root):
    fn = getattr(load(root), "ip_to_int")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
