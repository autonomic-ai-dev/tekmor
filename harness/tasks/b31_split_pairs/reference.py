def split_pairs(s):
    out = {}
    for pair in s.split(';'):
        if not pair:
            continue
        k, v = pair.split('=', 1)
        out[k] = v
    return out
