"""Fixture: call inside nested function should be attributed to outer defined symbol."""


class APIRouter:
    def add_api_route(self, path: str, fn):
        pass

    def api_route(self, path: str):
        def decorator(fn):
            self.add_api_route(path, fn)
            return fn
        return decorator
