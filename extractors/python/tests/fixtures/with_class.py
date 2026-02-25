"""Fixture with a class and method call."""

from simple import bar


class Helper:
    """A helper class."""

    def run(self, n: int) -> int:
        return bar(n)
