import importlib
import pkgutil

TRAPS = [
    importlib.import_module(f"{__name__}.{info.name}").SPEC
    for info in sorted(pkgutil.iter_modules(__path__), key=lambda info: info.name)
]
