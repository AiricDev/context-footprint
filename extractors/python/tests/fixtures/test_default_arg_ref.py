"""Fixture: default argument references (e.g. sentinel) must be emitted as Read."""

SENTINEL = object()


def foo(x: int = 0, flag: bool = SENTINEL):
    if flag is SENTINEL:
        flag = True
    return x if flag else 0
