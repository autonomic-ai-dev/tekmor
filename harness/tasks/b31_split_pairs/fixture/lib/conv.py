def split_pairs(s):
    out = {}
    for pair in s.split(';'):
        if not pair:
            continue
        k, v = pair.split('=')
        out[k] = v
    return out
