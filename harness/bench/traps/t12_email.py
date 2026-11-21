from textwrap import dedent as d

SPEC = {
    "id": "t12_email",
    "issue": "Addresses with a plus tag are rejected: is_email('ann+news@example.com') returns False.",
    "files": {
        "lib/validate.py": d('''\
            import re

            _EMAIL = re.compile(r"^[A-Za-z0-9_.]+@[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)+$")


            def is_email(text):
                return bool(_EMAIL.match(text.strip()))
            '''),
        "lib/signup.py": d('''\
            from lib.validate import is_email


            def check(form):
                errors = []
                if not is_email(form.get("email", "")):
                    errors.append("email")
                return errors
            '''),
        "tests/test_validate.py": d('''\
            import unittest

            from lib.signup import check
            from lib.validate import is_email


            class Email(unittest.TestCase):
                def test_valid_addresses(self):
                    for good in ["ann@example.com", "a.b@x.co.uk"]:
                        self.assertTrue(is_email(good), good)

                def test_invalid_addresses(self):
                    for bad in ["a@b@c.com", "ann@exa mple.com", "@x.com", "ann@", "plain"]:
                        self.assertFalse(is_email(bad), bad)


            class Signup(unittest.TestCase):
                def test_a_bad_email_is_reported(self):
                    self.assertEqual(check({"email": "x"}), ["email"])
            '''),
    },
    "naive": {
        "lib/validate.py": ('r"^[A-Za-z0-9_.]+@[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)+$"', 'r"^\\S+@\\S+\\.\\S+$"'),
    },
    "reference": {
        "lib/validate.py": ('r"^[A-Za-z0-9_.]+@', 'r"^[A-Za-z0-9_.+-]+@'),
    },
    "hidden": d('''\
        import unittest

        from lib.validate import is_email


        class Issue(unittest.TestCase):
            def test_plus_tags_are_accepted(self):
                self.assertTrue(is_email("ann+news@example.com"))
                self.assertTrue(is_email("a+b+c@x.org"))
        '''),
}
