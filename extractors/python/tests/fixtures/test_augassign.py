"""Fixture: augmented assignment (e.g. x += 1) must produce both Read and Write refs."""

counter = 0

def increment():
    global counter
    counter += 1
