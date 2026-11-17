def window_max(xs, k):
    return [min(xs[i:i + k]) for i in range(len(xs) - k + 1)]
