def flatten(items):
    out = []
    for x in items:
        if isinstance(x, list):
            out.extend(x)
        else:
            out.append(x)
    return out
