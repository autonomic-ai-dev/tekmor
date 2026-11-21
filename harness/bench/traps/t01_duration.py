from textwrap import dedent as d

SPEC = {
    "id": "t01_duration",
    "issue": "parse_duration rejects durations written with spaces between the parts: parse_duration('1h 30m') raises ValueError instead of returning 5400.",
    "files": {
        "lib/duration.py": d('''\
            import re

            _PART = re.compile(r"(\\d+)([hms])")
            _UNITS = {"h": 3600, "m": 60, "s": 1}


            def parse_duration(text):
                """Parse '1h30m', '45s' or a bare number of minutes into seconds."""
                text = text.strip().lower()
                if not text:
                    raise ValueError("empty duration")
                if text.isdigit():
                    return int(text) * 60
                total = 0
                pos = 0
                for match in _PART.finditer(text):
                    if match.start() != pos:
                        raise ValueError(f"bad duration: {text!r}")
                    total += int(match.group(1)) * _UNITS[match.group(2)]
                    pos = match.end()
                if pos != len(text):
                    raise ValueError(f"bad duration: {text!r}")
                return total
            '''),
        "lib/schedule.py": d('''\
            from lib.duration import parse_duration


            def timeout_seconds(job):
                return parse_duration(job.get("timeout", "10"))
            '''),
        "tests/test_duration.py": d('''\
            import unittest

            from lib.duration import parse_duration
            from lib.schedule import timeout_seconds


            class ParseDuration(unittest.TestCase):
                def test_units(self):
                    self.assertEqual(parse_duration("1h30m"), 5400)
                    self.assertEqual(parse_duration("45s"), 45)
                    self.assertEqual(parse_duration("2M"), 120)

                def test_a_bare_number_is_minutes(self):
                    self.assertEqual(parse_duration("90"), 5400)

                def test_rejects_garbage(self):
                    for bad in ["", "1x", "h", "1h30"]:
                        with self.assertRaises(ValueError):
                            parse_duration(bad)

                def test_two_bare_numbers_are_ambiguous(self):
                    with self.assertRaises(ValueError):
                        parse_duration("1 30")

                def test_job_timeout_defaults_to_ten_minutes(self):
                    self.assertEqual(timeout_seconds({}), 600)
            '''),
    },
    "naive": {
        "lib/duration.py": ('text = text.strip().lower()\n', 'text = text.strip().lower().replace(" ", "")\n'),
    },
    "reference": {
        "lib/duration.py": d('''\
            import re

            _PART = re.compile(r"(\\d+)([hms])")
            _UNITS = {"h": 3600, "m": 60, "s": 1}


            def parse_duration(text):
                """Parse '1h30m', '45s' or a bare number of minutes into seconds."""
                tokens = text.strip().lower().split()
                if not tokens:
                    raise ValueError("empty duration")
                if len(tokens) == 1 and tokens[0].isdigit():
                    return int(tokens[0]) * 60
                total = 0
                for token in tokens:
                    pos = 0
                    for match in _PART.finditer(token):
                        if match.start() != pos:
                            raise ValueError(f"bad duration: {text!r}")
                        total += int(match.group(1)) * _UNITS[match.group(2)]
                        pos = match.end()
                    if pos == 0 or pos != len(token):
                        raise ValueError(f"bad duration: {text!r}")
                return total
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.duration import parse_duration


        class Issue(unittest.TestCase):
            def test_spaces_between_parts(self):
                self.assertEqual(parse_duration("1h 30m"), 5400)
                self.assertEqual(parse_duration("2m 15s"), 135)
                self.assertEqual(parse_duration(" 1H  5S "), 3605)

            def test_a_part_without_a_unit_is_still_rejected(self):
                with self.assertRaises(ValueError):
                    parse_duration("1h 30")
        '''),
}
