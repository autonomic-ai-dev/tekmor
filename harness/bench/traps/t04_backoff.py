from textwrap import dedent as d

SPEC = {
    "id": "t04_backoff",
    "issue": "HTTP retries wait twice as long as configured: with base=1 the first retry waits 2 seconds. retry_delays(3) should be [1, 2, 4].",
    "files": {
        "lib/timing.py": d('''\
            def backoff(attempt, base=1.0):
                """Delay before retry number `attempt`, counting from 0."""
                if attempt < 0:
                    raise ValueError("attempt must be >= 0")
                return base * 2 ** attempt
            '''),
        "lib/http.py": d('''\
            from lib.timing import backoff


            def retry_delays(retries, base=1.0):
                """Delays before each of `retries` HTTP retries."""
                return [backoff(n, base) for n in range(1, retries + 1)]
            '''),
        "lib/jobs.py": d('''\
            from lib.timing import backoff


            def schedule(retries, base=1.0):
                """Delays before each retry of a failed background job."""
                return [backoff(n, base) for n in range(retries)]
            '''),
        "tests/test_timing.py": d('''\
            import unittest

            from lib.http import retry_delays
            from lib.jobs import schedule
            from lib.timing import backoff


            class Backoff(unittest.TestCase):
                def test_backoff_doubles_from_the_base(self):
                    self.assertEqual(backoff(0), 1)
                    self.assertEqual(backoff(3, 0.5), 4)
                    with self.assertRaises(ValueError):
                        backoff(-1)


            class Jobs(unittest.TestCase):
                def test_job_retries_start_at_the_base(self):
                    self.assertEqual(schedule(3), [1, 2, 4])


            class Http(unittest.TestCase):
                def test_one_delay_per_retry(self):
                    self.assertEqual(len(retry_delays(4)), 4)
                    self.assertEqual(retry_delays(0), [])
            '''),
    },
    "naive": {
        "lib/timing.py": ("return base * 2 ** attempt", "return base * 2 ** (attempt - 1)"),
    },
    "reference": {
        "lib/http.py": ("range(1, retries + 1)", "range(retries)"),
    },
    "hidden": d('''\
        import unittest

        from lib.http import retry_delays


        class Issue(unittest.TestCase):
            def test_first_retry_waits_the_base(self):
                self.assertEqual(retry_delays(3), [1, 2, 4])
                self.assertEqual(retry_delays(2, base=0.5), [0.5, 1.0])
        '''),
}
