from textwrap import dedent as d

SPEC = {
    "id": "t06_split_bill",
    "issue": "Splitting a bill loses cents: split(100, 3) returns [33, 33, 33], which adds up to 99. The shares must add up to the total.",
    "files": {
        "lib/money.py": d('''\
            def split(total_cents, ways):
                """Split an amount into `ways` shares.

                Shares differ by at most one cent, and a later share is never
                larger than an earlier one: the first payers cover any rounding.
                """
                if ways < 1:
                    raise ValueError("ways must be >= 1")
                share = total_cents // ways
                return [share] * ways
            '''),
        "lib/bills.py": d('''\
            from lib.money import split


            def shares(bill, people):
                return dict(zip(people, split(bill["total_cents"], len(people))))
            '''),
        "tests/test_money.py": d('''\
            import unittest

            from lib.bills import shares
            from lib.money import split


            class Split(unittest.TestCase):
                def test_even_amounts(self):
                    self.assertEqual(split(90, 3), [30, 30, 30])
                    self.assertEqual(split(0, 2), [0, 0])

                def test_zero_ways_is_rejected(self):
                    with self.assertRaises(ValueError):
                        split(5, 0)

                def test_shares_never_grow_and_differ_by_at_most_one(self):
                    for total in range(61):
                        for ways in range(1, 7):
                            parts = split(total, ways)
                            self.assertTrue(all(a >= b for a, b in zip(parts, parts[1:])), (total, ways, parts))
                            self.assertLessEqual(max(parts) - min(parts), 1)


            class Bills(unittest.TestCase):
                def test_each_person_gets_a_share(self):
                    self.assertEqual(shares({"total_cents": 60}, ["a", "b"]), {"a": 30, "b": 30})
            '''),
    },
    "naive": {
        "lib/money.py": ("    return [share] * ways\n", "    parts = [share] * ways\n    parts[-1] += total_cents - share * ways\n    return parts\n"),
    },
    "reference": {
        "lib/money.py": ("    return [share] * ways\n", "    extra = total_cents % ways\n    return [share + 1] * extra + [share] * (ways - extra)\n"),
    },
    "hidden": d('''\
        import unittest

        from lib.money import split


        class Issue(unittest.TestCase):
            def test_shares_add_up_to_the_total(self):
                for total, ways in [(100, 3), (7, 4), (1, 2), (99, 10)]:
                    self.assertEqual(sum(split(total, ways)), total)
                    self.assertEqual(len(split(total, ways)), ways)
        '''),
}
