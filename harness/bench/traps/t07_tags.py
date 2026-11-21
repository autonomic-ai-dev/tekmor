from textwrap import dedent as d

SPEC = {
    "id": "t07_tags",
    "issue": "dedupe_tags keeps 'Python' and 'python' as two tags. Tags should be compared without regard to case.",
    "files": {
        "lib/strings.py": d('''\
            def canonical(text):
                """Trim surrounding whitespace."""
                return text.strip()
            '''),
        "lib/tags.py": d('''\
            from lib.strings import canonical


            def dedupe_tags(tags):
                seen = set()
                out = []
                for tag in tags:
                    key = canonical(tag)
                    if key and key not in seen:
                        seen.add(key)
                        out.append(key)
                return out
            '''),
        "lib/accounts.py": d('''\
            from lib.strings import canonical


            def is_taken(username, existing):
                """Usernames are case-sensitive: 'Alice' and 'alice' are different accounts."""
                return canonical(username) in {canonical(name) for name in existing}
            '''),
        "tests/test_strings.py": d('''\
            import unittest

            from lib.accounts import is_taken
            from lib.tags import dedupe_tags


            class Tags(unittest.TestCase):
                def test_blank_and_padded_duplicates_are_dropped(self):
                    self.assertEqual(dedupe_tags(["x", " x ", "y", ""]), ["x", "y"])


            class Accounts(unittest.TestCase):
                def test_usernames_are_case_sensitive(self):
                    self.assertFalse(is_taken("Alice", ["alice"]))

                def test_padding_does_not_make_a_new_username(self):
                    self.assertTrue(is_taken(" bob ", ["bob"]))
            '''),
    },
    "naive": {
        "lib/strings.py": ("return text.strip()", "return text.strip().lower()"),
    },
    "reference": {
        "lib/tags.py": d('''\
            from lib.strings import canonical


            def dedupe_tags(tags):
                seen = set()
                out = []
                for tag in tags:
                    tag = canonical(tag)
                    key = tag.lower()
                    if key and key not in seen:
                        seen.add(key)
                        out.append(tag)
                return out
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.tags import dedupe_tags


        class Issue(unittest.TestCase):
            def test_case_variants_are_one_tag(self):
                result = dedupe_tags(["Python", "python", " PYTHON "])
                self.assertEqual(len(result), 1)
                self.assertEqual(result[0].lower(), "python")
                self.assertEqual(len(dedupe_tags(["a", "B", "b", "A"])), 2)
        '''),
}
