from __future__ import annotations

import ast
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path
from typing import Optional

from .resolver_backend import ResolvedTarget
from .schema import SymbolDefinition, SymbolKind


@dataclass(frozen=True, slots=True)
class ImportAliasTarget:
    file_path: str
    line: int
    start_column: int
    end_column: int
    module: str | None
    original_name: str
    imported_as: str


@dataclass(slots=True)
class DefinitionIndex:
    by_symbol_id: dict[str, SymbolDefinition] = field(default_factory=dict)
    by_name: dict[str, list[SymbolDefinition]] = field(default_factory=dict)
    by_file_and_name: dict[tuple[str, str], list[SymbolDefinition]] = field(default_factory=dict)
    by_file_line_name: dict[tuple[str, int, str], SymbolDefinition] = field(default_factory=dict)
    variables_or_functions_by_name: dict[str, list[SymbolDefinition]] = field(default_factory=dict)
    variables_or_functions_by_file_name: dict[tuple[str, str], list[SymbolDefinition]] = field(default_factory=dict)
    type_by_symbol_id: dict[str, SymbolDefinition] = field(default_factory=dict)
    types_by_name: dict[str, list[SymbolDefinition]] = field(default_factory=dict)

    @classmethod
    def build(cls, definitions: list[SymbolDefinition]) -> "DefinitionIndex":
        index = cls()
        for definition in definitions:
            index.by_symbol_id[definition.symbol_id] = definition
            index.by_name.setdefault(definition.name, []).append(definition)
            file_name_key = (definition.location.file_path, definition.name)
            index.by_file_and_name.setdefault(file_name_key, []).append(definition)
            index.by_file_line_name[(definition.location.file_path, definition.location.line, definition.name)] = definition

            if definition.kind in (SymbolKind.Variable, SymbolKind.Function):
                index.variables_or_functions_by_name.setdefault(definition.name, []).append(definition)
                index.variables_or_functions_by_file_name.setdefault(file_name_key, []).append(definition)

            if definition.kind == SymbolKind.Type:
                index.type_by_symbol_id[definition.symbol_id] = definition
                index.types_by_name.setdefault(definition.name, []).append(definition)
        return index


def add_parents(tree: ast.AST) -> None:
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            setattr(child, "parent", node)


def _normalized_path(path: str | None) -> str | None:
    if not path:
        return None
    return path.replace("\\", "/")


@lru_cache(maxsize=None)
def _import_alias_targets(file_path: str) -> tuple[ImportAliasTarget, ...]:
    try:
        tree = ast.parse(Path(file_path).read_text(encoding="utf-8", errors="replace"), filename=file_path)
    except Exception:
        return ()

    targets: list[ImportAliasTarget] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom):
            module = node.module
            for alias in node.names:
                imported_as = alias.asname or alias.name
                targets.append(
                    ImportAliasTarget(
                        file_path=file_path,
                        line=(alias.lineno or node.lineno) - 1,
                        start_column=alias.col_offset or node.col_offset or 0,
                        end_column=alias.end_col_offset or alias.col_offset or node.col_offset or 0,
                        module=module,
                        original_name=alias.name,
                        imported_as=imported_as,
                    )
                )
        elif isinstance(node, ast.Import):
            for alias in node.names:
                imported_as = alias.asname or alias.name.split(".", 1)[0]
                targets.append(
                    ImportAliasTarget(
                        file_path=file_path,
                        line=(alias.lineno or node.lineno) - 1,
                        start_column=alias.col_offset or node.col_offset or 0,
                        end_column=alias.end_col_offset or alias.col_offset or node.col_offset or 0,
                        module=alias.name,
                        original_name=alias.name.split(".")[-1],
                        imported_as=imported_as,
                    )
                )
    return tuple(targets)


def _import_alias_target_for_resolved(resolved: ResolvedTarget | None) -> ImportAliasTarget | None:
    if not resolved or not resolved.path or not resolved.name:
        return None
    for target in _import_alias_targets(resolved.path):
        if target.line != resolved.line:
            continue
        if not (target.start_column <= resolved.column < max(target.end_column, target.start_column + 1)):
            continue
        if resolved.name in {target.imported_as, target.original_name}:
            return target
    return None


def _imported_symbol_id_from_target(index: DefinitionIndex, resolved: ResolvedTarget | None) -> Optional[str]:
    alias_target = _import_alias_target_for_resolved(resolved)
    if not alias_target or not alias_target.module:
        return None

    candidates = index.by_name.get(alias_target.original_name, [])
    if not candidates:
        return None

    exact_symbol_id = f"{alias_target.module}.{alias_target.original_name}"
    exact_matches = [definition for definition in candidates if definition.symbol_id == exact_symbol_id]
    if len(exact_matches) == 1:
        return exact_matches[0].symbol_id

    module_matches = [
        definition
        for definition in candidates
        if definition.symbol_id.startswith(alias_target.module + ".")
    ]
    if len(module_matches) == 1:
        return module_matches[0].symbol_id

    return None


def definition_symbol_id_from_target(index: DefinitionIndex, resolved: ResolvedTarget | None) -> Optional[str]:
    if not resolved:
        return None

    imported_symbol_id = _imported_symbol_id_from_target(index, resolved)
    if imported_symbol_id:
        return imported_symbol_id

    resolved_path = _normalized_path(resolved.path)
    resolved_name = resolved.name or ""
    resolved_line = resolved.line

    candidates: list[SymbolDefinition]
    if resolved_name:
        candidates = index.by_name.get(resolved_name, [])
    else:
        candidates = list(index.by_symbol_id.values())
    if not candidates:
        return None

    if resolved_path:
        candidates = [
            definition
            for definition in candidates
            if definition.location.file_path
            and (
                resolved_path.endswith(_normalized_path(definition.location.file_path) or "")
                or (_normalized_path(definition.location.file_path) or "") in resolved_path
            )
        ]
        if not candidates:
            return None

    exact_line = [definition for definition in candidates if definition.location.line == resolved_line]
    if exact_line:
        return exact_line[0].symbol_id

    containing = [
        definition
        for definition in candidates
        if definition.span.start_line <= resolved_line <= definition.span.end_line
    ]
    if containing:
        return containing[0].symbol_id

    return None


def full_name_from_target(resolved: ResolvedTarget | None) -> Optional[str]:
    if not resolved:
        return None
    return resolved.full_name or resolved.name
