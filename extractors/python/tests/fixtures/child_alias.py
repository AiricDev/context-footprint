"""Child class that imports base under an alias; super().__init__() must resolve to base_alias.Base.__init__."""

from base_alias import Base as AliasBase


class Child(AliasBase):
    def __init__(self):
        super().__init__()
        self.y = 2
