"""
Pass 2: Collect references (Call, Read, Write, Decorate) and use a pluggable
resolver backend to resolve target symbols where possible.
"""

from __future__ import annotations

import ast
import os
from dataclasses import dataclass, field
from typing import Optional

from .resolver_backend import DocumentResolver, ResolvedTarget
from .schema import (
    DocumentSemantics,
    FunctionDetails,
    Mutability,
    ReferenceRole,
    SourceLocation,
    SourceSpan,
    SymbolDefinition,
    SymbolKind,
    SymbolReference,
    TypeDetails,
    TypeKind,
    VariableDetails,
)


def _add_parents(tree: ast.AST) -> None:
    """Mutate AST to add .parent on each node for later reference."""
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            setattr(child, "parent", node)


@dataclass(slots=True)
class DefinitionIndex:
    by_symbol_id: dict[str, SymbolDefinition] = field(default_factory=dict)
    by_name: dict[str, list[SymbolDefinition]] = field(default_factory=dict)
    by_file_and_name: dict[tuple[str, str], list[SymbolDefinition]] = field(default_factory=dict)
    by_file_line_name: dict[tuple[str, int, str], SymbolDefinition] = field(default_factory=dict)
    variables_or_functions_by_name: dict[str, list[SymbolDefinition]] = field(default_factory=dict)
    variables_or_functions_by_file_name: dict[tuple[str, str], list[SymbolDefinition]] = field(
        default_factory=dict
    )
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


def _normalized_path(path: str | None) -> str | None:
    if not path:
        return None
    return path.replace("\\", "/")


def _definition_symbol_id_from_target(index: DefinitionIndex, resolved: ResolvedTarget | None) -> Optional[str]:
    """Map a resolved target to our symbol_id by matching file, line, and optional name."""
    if not resolved:
        return None

    j_path = _normalized_path(resolved.path)
    j_name = resolved.name or ""
    j_line_0 = resolved.line

    candidates: list[SymbolDefinition]
    if j_name:
        candidates = index.by_name.get(j_name, [])
    else:
        candidates = list(index.by_symbol_id.values())
    if not candidates:
        return None

    if j_path:
        candidates = [
            definition
            for definition in candidates
            if definition.location.file_path
            and (
                j_path.endswith(_normalized_path(definition.location.file_path) or "")
                or (_normalized_path(definition.location.file_path) or "") in j_path
            )
        ]
        if not candidates:
            return None

    exact_line = [definition for definition in candidates if definition.location.line == j_line_0]
    if exact_line:
        return exact_line[0].symbol_id

    containing = [
        definition
        for definition in candidates
        if definition.span.start_line <= j_line_0 <= definition.span.end_line
    ]
    if containing:
        return containing[0].symbol_id

    return None


def _full_name_from_target(resolved: ResolvedTarget | None) -> Optional[str]:
    if not resolved:
        return None
    return resolved.full_name or resolved.name


class ReferenceCollector(ast.NodeVisitor):
    """Collects references and uses the injected resolver to resolve targets."""

    def __init__(
        self,
        file_path: str,
        source: str,
        project_root: str,
        all_definitions: list[SymbolDefinition],
        definition_index: DefinitionIndex,
        module_symbol_id: str,
        resolver: DocumentResolver,
    ):
        self.file_path = file_path
        self.rel_path = os.path.relpath(file_path, project_root).replace("\\", "/")
        self.source = source
        self.project_root = project_root
        self.all_definitions = all_definitions
        self.definition_index = definition_index
        self.module_symbol_id = module_symbol_id
        self.resolver = resolver
        self.references: list[SymbolReference] = []
        self.external_symbols: dict[str, SymbolDefinition] = {}
        self._func_spans: list[tuple[int, int, str]] = []
        self._goto_cache: dict[tuple[int, int, bool], list[ResolvedTarget]] = {}
        self._enclosing_defined_cache: dict[str, Optional[str]] = {}
        self._class_prefix_cache: dict[str, Optional[str]] = {}
        self.tree: Optional[ast.AST] = None

    def _kind_rank(self, kind: SymbolKind) -> int:
        if kind == SymbolKind.Variable:
            return 0
        if kind == SymbolKind.Function:
            return 1
        return 2

    def _collect_external_symbol(
        self,
        target_sym: str,
        resolved: ResolvedTarget,
        *,
        kind_hint: SymbolKind = SymbolKind.Variable,
    ) -> None:
        name = resolved.name or target_sym.rsplit(".", 1)[-1]
        if not name:
            return

        file_path = resolved.path or "unknown"
        line = max(0, resolved.line)
        column = max(0, resolved.column)
        loc = SourceLocation(file_path=file_path, line=line, column=column)
        span = SourceSpan(
            start_line=line,
            start_column=column,
            end_line=line + 1,
            end_column=column,
        )

        kind = kind_hint
        details: FunctionDetails | VariableDetails | TypeDetails
        if kind == SymbolKind.Type:
            details = TypeDetails(kind=TypeKind.Class)
        elif kind == SymbolKind.Function:
            details = FunctionDetails()
        else:
            details = VariableDetails(mutability=Mutability.Immutable)

        existing = self.external_symbols.get(target_sym)
        if existing and self._kind_rank(existing.kind) >= self._kind_rank(kind):
            return

        self.external_symbols[target_sym] = SymbolDefinition(
            symbol_id=target_sym,
            kind=kind,
            name=name,
            display_name=resolved.full_name or name,
            location=loc,
            span=span,
            enclosing_symbol=None,
            is_external=True,
            documentation=resolved.documentation,
            details=details,
        )

    def _resolve_symbol_from_targets_with_hint(
        self,
        defs: list[ResolvedTarget],
        *,
        kind_hint: SymbolKind,
    ) -> Optional[str]:
        if not defs:
            return None

        target_sym = _definition_symbol_id_from_target(self.definition_index, defs[0])
        if not target_sym:
            target_sym = _full_name_from_target(defs[0])
            if target_sym and self._should_externalize(defs[0]):
                self._collect_external_symbol(target_sym, defs[0], kind_hint=kind_hint)
            else:
                target_sym = None
        return target_sym

    def _should_externalize(self, resolved: ResolvedTarget) -> bool:
        if resolved.kind in {"param", "statement"}:
            return False
        if resolved.path:
            abs_path = os.path.abspath(resolved.path)
            try:
                common = os.path.commonpath([self.project_root, abs_path])
            except ValueError:
                common = None
            if common == self.project_root:
                return False
        return bool(resolved.full_name or resolved.name)

    def _resolve_symbol_at_with_hint(
        self,
        line: int,
        column: int,
        *,
        follow_imports: bool = False,
        kind_hint: SymbolKind,
    ) -> Optional[str]:
        return self._resolve_symbol_from_targets_with_hint(
            self._resolve_at(line, column, follow_imports=follow_imports),
            kind_hint=kind_hint,
        )

    def _resolve_internal_symbol_from_targets(self, defs: list[ResolvedTarget]) -> Optional[str]:
        if not defs:
            return None
        return _definition_symbol_id_from_target(self.definition_index, defs[0])

    def _resolve_internal_symbol_at(
        self,
        line: int,
        column: int,
        *,
        follow_imports: bool = False,
    ) -> Optional[str]:
        return self._resolve_internal_symbol_from_targets(
            self._resolve_at(line, column, follow_imports=follow_imports)
        )

    def _definition(self, symbol_id: str | None) -> Optional[SymbolDefinition]:
        if not symbol_id:
            return None
        return self.definition_index.by_symbol_id.get(symbol_id) or self.external_symbols.get(symbol_id)

    def _is_internal_kind(self, symbol_id: str | None, kind: SymbolKind) -> bool:
        definition = self._definition(symbol_id)
        return bool(definition and definition.kind == kind and not definition.is_external)

    def _is_external_symbol(self, symbol_id: str | None) -> bool:
        definition = self._definition(symbol_id)
        return bool(definition and definition.is_external)

    def _is_read_target(self, symbol_id: str | None) -> bool:
        return (
            self._is_internal_kind(symbol_id, SymbolKind.Variable)
            or self._is_internal_kind(symbol_id, SymbolKind.Function)
            or self._is_external_symbol(symbol_id)
        )

    def _is_write_target(self, symbol_id: str | None) -> bool:
        return self._is_internal_kind(symbol_id, SymbolKind.Variable) or self._is_external_symbol(symbol_id)

    def _type_candidates(self, name: str) -> list[SymbolDefinition]:
        return self.definition_index.types_by_name.get(name, [])

    def _find_type_for_enclosing_symbol(self, enc: str) -> Optional[str]:
        cached = self._class_prefix_cache.get(enc)
        if cached is not None:
            return cached

        for definition in self.definition_index.type_by_symbol_id.values():
            if enc.startswith(definition.symbol_id + "."):
                self._class_prefix_cache[enc] = definition.symbol_id
                return definition.symbol_id

        self._class_prefix_cache[enc] = None
        return None

    def _enclosing_symbol(self, node: ast.AST) -> Optional[str]:
        line_1 = node.lineno
        for start, end, sym_id in reversed(self._func_spans):
            if start <= line_1 <= end:
                return sym_id
        return self.module_symbol_id

    def _enclosing_symbol_defined(self, node: ast.AST) -> Optional[str]:
        enc = self._enclosing_symbol(node)
        if enc in self._enclosing_defined_cache:
            return self._enclosing_defined_cache[enc]

        while enc and "." in enc:
            if enc in self.definition_index.by_symbol_id:
                self._enclosing_defined_cache[self._enclosing_symbol(node)] = enc
                return enc
            enc = enc.rsplit(".", 1)[0]
        result = enc if (enc and enc in self.definition_index.by_symbol_id) else (
            self.module_symbol_id if self.module_symbol_id in self.definition_index.by_symbol_id else enc
        )
        self._enclosing_defined_cache[self._enclosing_symbol(node)] = result
        return result

    def _loc(self, node: ast.AST) -> SourceLocation:
        return SourceLocation(
            file_path=self.rel_path,
            line=node.lineno - 1,
            column=getattr(node, "col_offset", 0) or 0,
        )

    def _prefer_same_module_variable(self, target_sym: str, bare_name: str) -> str:
        if target_sym in self.definition_index.by_symbol_id:
            return target_sym
        same_module = [
            definition
            for definition in self.definition_index.variables_or_functions_by_name.get(bare_name, [])
            if definition.kind == SymbolKind.Variable
            and (
                definition.symbol_id == f"{self.module_symbol_id}.{bare_name}"
                or definition.symbol_id.startswith(self.module_symbol_id + ".")
            )
        ]
        if len(same_module) == 1:
            return same_module[0].symbol_id
        return target_sym

    def _same_module_variable_symbol(self, bare_name: str) -> Optional[str]:
        same_module = [
            definition
            for definition in self.definition_index.variables_or_functions_by_name.get(bare_name, [])
            if definition.kind == SymbolKind.Variable
            and (
                definition.symbol_id == f"{self.module_symbol_id}.{bare_name}"
                or definition.symbol_id.startswith(self.module_symbol_id + ".")
            )
        ]
        if len(same_module) == 1:
            return same_module[0].symbol_id
        return None

    def _resolve_at(self, line: int, column: int, *, follow_imports: bool = False) -> list[ResolvedTarget]:
        cache_key = (line, column, follow_imports)
        if cache_key in self._goto_cache:
            return self._goto_cache[cache_key]
        resolved = self.resolver.goto(line, column, follow_imports=follow_imports)
        self._goto_cache[cache_key] = resolved
        return resolved

    def _is_inside_default_value(self, node: ast.AST) -> bool:
        n = node
        while getattr(n, "parent", None) is not None:
            par = n.parent
            if isinstance(par, ast.arguments):
                for d in par.defaults or []:
                    if any(n is x for x in ast.walk(d)):
                        return True
                for k in par.kw_defaults or []:
                    if k is not None and any(n is x for x in ast.walk(k)):
                        return True
                return False
            n = par
        return False

    def _collect_read_ref_from_expr(self, expr: ast.AST, enclosing_symbol: str) -> None:
        if isinstance(expr, ast.Name) and isinstance(expr.ctx, ast.Load):
            target_sym = self._resolve_name_to_read_target(expr)
            if target_sym:
                self.references.append(
                    SymbolReference(
                        target_symbol=target_sym,
                        location=self._loc(expr),
                        enclosing_symbol=enclosing_symbol,
                        role=ReferenceRole.Read,
                        receiver=None,
                        method_name=None,
                        assigned_to=None,
                    )
                )
            return
        if isinstance(expr, ast.Attribute) and isinstance(expr.ctx, ast.Load):
            target_sym = self._resolve_symbol_at_with_hint(
                expr.lineno,
                expr.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym and self._is_read_target(target_sym):
                receiver_sym = None
                if isinstance(expr.value, ast.Name):
                    receiver_sym = self._resolve_internal_symbol_at(expr.value.lineno, expr.value.col_offset)
                self.references.append(
                    SymbolReference(
                        target_symbol=target_sym,
                        location=self._loc(expr),
                        enclosing_symbol=enclosing_symbol,
                        role=ReferenceRole.Read,
                        receiver=receiver_sym,
                        method_name=None,
                        assigned_to=None,
                    )
                )
            return
        for child in ast.iter_child_nodes(expr):
            self._collect_read_ref_from_expr(child, enclosing_symbol)

    def _resolve_name_to_read_target(self, node: ast.Name) -> Optional[str]:
        target_sym = self._resolve_symbol_at_with_hint(
            node.lineno,
            node.col_offset,
            kind_hint=SymbolKind.Variable,
        )
        if target_sym and self._is_read_target(target_sym):
            return target_sym

        same_file = self.definition_index.variables_or_functions_by_file_name.get((self.rel_path, node.id), [])
        if len(same_file) == 1:
            return same_file[0].symbol_id
        if same_file:
            return same_file[0].symbol_id

        by_name = self.definition_index.variables_or_functions_by_name.get(node.id, [])
        if len(by_name) == 1:
            return by_name[0].symbol_id
        if len(by_name) > 1 and self.tree is not None:
            narrowed = self._narrow_by_import(node.id, by_name)
            if narrowed:
                return narrowed
        return None

    def _resolve_import_module_prefix(self, node: ast.ImportFrom) -> Optional[str]:
        if node.level == 0:
            return node.module
        module_path = self.rel_path.replace("\\", "/")
        if module_path.endswith(".py"):
            module_path = module_path[:-3]
        parts = module_path.split("/")
        if node.level > len(parts):
            return None
        parent_parts = parts[: len(parts) - node.level]
        parent_module = ".".join(parent_parts)
        if node.module:
            return f"{parent_module}.{node.module}" if parent_module else node.module
        return parent_module

    def _narrow_by_import(self, name: str, candidates: list[SymbolDefinition]) -> Optional[str]:
        if self.tree is None:
            return None
        for imp_node in ast.walk(self.tree):
            if not isinstance(imp_node, ast.ImportFrom):
                continue
            for alias in imp_node.names:
                imported_as = alias.asname or alias.name
                if imported_as != name:
                    continue
                prefix = self._resolve_import_module_prefix(imp_node)
                if not prefix:
                    continue
                original_name = alias.name
                matched = [
                    candidate
                    for candidate in candidates
                    if candidate.name == original_name
                    and (
                        candidate.symbol_id.startswith(prefix + ".")
                        or candidate.symbol_id == prefix
                    )
                ]
                if len(matched) == 1:
                    return matched[0].symbol_id
        return None

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        enc = self._enclosing_symbol(node)
        func_id = f"{enc}.{node.name}"
        exact = self.definition_index.by_symbol_id.get(func_id)
        if not exact:
            for definition in self.definition_index.by_file_and_name.get((self.rel_path, node.name), []):
                if definition.location.line != node.lineno - 1:
                    continue
                if definition.symbol_id.count(".") >= func_id.count("."):
                    func_id = definition.symbol_id
                break
        start = node.lineno
        end = getattr(node, "end_lineno", node.lineno)
        self._func_spans.append((start, end, func_id))
        for dec in node.decorator_list:
            self._visit_decorator(dec, func_id)
        for default in node.args.defaults or []:
            self._collect_read_ref_from_expr(default, func_id)
        for kw_default in node.args.kw_defaults or []:
            if kw_default is not None:
                self._collect_read_ref_from_expr(kw_default, func_id)
        self.generic_visit(node)
        self._func_spans.pop()

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.visit_FunctionDef(node)

    def _visit_decorator(self, node: ast.expr, decorated_symbol_id: str) -> None:
        target_sym = None
        if isinstance(node, ast.Name):
            target_sym = self._resolve_symbol_at_with_hint(
                node.lineno,
                node.col_offset,
                kind_hint=SymbolKind.Function,
            )
        elif isinstance(node, ast.Attribute):
            target_sym = self._resolve_symbol_at_with_hint(
                node.lineno,
                node.col_offset,
                kind_hint=SymbolKind.Function,
            )
        if target_sym:
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

    def visit_ExceptHandler(self, node: ast.ExceptHandler) -> None:
        enc = self._enclosing_symbol(node)
        if enc and node.type:

            def _extract_type(t_node: ast.AST) -> None:
                if isinstance(t_node, (ast.Name, ast.Attribute)):
                    target_sym = self._resolve_symbol_at_with_hint(
                        t_node.lineno,
                        t_node.col_offset,
                        kind_hint=SymbolKind.Type,
                    )
                    if target_sym:
                        self.references.append(
                            SymbolReference(
                                target_symbol=target_sym,
                                location=self._loc(t_node),
                                enclosing_symbol=self._enclosing_symbol_defined(node),
                                role=ReferenceRole.Read,
                                receiver=None,
                                method_name=None,
                                assigned_to=None,
                            )
                        )
                elif isinstance(t_node, ast.Tuple):
                    for elt in t_node.elts:
                        _extract_type(elt)

            _extract_type(node.type)
        self.generic_visit(node)

    def _resolve_super_call_target(self, enc: str, method_name: str) -> Optional[str]:
        parts = enc.split(".")
        if len(parts) < 2:
            return None
        class_symbol_id = ".".join(parts[:-1])
        type_def = self.definition_index.type_by_symbol_id.get(class_symbol_id)
        if not type_def or not isinstance(type_def.details, TypeDetails):
            return None
        bases = type_def.details.inherits
        if not bases:
            return None
        base_ref = bases[0]
        base_name = base_ref.split(".")[-1].strip()
        class_module_prefix = class_symbol_id.rsplit(".", 1)[0] if "." in class_symbol_id else ""
        base_full = f"{class_module_prefix}.{base_name}" if class_module_prefix else base_name
        base_type = next(
            (
                definition
                for definition in self._type_candidates(base_name)
                if definition.symbol_id == base_full
                or (not class_module_prefix or definition.symbol_id.startswith(class_module_prefix + "."))
            ),
            None,
        )
        if not base_type:
            cross_module = self._type_candidates(base_name)
            if len(cross_module) == 1:
                base_type = cross_module[0]
        if not base_type and self.tree is not None and type_def.location.file_path == self.rel_path:
            class_line = type_def.location.line
            class_def_node = next(
                (
                    n for n in ast.walk(self.tree)
                    if isinstance(n, ast.ClassDef) and n.lineno - 1 == class_line
                ),
                None,
            )
            if class_def_node:
                for base_node in class_def_node.bases:
                    node_str = ast.unparse(base_node)
                    if node_str == base_ref or node_str.split(".")[-1] == base_name:
                        defs = self._resolve_at(base_node.lineno, base_node.col_offset, follow_imports=True)
                        if defs:
                            canon_id = _definition_symbol_id_from_target(self.definition_index, defs[0])
                            if canon_id:
                                base_type = self.definition_index.type_by_symbol_id.get(canon_id)
                                if base_type:
                                    break
                            if not base_type and defs[0].name:
                                jedi_name = defs[0].name
                                base_type = next(
                                    (
                                        definition
                                        for definition in self._type_candidates(jedi_name)
                                        if definition.symbol_id != class_symbol_id
                                    ),
                                    None,
                                )
                                if base_type:
                                    break
        if not base_type:
            return None
        return f"{base_type.symbol_id}.{method_name}"

    def visit_Call(self, node: ast.Call) -> None:
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        target_sym = None
        receiver_sym = None
        method_name = None

        if (
            isinstance(node.func, ast.Attribute)
            and isinstance(node.func.value, ast.Call)
            and isinstance(node.func.value.func, ast.Name)
            and node.func.value.func.id == "super"
        ):
            method_name = node.func.attr
            target_sym = self._resolve_super_call_target(enc, method_name)

        if target_sym is None and isinstance(node.func, ast.Name):
            if node.func.id == "cls" and "." in enc:
                class_symbol_id = ".".join(enc.split(".")[:-1])
                target_sym = f"{class_symbol_id}.__init__"
            else:
                target_sym = self._resolve_symbol_at_with_hint(
                    node.func.lineno,
                    node.func.col_offset,
                    kind_hint=SymbolKind.Function,
                )
        elif target_sym is None and isinstance(node.func, ast.Attribute):
            method_name = node.func.attr
            target_sym = self._resolve_symbol_at_with_hint(
                node.func.lineno,
                node.func.col_offset,
                kind_hint=SymbolKind.Function,
            )

            if not target_sym and isinstance(node.func.value, ast.Name):
                receiver_name = node.func.value.id
                enc_def = self.definition_index.by_symbol_id.get(enc)
                if enc_def and enc_def.kind == SymbolKind.Function and hasattr(enc_def.details, "parameters"):
                    param = next((p for p in enc_def.details.parameters if p.name == receiver_name), None)
                    if param and param.param_type:
                        ptype = param.param_type.split("[")[0].strip()
                        ptype_short = ptype.split(".")[-1]
                        type_def = next(iter(self._type_candidates(ptype_short)), None)
                        if type_def:
                            target_sym = f"{type_def.symbol_id}.{method_name}"
                        elif ptype:
                            target_sym = f"{ptype}.{method_name}"
                    elif receiver_name == "self" and "." in enc:
                        class_prefix = self._find_type_for_enclosing_symbol(enc)
                        if class_prefix:
                            target_sym = f"{class_prefix}.{method_name}"
                        else:
                            parts = enc.split(".")
                            enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                            target_sym = f"{enclosing_class}.{method_name}"
                    elif receiver_name == "cls" and "." in enc:
                        class_prefix = self._find_type_for_enclosing_symbol(enc)
                        if class_prefix:
                            target_sym = f"{class_prefix}.{method_name}"
                        else:
                            parts = enc.split(".")
                            enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                            target_sym = f"{enclosing_class}.{method_name}"
                elif receiver_name == "self" and "." in enc:
                    class_prefix = self._find_type_for_enclosing_symbol(enc)
                    if class_prefix:
                        target_sym = f"{class_prefix}.{method_name}"
                    else:
                        parts = enc.split(".")
                        enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                        target_sym = f"{enclosing_class}.{method_name}"
                elif receiver_name == "cls" and "." in enc:
                    class_prefix = self._find_type_for_enclosing_symbol(enc)
                    if class_prefix:
                        target_sym = f"{class_prefix}.{method_name}"
                    else:
                        parts = enc.split(".")
                        enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                        target_sym = f"{enclosing_class}.{method_name}"

            if (
                target_sym
                and "." not in target_sym
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id in ("self", "cls")
                and enc
            ):
                class_prefix = self._find_type_for_enclosing_symbol(enc)
                if class_prefix:
                    target_sym = f"{class_prefix}.{method_name}"
                else:
                    parts = enc.split(".")
                    if len(parts) >= 2:
                        enclosing_class = ".".join(parts[:-1])
                        target_sym = f"{enclosing_class}.{method_name}"

            if isinstance(node.func.value, ast.Name):
                receiver_sym = self._resolve_internal_symbol_at(
                    node.func.value.lineno,
                    node.func.value.col_offset,
                )

        assigned_to = None
        parent = getattr(node, "parent", None)
        if isinstance(parent, ast.Assign) and parent.value == node:
            for t in parent.targets:
                if isinstance(t, ast.Name):
                    definition = self.definition_index.by_file_line_name.get((self.rel_path, parent.lineno - 1, t.id))
                    if definition and definition.kind == SymbolKind.Variable:
                        assigned_to = definition.symbol_id
                break
        self.references.append(
            SymbolReference(
                target_symbol=target_sym,
                location=self._loc(node),
                enclosing_symbol=self._enclosing_symbol_defined(node),
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
        if self._is_inside_default_value(node):
            self.generic_visit(node)
            return
        if isinstance(node.ctx, ast.Load):
            target_sym = self._resolve_symbol_at_with_hint(
                node.lineno,
                node.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym and self._is_read_target(target_sym):
                self.references.append(
                    SymbolReference(
                        target_symbol=target_sym,
                        location=self._loc(node),
                        enclosing_symbol=self._enclosing_symbol_defined(node),
                        role=ReferenceRole.Read,
                        receiver=None,
                        method_name=None,
                        assigned_to=None,
                    )
                )
        elif isinstance(node.ctx, ast.Store):
            target_sym = self._resolve_symbol_at_with_hint(
                node.lineno,
                node.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym and self._is_write_target(target_sym):
                self.references.append(
                    SymbolReference(
                        target_symbol=target_sym,
                        location=self._loc(node),
                        enclosing_symbol=self._enclosing_symbol_defined(node),
                        role=ReferenceRole.Write,
                        receiver=None,
                        method_name=None,
                        assigned_to=None,
                    )
                )
        self.generic_visit(node)

    def visit_AugAssign(self, node: ast.AugAssign) -> None:
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        target_sym = None
        receiver_sym = None
        if isinstance(node.target, ast.Name):
            target_sym = self._resolve_symbol_at_with_hint(
                node.target.lineno,
                node.target.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym:
                target_sym = self._prefer_same_module_variable(target_sym, node.target.id)
            else:
                target_sym = self._same_module_variable_symbol(node.target.id)
        elif isinstance(node.target, ast.Attribute):
            target_sym = self._resolve_symbol_at_with_hint(
                node.target.lineno,
                node.target.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym:
                target_sym = self._prefer_same_module_variable(target_sym, node.target.attr)
            if isinstance(node.target.value, ast.Name):
                receiver_sym = self._resolve_internal_symbol_at(
                    node.target.value.lineno,
                    node.target.value.col_offset,
                )
        if target_sym and self._is_write_target(target_sym):
            loc = self._loc(node.target)
            enc_def = self._enclosing_symbol_defined(node)
            self.references.append(
                SymbolReference(
                    target_symbol=target_sym,
                    location=loc,
                    enclosing_symbol=enc_def,
                    role=ReferenceRole.Read,
                    receiver=receiver_sym,
                    method_name=None,
                    assigned_to=None,
                )
            )
            self.references.append(
                SymbolReference(
                    target_symbol=target_sym,
                    location=loc,
                    enclosing_symbol=enc_def,
                    role=ReferenceRole.Write,
                    receiver=receiver_sym,
                    method_name=None,
                    assigned_to=None,
                )
            )
        self.generic_visit(node)

    def visit_Attribute(self, node: ast.Attribute) -> None:
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        if self._is_inside_default_value(node):
            self.generic_visit(node)
            return
        if isinstance(node.ctx, ast.Load):
            target_sym = self._resolve_symbol_at_with_hint(
                node.lineno,
                node.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym and self._is_read_target(target_sym):
                receiver_sym = None
                if isinstance(node.value, ast.Name):
                    receiver_sym = self._resolve_internal_symbol_at(node.value.lineno, node.value.col_offset)
                self.references.append(
                    SymbolReference(
                        target_symbol=target_sym,
                        location=self._loc(node),
                        enclosing_symbol=self._enclosing_symbol_defined(node),
                        role=ReferenceRole.Read,
                        receiver=receiver_sym,
                        method_name=None,
                        assigned_to=None,
                    )
                )
        elif isinstance(node.ctx, ast.Store):
            target_sym = self._resolve_symbol_at_with_hint(
                node.lineno,
                node.col_offset,
                kind_hint=SymbolKind.Variable,
            )
            if target_sym and self._is_write_target(target_sym):
                receiver_sym = None
                if isinstance(node.value, ast.Name):
                    receiver_sym = self._resolve_internal_symbol_at(node.value.lineno, node.value.col_offset)
                self.references.append(
                    SymbolReference(
                        target_symbol=target_sym,
                        location=self._loc(node),
                        enclosing_symbol=self._enclosing_symbol_defined(node),
                        role=ReferenceRole.Write,
                        receiver=receiver_sym,
                        method_name=None,
                        assigned_to=None,
                    )
                )
        self.generic_visit(node)


def collect_references(
    doc: DocumentSemantics,
    file_path: str,
    source: str,
    project_root: str,
    all_definitions: list[SymbolDefinition],
    module_symbol_id: str,
    resolver: DocumentResolver,
) -> tuple[list[SymbolReference], list[SymbolDefinition]]:
    """Run reference collection with the provided resolver on a single document."""
    abs_path = os.path.join(project_root, doc.relative_path)
    collector = ReferenceCollector(
        file_path=abs_path,
        source=source,
        project_root=project_root,
        all_definitions=all_definitions,
        definition_index=DefinitionIndex.build(all_definitions),
        module_symbol_id=module_symbol_id,
        resolver=resolver,
    )
    tree = ast.parse(source, filename=abs_path)
    _add_parents(tree)
    collector.tree = tree
    collector.visit(tree)
    return collector.references, list(collector.external_symbols.values())
