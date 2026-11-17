import importlib.util
import pathlib
import sys

sys.dont_write_bytecode = True


def load(root):
    path = pathlib.Path(root) / "lib" / "clamp.py"
    spec = importlib.util.spec_from_file_location("clamp", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def run(fn, args):
    try:
        return fn(*args)
    except Exception as exc:
        return ("raises", type(exc).__name__)
