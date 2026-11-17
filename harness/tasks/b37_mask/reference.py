def mask_card(num):
    digits = ''.join(c for c in num if c.isdigit())
    return '*' * (len(digits) - 4) + digits[-4:]
