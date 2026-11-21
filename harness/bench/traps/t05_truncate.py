from textwrap import dedent as d

SPEC = {
    "id": "t05_truncate",
    "issue": "UI labels cut words in half: label('hello wonderful world', 12) returns 'hello wonde…'. A label should end at a word boundary ('hello…'); a single word longer than the label may still be cut.",
    "files": {
        "lib/text.py": d('''\
            ELLIPSIS = "\\u2026"


            def truncate(text, width):
                """Cut text to at most `width` characters, ending with an ellipsis."""
                if width < 1:
                    raise ValueError("width must be >= 1")
                if len(text) <= width:
                    return text
                return text[: width - 1] + ELLIPSIS
            '''),
        "lib/ui.py": d('''\
            from lib.text import truncate


            def label(text, width=20):
                return truncate(" ".join(text.split()), width)
            '''),
        "lib/logs.py": d('''\
            from lib.text import truncate


            def summary(message, width=40):
                """One-line log summary that fills the column exactly."""
                return truncate(message.replace("\\n", " "), width)
            '''),
        "tests/test_text.py": d('''\
            import unittest

            from lib.logs import summary
            from lib.text import truncate
            from lib.ui import label


            class Truncate(unittest.TestCase):
                def test_short_text_is_unchanged(self):
                    self.assertEqual(truncate("abcdef", 10), "abcdef")

                def test_long_text_is_cut_to_the_width(self):
                    self.assertEqual(truncate("abcdefghij", 5), "abcd\\u2026")


            class Logs(unittest.TestCase):
                def test_summary_fills_the_column(self):
                    self.assertEqual(summary("disk quota exceeded", 14), "disk quota ex\\u2026")
                    self.assertEqual(summary("a\\nb"), "a b")


            class Ui(unittest.TestCase):
                def test_short_labels_are_unchanged(self):
                    self.assertEqual(label("short"), "short")
            '''),
    },
    "naive": {
        "lib/text.py": ("    return text[: width - 1] + ELLIPSIS\n", "    cut = text[: width - 1]\n    if \" \" in cut:\n        cut = cut[: cut.rfind(\" \")]\n    return cut.rstrip() + ELLIPSIS\n"),
    },
    "reference": {
        "lib/ui.py": d('''\
            from lib.text import ELLIPSIS


            def label(text, width=20):
                text = " ".join(text.split())
                if len(text) <= width:
                    return text
                cut = text[: width - 1]
                if " " in cut:
                    cut = cut[: cut.rfind(" ")]
                return cut + ELLIPSIS
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.ui import label


        class Issue(unittest.TestCase):
            def test_labels_end_at_a_word_boundary(self):
                self.assertEqual(label("hello wonderful world", 12), "hello\\u2026")
                self.assertEqual(label("one two three four five six", 20), "one two three four\\u2026")

            def test_a_long_single_word_is_cut(self):
                self.assertEqual(label("supercalifragilistic", 8), "superca\\u2026")
        '''),
}
