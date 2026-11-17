import re

UNITS = {'h': 3600, 'm': 60, 's': 1}


def parse_duration(s):
    parts = re.findall(r'(\d+)([hms])', s)
    if not parts or ''.join(a + b for a, b in parts) != s:
        raise ValueError(s)
    return sum(int(n) * UNITS[u] for n, u in parts)
