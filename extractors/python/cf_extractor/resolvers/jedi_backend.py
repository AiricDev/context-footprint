from __future__ import annotations

from typing import Any

import jedi

from .base import DocumentResolver, ProjectResolverBackend, ResolvedTarget


class JediDocumentResolver(DocumentResolver):
    def __init__(self, script: jedi.Script):
        self._script = script

    @staticmethod
    def _to_target(definition: Any) -> ResolvedTarget:
        path = str(definition.module_path) if definition.module_path else None
        line = max(0, (definition.line or 1) - 1)
        column = definition.column or 0
        docs: list[str] = []
        signature = None
        try:
            doc_str = definition.docstring()
            if doc_str:
                docs.append(doc_str)
        except Exception:
            pass
        try:
            sigs = definition.get_signatures()
            if sigs:
                signature = sigs[0].to_string()
        except Exception:
            pass
        return ResolvedTarget(
            path=path,
            line=line,
            column=column,
            name=getattr(definition, "name", None),
            full_name=getattr(definition, "full_name", None),
            kind=getattr(definition, "type", None),
            documentation=docs,
            signature=signature,
        )

    def goto(self, line: int, column: int, *, follow_imports: bool = False) -> list[ResolvedTarget]:
        try:
            defs = self._script.goto(line, column, follow_imports=follow_imports)
        except Exception:
            return []
        return [self._to_target(definition) for definition in defs]


class JediProjectResolverBackend(ProjectResolverBackend):
    def __init__(self, *, venv_path: str | None = None):
        self._environment = jedi.create_environment(venv_path, safe=False) if venv_path else None

    def open_document(self, file_path: str, source: str) -> DocumentResolver:
        return JediDocumentResolver(jedi.Script(source, path=file_path, environment=self._environment))
