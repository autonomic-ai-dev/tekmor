def group_by_first(items):
    out = {}
    for item in items:
        out[item[0]] = [item]
    return out
