"""Fixture: super().method() must resolve to parent class method."""

class Base:
    def __init__(self, name):
        self.name = name

class Child(Base):
    def __init__(self, name, extra):
        super().__init__(name)
        self.extra = extra
