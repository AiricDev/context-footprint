"""Fixture for resolving self.method() to same-class method (e.g. put -> api_route)."""


class APIRouter:
    def api_route(self, path: str, methods: list):
        return None

    def put(self, path: str):
        return self.api_route(path=path, methods=["PUT"])
