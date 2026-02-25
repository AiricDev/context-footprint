"""Minimal fixture: one function calls another."""

def foo(x: int) -> int:
    """Foo doc."""
    return bar(x)


def bar(y: int) -> int:
    return y + 1
