def percent(part, total, ndigits=1):
    if total == 0:
        raise ValueError('total is zero')
    return round(100 * part // total, ndigits)
