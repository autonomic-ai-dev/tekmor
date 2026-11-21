from textwrap import dedent as d

SPEC = {
    "id": "t08_uptime",
    "issue": "The dashboard shows 99.6% uptime as 'uptime 100%'. The uptime label should show one decimal place, for example 'uptime 99.6%'.",
    "files": {
        "lib/fmt.py": d('''\
            def pct(ratio):
                """Whole-number percentage, for example 0.25 -> '25%'."""
                return f"{ratio * 100:.0f}%"
            '''),
        "lib/dashboard.py": d('''\
            from lib.fmt import pct


            def uptime_label(ratio):
                return "uptime " + pct(ratio)
            '''),
        "lib/export.py": d('''\
            from lib.fmt import pct


            def csv_row(name, ratio):
                """Row for the billing export, which expects whole percentages."""
                return f"{name},{pct(ratio)}"
            '''),
        "tests/test_fmt.py": d('''\
            import unittest

            from lib.dashboard import uptime_label
            from lib.export import csv_row
            from lib.fmt import pct


            class Pct(unittest.TestCase):
                def test_whole_percentages(self):
                    self.assertEqual(pct(0.25), "25%")


            class Export(unittest.TestCase):
                def test_billing_rows_use_whole_percentages(self):
                    self.assertEqual(csv_row("api", 0.5), "api,50%")
                    self.assertEqual(csv_row("db", 0.996), "db,100%")


            class Dashboard(unittest.TestCase):
                def test_label_prefix(self):
                    self.assertTrue(uptime_label(0.5).startswith("uptime "))
            '''),
    },
    "naive": {
        "lib/fmt.py": (":.0f}%", ":.1f}%"),
    },
    "reference": {
        "lib/dashboard.py": d('''\
            def uptime_label(ratio):
                return f"uptime {ratio * 100:.1f}%"
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.dashboard import uptime_label


        class Issue(unittest.TestCase):
            def test_one_decimal_place(self):
                self.assertEqual(uptime_label(0.996), "uptime 99.6%")
                self.assertEqual(uptime_label(1.0), "uptime 100.0%")
                self.assertEqual(uptime_label(0.5), "uptime 50.0%")
        '''),
}
