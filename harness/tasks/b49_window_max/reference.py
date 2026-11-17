def window_max(xs, k):
    return [max(xs[i:i + k]) for i in range(len(xs) - k + 1)]
