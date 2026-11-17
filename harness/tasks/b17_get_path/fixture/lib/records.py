def get_path(d, path, default=None):
    cur = d
    for key in path.split('.'):
        if key not in cur:
            return None
        cur = cur[key]
    return cur
