def bytes_human(n):
    units = ['B', 'KB', 'MB', 'GB']
    i = 0
    while n >= 1024 and i < len(units) - 1:
        n /= 1024
        i += 1
    if i == 0:
        return f'{int(n)} B'
    return f'{n:.1f} {units[i]}'
