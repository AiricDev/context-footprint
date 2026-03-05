"""Fixture: cls() in classmethod must resolve to enclosing class __init__."""

class MyClass:
    def __init__(self, name):
        self.name = name

    @classmethod
    def create(cls, name):
        return cls(name)
