from textwrap import dedent as d

SPEC = {
    "id": "t10_settings",
    "issue": "Overriding one nested setting drops its siblings: with defaults {'theme': {'color': 'blue', 'font': 'mono'}} and user settings {'theme': {'color': 'red'}}, load_settings loses the font. Nested sections should be merged.",
    "files": {
        "lib/merge.py": d('''\
            def merge(base, override):
                """Return a copy of base updated with override; override wins."""
                result = dict(base)
                result.update(override)
                return result
            '''),
        "lib/settings.py": d('''\
            from lib.merge import merge


            def load_settings(defaults, user):
                return merge(defaults, user)
            '''),
        "lib/permissions.py": d('''\
            from lib.merge import merge


            def effective(role, override):
                """A role override replaces whole sections: an 'allow' section in the
                override is the complete allow list."""
                return merge(role, override)
            '''),
        "tests/test_settings.py": d('''\
            import unittest

            from lib.merge import merge
            from lib.permissions import effective
            from lib.settings import load_settings


            class Settings(unittest.TestCase):
                def test_top_level_keys(self):
                    self.assertEqual(load_settings({"a": 1}, {"b": 2}), {"a": 1, "b": 2})
                    self.assertEqual(load_settings({"a": 1}, {"a": 3}), {"a": 3})

                def test_merge_does_not_change_its_inputs(self):
                    base = {"a": {"b": 1}}
                    merge(base, {"a": {"c": 2}})
                    self.assertEqual(base, {"a": {"b": 1}})


            class Permissions(unittest.TestCase):
                def test_an_override_replaces_the_allow_section(self):
                    role = {"allow": {"read": True, "write": True}}
                    self.assertEqual(effective(role, {"allow": {"read": True}}), {"allow": {"read": True}})
            '''),
    },
    "naive": {
        "lib/merge.py": d('''\
            def merge(base, override):
                """Return a copy of base updated with override; override wins."""
                result = dict(base)
                for key, value in override.items():
                    if isinstance(value, dict) and isinstance(result.get(key), dict):
                        result[key] = merge(result[key], value)
                    else:
                        result[key] = value
                return result
            '''),
    },
    "reference": {
        "lib/settings.py": d('''\
            def _deep_merge(base, override):
                result = dict(base)
                for key, value in override.items():
                    if isinstance(value, dict) and isinstance(result.get(key), dict):
                        result[key] = _deep_merge(result[key], value)
                    else:
                        result[key] = value
                return result


            def load_settings(defaults, user):
                return _deep_merge(defaults, user)
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.settings import load_settings


        class Issue(unittest.TestCase):
            def test_nested_sections_are_merged(self):
                defaults = {"theme": {"color": "blue", "font": "mono"}}
                self.assertEqual(
                    load_settings(defaults, {"theme": {"color": "red"}}),
                    {"theme": {"color": "red", "font": "mono"}},
                )
                self.assertEqual(defaults, {"theme": {"color": "blue", "font": "mono"}})

            def test_deeper_nesting_and_scalar_replacement(self):
                defaults = {"a": {"b": {"c": 1, "d": 2}}, "x": {"y": 1}}
                self.assertEqual(
                    load_settings(defaults, {"a": {"b": {"c": 9}}, "x": 5}),
                    {"a": {"b": {"c": 9, "d": 2}}, "x": 5},
                )
        '''),
}
