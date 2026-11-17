def group_by_first(items):
    out = {}
    for item in items:
        out.setdefault(item[0], []).append(item)
    return out
