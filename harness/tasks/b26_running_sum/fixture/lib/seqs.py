def running_sum(xs):
    out = []
    total = 0
    for x in xs:
        out.append(total)
        total += x
    return out
