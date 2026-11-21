from textwrap import dedent as d

SPEC = {
    "id": "t02_display_name",
    "issue": "display_name mangles names with inner capitals: display_name('ronald McDonald') returns 'Ronald Mcdonald'. Only the first letter of each part of the name should change.",
    "files": {
        "lib/text.py": d('''\
            def capitalize_words(text):
                """Title-case each space-separated word: first letter upper, the rest lower."""
                return " ".join(word.capitalize() for word in text.split(" "))
            '''),
        "lib/headings.py": d('''\
            from lib.text import capitalize_words


            def heading(text):
                return capitalize_words(text.strip())
            '''),
        "lib/names.py": d('''\
            from lib.text import capitalize_words


            def display_name(name):
                return capitalize_words(" ".join(name.split()))
            '''),
        "tests/test_text.py": d('''\
            import unittest

            from lib.headings import heading
            from lib.names import display_name


            class Headings(unittest.TestCase):
                def test_headings_are_title_case(self):
                    self.assertEqual(heading("HELLO WORLD"), "Hello World")
                    self.assertEqual(heading("  a tale  "), "A Tale")


            class Names(unittest.TestCase):
                def test_display_name_capitalizes_and_collapses_spaces(self):
                    self.assertEqual(display_name("ada   lovelace"), "Ada Lovelace")
            '''),
    },
    "naive": {
        "lib/text.py": ("word.capitalize()", "word[:1].upper() + word[1:]"),
    },
    "reference": {
        "lib/names.py": d('''\
            def display_name(name):
                return " ".join(part[:1].upper() + part[1:] for part in name.split())
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.names import display_name


        class Issue(unittest.TestCase):
            def test_inner_capitals_are_kept(self):
                self.assertEqual(display_name("ronald McDonald"), "Ronald McDonald")
                self.assertEqual(display_name("leonardo DiCaprio"), "Leonardo DiCaprio")

            def test_plain_names_still_capitalize(self):
                self.assertEqual(display_name("ada"), "Ada")
                self.assertEqual(display_name("  grace  hopper "), "Grace Hopper")
        '''),
}
