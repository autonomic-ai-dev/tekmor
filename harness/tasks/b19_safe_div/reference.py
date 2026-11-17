def safe_div(a, b, default=0):
    if b == 0:
        return default
    return a / b
