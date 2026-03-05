"""Fixture: function/method used as value (not called) must produce Read reference."""

def handler():
    pass

def register(fn):
    fn()

def setup():
    register(handler)  # handler as value -> should produce Read from setup to handler
    callback = handler  # same
