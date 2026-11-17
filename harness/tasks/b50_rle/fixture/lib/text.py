def rle(s):
    out = []
    for c in s:
        if out and out[-1][0] == c:
            out[-1][1] = 2
        else:
            out.append([c, 1])
    return ''.join(f'{c}{n}' for c, n in out)
