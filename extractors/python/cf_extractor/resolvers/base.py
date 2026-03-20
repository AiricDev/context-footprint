from __future__ import annotations

import abc
from dataclasses import dataclass, field
from typing import Any, Optional


DEFAULT_RESOLVER_BACKEND = "ty"
RESOLVER_BACKENDS = ("jedi", "ty", "pyrefly")


@dataclass(slots=True)
class ResolvedTarget:
    path: Optional[str]
    line: int
    column: int
    name: Optional[str] = None
    full_name: Optional[str] = None
    kind: Optional[str] = None
    documentation: list[str] = field(default_factory=list)
    signature: Optional[str] = None


class DocumentResolver(abc.ABC):
    @abc.abstractmethod
    def goto(self, line: int, column: int, *, follow_imports: bool = False) -> list[ResolvedTarget]:
        raise NotImplementedError

    def references(self, line: int, column: int) -> list[ResolvedTarget]:
        return []

    def document_symbols(self) -> list[dict[str, Any]]:
        return []

    def workspace_symbols(self, query: str) -> list[dict[str, Any]]:
        return []

    def hover(self, line: int, column: int) -> list[str]:
        return []


class ProjectResolverBackend(abc.ABC):
    @abc.abstractmethod
    def open_document(self, file_path: str, source: str) -> DocumentResolver:
        raise NotImplementedError

    def close(self) -> None:
        return None
