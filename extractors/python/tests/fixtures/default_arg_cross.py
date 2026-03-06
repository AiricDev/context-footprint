"""Uses SENTINEL from another module as default arg; must emit Read to sentinel_def.SENTINEL."""

from sentinel_def import SENTINEL


def foo(x: int = 0, flag: object = SENTINEL):
    if flag is SENTINEL:
        flag = True
    return x if flag else 0
