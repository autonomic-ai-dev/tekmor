def hex_color(rgb):
    if len(rgb) != 3 or any(not 0 <= c <= 255 for c in rgb):
        raise ValueError(rgb)
    return '#%x%x%x' % tuple(rgb)
