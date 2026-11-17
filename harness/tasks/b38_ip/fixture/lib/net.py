def ip_to_int(s):
    parts = [int(p) for p in s.split('.')]
    if len(parts) != 4 or any(p < 0 or p > 255 for p in parts):
        raise ValueError(s)
    return sum(p << (8 * i) for i, p in enumerate(parts))
