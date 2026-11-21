from textwrap import dedent as d

SPEC = {
    "id": "t03_paging",
    "issue": "The items endpoint skips the first page: list_items(items, page=1) returns items 11 to 20 instead of the first 10. Page numbers from the query string start at 1, and page 0 should raise ValueError.",
    "files": {
        "lib/paging.py": d('''\
            def offset(page, size):
                """Index of the first item on a zero-based page."""
                if page < 0 or size < 1:
                    raise ValueError("page must be >= 0 and size >= 1")
                return page * size


            def page_count(total, size):
                return (total + size - 1) // size
            '''),
        "lib/api.py": d('''\
            from lib.paging import offset

            PAGE_SIZE = 10


            def list_items(items, page=1):
                """Items for a page number taken from the query string."""
                start = offset(page, PAGE_SIZE)
                return items[start:start + PAGE_SIZE]
            '''),
        "lib/export.py": d('''\
            from lib.paging import offset, page_count


            def export_all(items, size=100):
                out = []
                for page in range(page_count(len(items), size)):
                    start = offset(page, size)
                    out.extend(items[start:start + size])
                return out
            '''),
        "tests/test_paging.py": d('''\
            import unittest

            from lib.api import list_items
            from lib.export import export_all
            from lib.paging import offset, page_count


            class Paging(unittest.TestCase):
                def test_offsets(self):
                    self.assertEqual(offset(0, 10), 0)
                    self.assertEqual(offset(3, 10), 30)
                    with self.assertRaises(ValueError):
                        offset(-1, 10)

                def test_page_count(self):
                    self.assertEqual(page_count(0, 10), 0)
                    self.assertEqual(page_count(21, 10), 3)


            class Export(unittest.TestCase):
                def test_export_returns_every_item_once(self):
                    self.assertEqual(export_all(list(range(25)), size=10), list(range(25)))


            class Api(unittest.TestCase):
                def test_a_page_has_at_most_ten_items(self):
                    self.assertEqual(len(list_items(list(range(100)), page=2)), 10)
            '''),
    },
    "naive": {
        "lib/paging.py": ("if page < 0 or size < 1:\n        raise ValueError(\"page must be >= 0 and size >= 1\")\n    return page * size", "if page < 1 or size < 1:\n        raise ValueError(\"page must be >= 1 and size >= 1\")\n    return (page - 1) * size"),
    },
    "reference": {
        "lib/api.py": d('''\
            from lib.paging import offset

            PAGE_SIZE = 10


            def list_items(items, page=1):
                """Items for a page number taken from the query string."""
                if page < 1:
                    raise ValueError("pages start at 1")
                start = offset(page - 1, PAGE_SIZE)
                return items[start:start + PAGE_SIZE]
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.api import list_items


        class Issue(unittest.TestCase):
            def test_pages_start_at_one(self):
                items = list(range(25))
                self.assertEqual(list_items(items, page=1), list(range(10)))
                self.assertEqual(list_items(items, page=3), list(range(20, 25)))
                self.assertEqual(list_items(items, page=4), [])

            def test_page_zero_is_rejected(self):
                with self.assertRaises(ValueError):
                    list_items(list(range(25)), page=0)
        '''),
}
