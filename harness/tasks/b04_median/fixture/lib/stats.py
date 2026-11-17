def median(xs):
    if not xs:
        raise ValueError('empty')
    s = sorted(xs)
    n = len(s)
    if n % 2:
        return s[n // 2]
    return s[n // 2]
