def lerp(x0, y0, x1, y1, x):
    return y0 + (y1 - y0) * (x - x1) / (x1 - x0)
