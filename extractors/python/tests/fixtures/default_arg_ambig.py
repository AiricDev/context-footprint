"""Uses _sentinel from sentinel_a (not sentinel_b); import context must disambiguate."""

from sentinel_a import _sentinel


def bar(x: int = 0, flag: object = _sentinel) -> object:
    if flag is _sentinel:
        return x
    return flag
