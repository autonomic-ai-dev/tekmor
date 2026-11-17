import re


def camel_to_snake(s):
    return re.sub(r'(?<=[a-z0-9])(?=[A-Z])', '_', s).lower()
