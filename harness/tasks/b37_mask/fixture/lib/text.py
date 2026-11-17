def mask_card(num):
    digits = ''.join(c for c in num if c.isdigit())
    return digits[:4] + '*' * (len(digits) - 4)
