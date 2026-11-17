def safe_div(a, b, default=0):
    if a == 0:
        return default
    return a / b
