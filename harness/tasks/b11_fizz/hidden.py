import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_support import load, run

CASES = [((0,), []), ((1,), ['1']), ((3,), ['1', '2', 'Fizz']), ((16,), ['1', '2', 'Fizz', '4', 'Buzz', 'Fizz', '7', '8', 'Fizz', 'Buzz', '11', 'Fizz', '13', '14', 'FizzBuzz', '16']), ((30,), ['1', '2', 'Fizz', '4', 'Buzz', 'Fizz', '7', '8', 'Fizz', 'Buzz', '11', 'Fizz', '13', '14', 'FizzBuzz', '16', '17', 'Fizz', '19', 'Buzz', 'Fizz', '22', '23', 'Fizz', 'Buzz', '26', 'Fizz', '28', '29', 'FizzBuzz'])]


def main(root):
    fn = getattr(load(root), "fizz")
    bad = sum(1 for args, expected in CASES if run(fn, args) != expected)
    if bad:
        print("rejected: %d of %d acceptance cases failed" % (bad, len(CASES)))
        return 1
    print("accepted: %d of %d acceptance cases passed" % (len(CASES), len(CASES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
