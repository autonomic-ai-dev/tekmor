def parse_bool(s):
    v = s.strip()
    if v in ('true', 'yes', '1'):
        return True
    if v in ('false', 'no', '0'):
        return False
    raise ValueError(s)
