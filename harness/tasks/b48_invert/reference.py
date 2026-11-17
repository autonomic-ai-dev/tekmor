def invert(d):
    out = {}
    for k, v in d.items():
        out.setdefault(v, []).append(k)
    return out
