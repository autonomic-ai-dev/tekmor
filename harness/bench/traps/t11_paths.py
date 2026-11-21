from textwrap import dedent as d

SPEC = {
    "id": "t11_paths",
    "issue": "normalize keeps a trailing slash, so '/users/' and '/users' are treated as different routes. A trailing slash should be removed.",
    "files": {
        "lib/paths.py": d('''\
            import re


            def normalize(path):
                """Collapse repeated slashes."""
                return re.sub("/+", "/", path)
            '''),
        "lib/routes.py": d('''\
            from lib.paths import normalize


            def route_key(path):
                return normalize(path)


            def is_root(path):
                return normalize(path) == "/"
            '''),
        "tests/test_paths.py": d('''\
            import unittest

            from lib.paths import normalize
            from lib.routes import is_root


            class Normalize(unittest.TestCase):
                def test_repeated_slashes_collapse(self):
                    self.assertEqual(normalize("a//b"), "a/b")

                def test_the_root_stays_the_root(self):
                    self.assertEqual(normalize("/"), "/")
                    self.assertEqual(normalize("///"), "/")


            class Routes(unittest.TestCase):
                def test_root_detection(self):
                    self.assertTrue(is_root("//"))
                    self.assertFalse(is_root("/users"))
            '''),
    },
    "naive": {
        "lib/paths.py": ('return re.sub("/+", "/", path)', 'return re.sub("/+", "/", path).rstrip("/")'),
    },
    "reference": {
        "lib/paths.py": ('    return re.sub("/+", "/", path)\n', '    path = re.sub("/+", "/", path)\n    return path.rstrip("/") if len(path) > 1 else path\n'),
    },
    "hidden": d('''\
        import unittest

        from lib.paths import normalize


        class Issue(unittest.TestCase):
            def test_trailing_slashes_are_removed(self):
                self.assertEqual(normalize("/users/"), "/users")
                self.assertEqual(normalize("a/b//"), "a/b")
                self.assertEqual(normalize("/a//b/"), "/a/b")
        '''),
}
