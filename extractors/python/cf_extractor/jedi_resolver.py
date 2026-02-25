"""
Pass 2: Collect references (Call, Read, Write, Decorate) and use Jedi to resolve
target_symbol where possible. Fallback to receiver + method_name for builder recovery.
"""

import ast
import os
from pathlib import Path
from typing import Any, Optional

import jedi


def _add_parents(tree: ast.AST) -> None:
    """Mutate AST to add .parent on each node for later reference."""
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            setattr(child, "parent", node)

from .schema import (
    DocumentSemantics,
    ReferenceRole,
    SourceLocation,
    SymbolDefinition,
    SymbolKind,
    SymbolReference,
)


def _definition_symbol_id_from_jedi(definitions: list[SymbolDefinition], jedi_def) -> Optional[str]:
    """Map a Jedi Definition to our symbol_id by matching module_path, line, and name."""
    if not jedi_def or not definitions:
        return None
    try:
        j_path = str(jedi_def.module_path) if jedi_def.module_path else None
        j_line = jedi_def.line if jedi_def.line else 0
        j_name = jedi_def.name or ""
        j_line_0 = j_line - 1 if j_line else -1
        for d in definitions:
            if d.name != j_name:
                continue
            if j_path and d.location.file_path:
                norm_j = j_path.replace("\\", "/")
                norm_d = d.location.file_path.replace("\\", "/")
                if not (norm_j.endswith(norm_d) or norm_d in norm_j):
                    continue
            if j_line_0 >= 0 and d.location.line == j_line_0:
                return d.symbol_id
            if j_line_0 >= 0 and d.span.start_line <= j_line_0 <= d.span.end_line:
                return d.symbol_id
        return None
    except Exception:
        return None


def _full_name_from_jedi(jedi_def) -> Optional[str]:
    """Build a hierarchical name from Jedi definition for cross-file symbol_id matching."""
    if not jedi_def:
        return None
    try:
        return jedi_def.full_name
    except Exception:
        return jedi_def.name if jedi_def.name else None


class ReferenceCollector(ast.NodeVisitor):
    """Collects references (Call, Read, Write, Decorate) and uses Jedi to resolve targets."""

    def __init__(
        self,
        file_path: str,
        source: str,
        project_root: str,
        all_definitions: list[SymbolDefinition],
        module_symbol_id: str,
        script: Optional[jedi.Script] = None,
    ):
        self.file_path = file_path
        self.rel_path = os.path.relpath(file_path, project_root).replace("\\", "/")
        self.source = source
        self.project_root = project_root
        self.all_definitions = all_definitions
        self.module_symbol_id = module_symbol_id
        self.script = script or jedi.Script(source, path=file_path)
        self.references: list[SymbolReference] = []
        self._func_spans: list[tuple[int, int, str]] = []

    def _enclosing_symbol(self, node: ast.AST) -> Optional[str]:
        line_1 = node.lineno
        for start, end, sym_id in reversed(self._func_spans):
            if start <= line_1 <= end:
                return sym_id
        return self.module_symbol_id

    def _loc(self, node: ast.AST) -> SourceLocation:
        return SourceLocation(
            file_path=self.rel_path,
            line=node.lineno - 1,
            column=getattr(node, "col_offset", 0) or 0,
        )

    def _resolve_at(self, line: int, column: int):
        try:
            return self.script.goto(line, column)
        except Exception:
            return []

    def _infer_at(self, line: int, column: int):
        try:
            return self.script.infer(line, column)
        except Exception:
            return []

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        enc = self._enclosing_symbol(node)
        func_id = f"{enc}.{node.name}"
        for d in self.all_definitions:
            if d.symbol_id == func_id or (d.name == node.name and d.location.file_path == self.rel_path and d.location.line == node.lineno - 1):
                func_id = d.symbol_id
                break
        start = node.lineno
        end = getattr(node, "end_lineno", node.lineno)
        self._func_spans.append((start, end, func_id))
        for dec in node.decorator_list:
            self._visit_decorator(dec, func_id)
        self.generic_visit(node)
        self._func_spans.pop()

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.visit_FunctionDef(node)

    def _visit_decorator(self, node: ast.expr, decorated_symbol_id: str) -> None:
        enc = self._enclosing_symbol(node)
        target_sym = None
        if isinstance(node, ast.Name):
            defs = self._resolve_at(node.lineno, node.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
        elif isinstance(node, ast.Attribute):
            defs = self._resolve_at(node.lineno, node.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
        if target_sym or True:
            self.references.append(
                SymbolReference(
                    target_symbol=target_sym,
                    location=self._loc(node),
                    enclosing_symbol=decorated_symbol_id,
                    role=ReferenceRole.Decorate,
                    receiver=None,
                    method_name=None,
                    assigned_to=None,
                )
            )

    def visit_Call(self, node: ast.Call) -> None:
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        target_sym = None
        receiver_sym = None
        method_name = None
        if isinstance(node.func, ast.Name):
            defs = self._resolve_at(node.func.lineno, node.func.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
        elif isinstance(node.func, ast.Attribute):
            method_name = node.func.attr
            defs = self._resolve_at(node.func.lineno, node.func.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
            if isinstance(node.func.value, ast.Name):
                receiver_defs = self._resolve_at(node.func.value.lineno, node.func.value.col_offset)
                if receiver_defs:
                    receiver_sym = _definition_symbol_id_from_jedi(self.all_definitions, receiver_defs[0])
        assigned_to = None
        parent = getattr(node, "parent", None)
        if isinstance(parent, ast.Assign) and parent.value == node:
            for t in parent.targets:
                if isinstance(t, ast.Name):
                    for d in self.all_definitions:
                        if d.kind != SymbolKind.Variable:
                            continue
                        if d.location.line == parent.lineno - 1 and d.name == t.id:
                            assigned_to = d.symbol_id
                            break
                break
        self.references.append(
            SymbolReference(
                target_symbol=target_sym,
                location=self._loc(node),
                enclosing_symbol=enc,
                role=ReferenceRole.Call,
                receiver=receiver_sym,
                method_name=method_name,
                assigned_to=assigned_to,
            )
        )
        self.generic_visit(node)

    def visit_Name(self, node: ast.Name) -> None:
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        if isinstance(node.ctx, ast.Load):
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if target_sym:
                for d in self.all_definitions:
                    if d.symbol_id == target_sym and d.kind == SymbolKind.Variable:
                        self.references.append(
                            SymbolReference(
                                target_symbol=target_sym,
                                location=self._loc(node),
                                enclosing_symbol=enc,
                                role=ReferenceRole.Read,
                                receiver=None,
                                method_name=None,
                                assigned_to=None,
                            )
                        )
                        break
        elif isinstance(node.ctx, ast.Store):
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if target_sym:
                for d in self.all_definitions:
                    if d.symbol_id == target_sym and d.kind == SymbolKind.Variable:
                        self.references.append(
                            SymbolReference(
                                target_symbol=target_sym,
                                location=self._loc(node),
                                enclosing_symbol=enc,
                                role=ReferenceRole.Write,
                                receiver=None,
                                method_name=None,
                                assigned_to=None,
                            )
                        )
                        break
        self.generic_visit(node)

    def visit_Attribute(self, node: ast.Attribute) -> None:
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        if isinstance(node.ctx, ast.Load):
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if target_sym:
                for d in self.all_definitions:
                    if d.symbol_id == target_sym and d.kind == SymbolKind.Variable:
                        receiver_sym = None
                        if isinstance(node.value, ast.Name):
                            rdefs = self._resolve_at(node.value.lineno, node.value.col_offset)
                            if rdefs:
                                receiver_sym = _definition_symbol_id_from_jedi(self.all_definitions, rdefs[0])
                        self.references.append(
                            SymbolReference(
                                target_symbol=target_sym,
                                location=self._loc(node),
                                enclosing_symbol=enc,
                                role=ReferenceRole.Read,
                                receiver=receiver_sym,
                                method_name=None,
                                assigned_to=None,
                            )
                        )
                        break
        elif isinstance(node.ctx, ast.Store):
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if target_sym:
                for d in self.all_definitions:
                    if d.symbol_id == target_sym and d.kind == SymbolKind.Variable:
                        receiver_sym = None
                        if isinstance(node.value, ast.Name):
                            rdefs = self._resolve_at(node.value.lineno, node.value.col_offset)
                            if rdefs:
                                receiver_sym = _definition_symbol_id_from_jedi(self.all_definitions, rdefs[0])
                        self.references.append(
                            SymbolReference(
                                target_symbol=target_sym,
                                location=self._loc(node),
                                enclosing_symbol=enc,
                                role=ReferenceRole.Write,
                                receiver=receiver_sym,
                                method_name=None,
                                assigned_to=None,
                            )
                        )
                        break
        self.generic_visit(node)


def collect_references(
    doc: DocumentSemantics,
    file_path: str,
    source: str,
    project_root: str,
    all_definitions: list[SymbolDefinition],
    module_symbol_id: str,
) -> list[SymbolReference]:
    """Run reference collection with Jedi on a single document."""
    abs_path = os.path.join(project_root, doc.relative_path)
    script = jedi.Script(source, path=abs_path)
    collector = ReferenceCollector(
        file_path=abs_path,
        source=source,
        project_root=project_root,
        all_definitions=all_definitions,
        module_symbol_id=module_symbol_id,
        script=script,
    )
    tree = ast.parse(source, filename=abs_path)
    _add_parents(tree)
    collector.visit(tree)
    return collector.references
