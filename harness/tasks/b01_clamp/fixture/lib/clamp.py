def clamp(x, lo, hi):
    if x < lo:
        return hi
    if x > hi:
        return hi
    return x
