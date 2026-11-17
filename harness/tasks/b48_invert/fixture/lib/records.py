def invert(d):
    out = {}
    for k, v in d.items():
        out[v] = [k]
    return out
