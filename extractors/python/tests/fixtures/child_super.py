"""Child class in another module; super() must resolve to base_super.Base.__init__."""

from base_super import Base


class Child(Base):
    def __init__(self):
        super().__init__()
        self.y = 2
