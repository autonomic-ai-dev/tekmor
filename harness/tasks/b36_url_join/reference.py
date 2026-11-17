def url_join(base, path):
    return base.rstrip('/') + '/' + path.lstrip('/')
