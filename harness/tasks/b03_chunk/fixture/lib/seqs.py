def chunk(seq, n):
    return [seq[i:i + n] for i in range(0, len(seq) - n + 1, n)]
