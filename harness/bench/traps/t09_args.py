from textwrap import dedent as d

SPEC = {
    "id": "t09_args",
    "issue": "Quoted arguments are split apart: split_args('say \"hello world\"') returns ['say', '\"hello', 'world\"'] instead of ['say', 'hello world'].",
    "files": {
        "lib/args.py": d('''\
            def split_args(line):
                """Split a command line into arguments."""
                return line.split()
            '''),
        "lib/runner.py": d('''\
            from lib.args import split_args


            def program_and_args(line):
                parts = split_args(line)
                return parts[0], parts[1:]
            '''),
        "tests/test_args.py": d('''\
            import unittest

            from lib.args import split_args
            from lib.runner import program_and_args


            class SplitArgs(unittest.TestCase):
                def test_whitespace(self):
                    self.assertEqual(split_args("a  b c"), ["a", "b", "c"])

                def test_windows_paths_keep_their_backslashes(self):
                    self.assertEqual(
                        split_args(r"copy C:\\temp\\x.txt D:\\out"),
                        ["copy", "C:\\\\temp\\\\x.txt", "D:\\\\out"],
                    )


            class Runner(unittest.TestCase):
                def test_program_and_args(self):
                    self.assertEqual(program_and_args("ls -la"), ("ls", ["-la"]))
            '''),
    },
    "naive": {
        "lib/args.py": d('''\
            import shlex


            def split_args(line):
                """Split a command line into arguments."""
                return shlex.split(line)
            '''),
    },
    "reference": {
        "lib/args.py": d('''\
            import shlex


            def split_args(line):
                """Split a command line into arguments."""
                lexer = shlex.shlex(line, posix=True)
                lexer.whitespace_split = True
                lexer.escape = ""
                return list(lexer)
            '''),
    },
    "hidden": d('''\
        import unittest

        from lib.args import split_args


        class Issue(unittest.TestCase):
            def test_quoted_arguments_stay_together(self):
                self.assertEqual(split_args('say "hello world"'), ["say", "hello world"])
                self.assertEqual(split_args("a 'b c' d"), ["a", "b c", "d"])
                self.assertEqual(split_args('echo "x"'), ["echo", "x"])
        '''),
}
