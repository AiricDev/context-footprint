"""
Pass 2: Collect references (Call, Read, Write, Decorate) and use Jedi to resolve
target_symbol where possible. Fallback to receiver + method_name for builder recovery.
"""

import ast
import os
from pathlib import Path
from typing import Any, Optional

import jedi
from jedi.api.classes import Name


def _add_parents(tree: ast.AST) -> None:
    """Mutate AST to add .parent on each node for later reference."""
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            setattr(child, "parent", node)

from .schema import (
    DocumentSemantics,
    FunctionDetails,
    Mutability,
    Parameter,
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


def _definition_symbol_id_from_jedi(definitions: list[SymbolDefinition], jedi_def: Name | None) -> Optional[str]:
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


def _full_name_from_jedi(jedi_def: Name | None) -> Optional[str]:
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
        self.external_symbols: list[SymbolDefinition] = []
        self._func_spans: list[tuple[int, int, str]] = []
        # Stored AST for cross-method inspection (alias resolution, import analysis)
        self.tree: Optional[ast.AST] = None

    def _collect_external_symbol(self, target_sym: str, jedi_def: Any) -> None:
        if any(d.symbol_id == target_sym for d in self.external_symbols):
            return

        name = jedi_def.name
        if not name:
            return

        doc = []
        try:
            doc_str = jedi_def.docstring()
            if doc_str:
                doc.append(doc_str)
        except Exception:
            pass

        file_path = str(jedi_def.module_path) if jedi_def.module_path else "unknown"
        # Make path relative to project root if possible to keep it clean, but absolute is fine for external
        
        line = max(0, (jedi_def.line or 1) - 1)
        column = jedi_def.column or 0
        loc = SourceLocation(file_path=file_path, line=line, column=column)
        
        span = SourceSpan(
            start_line=line,
            start_column=column,
            end_line=line + 1,
            end_column=column,
        )

        kind = SymbolKind.Function
        details = None

        if jedi_def.type == "class":
            kind = SymbolKind.Type
            details = TypeDetails(kind=TypeKind.Class)
        elif jedi_def.type == "function":
            kind = SymbolKind.Function
            params = []
            return_types = []
            try:
                sigs = jedi_def.get_signatures()
                if sigs:
                    sig = sigs[0]
                    sig_str = sig.to_string()
                    if "->" in sig_str:
                        ret = sig_str.rsplit("->", 1)[-1].strip()
                        if ret:
                            return_types.append(ret)
                    for p in sig.params:
                        p_name = p.name
                        p_type = None
                        if p.description and ":" in p.description:
                            p_type = p.description.split(":", 1)[-1].strip()
                        params.append(Parameter(name=p_name, param_type=p_type))
            except Exception:
                pass
            details = FunctionDetails(parameters=params, return_types=return_types)
        else:
            kind = SymbolKind.Variable
            details = VariableDetails(mutability=Mutability.Immutable)

        self.external_symbols.append(
            SymbolDefinition(
                symbol_id=target_sym,
                kind=kind,
                name=name,
                display_name=jedi_def.full_name or name,
                location=loc,
                span=span,
                enclosing_symbol=None,
                is_external=True,
                documentation=doc,
                details=details,
            )
        )

    def _enclosing_symbol(self, node: ast.AST) -> Optional[str]:
        line_1 = node.lineno
        for start, end, sym_id in reversed(self._func_spans):
            if start <= line_1 <= end:
                return sym_id
        return self.module_symbol_id

    def _enclosing_symbol_defined(self, node: ast.AST) -> Optional[str]:
        """Return the closest enclosing symbol that exists in all_definitions and is a
        top-level unit (module, class, or class/module method). References inside nested
        functions (e.g. api_route.decorator) are attributed to the outer method (api_route)
        so the graph can create edges from the method node."""
        enc = self._enclosing_symbol(node)
        defined_ids = {d.symbol_id for d in self.all_definitions}
        # Strip until we have a defined symbol that is at most module.class.method (3 segments)
        while enc and "." in enc:
            # We want to match what was extracted as definitions. If it's a nested function,
            # it might not be in defined_ids. If it's a method or class, it should be.
            # Only consider it defined if it's in defined_ids and it's a module, class, or method (<= 3 segments if it's module.class.method, but module could be a.b.c so we can't count segments simply anymore).
            # The definition extractor doesn't collect nested functions.
            if enc in defined_ids:
                return enc
            enc = enc.rsplit(".", 1)[0]
        result = enc if (enc and enc in defined_ids) else (
            self.module_symbol_id if self.module_symbol_id in defined_ids else enc
        )
        return result

    def _loc(self, node: ast.AST) -> SourceLocation:
        return SourceLocation(
            file_path=self.rel_path,
            line=node.lineno - 1,
            column=getattr(node, "col_offset", 0) or 0,
        )

    def _prefer_same_module_variable(self, target_sym: str, bare_name: str) -> str:
        """If target_sym is from Jedi full_name and not in our definitions, try to map to
        the same-module Variable by bare name (e.g. Jedi '...increment.counter' -> module.counter)."""
        defined_ids = {d.symbol_id for d in self.all_definitions}
        if target_sym in defined_ids:
            return target_sym
        same_module = [
            d
            for d in self.all_definitions
            if d.kind == SymbolKind.Variable
            and d.name == bare_name
            and (d.symbol_id == f"{self.module_symbol_id}.{bare_name}" or d.symbol_id.startswith(self.module_symbol_id + "."))
        ]
        if len(same_module) == 1:
            return same_module[0].symbol_id
        return target_sym

    def _resolve_at(self, line: int, column: int) -> list[Name]:
        """Resolve the symbol at (line, column) to its definition (goto definition)."""
        try:
            return self.script.goto(line, column)
        except Exception:
            return []

    def _is_inside_default_value(self, node: ast.AST) -> bool:
        """True if node is inside a default or kw_default expression of an arguments node."""
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

    def _collect_read_ref_from_expr(
        self, expr: ast.AST, enclosing_symbol: str
    ) -> None:
        """Emit Read references for variable names used in an expression (e.g. default arg).
        Jedi may not resolve in default-argument context; fallback to same-file variable by name.
        """
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
            defs = self._resolve_at(expr.lineno, expr.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                is_var = any(
                    d.symbol_id == target_sym and d.kind == SymbolKind.Variable
                    for d in self.all_definitions
                )
                is_func = any(
                    d.symbol_id == target_sym and d.kind == SymbolKind.Function
                    for d in self.all_definitions
                )
                is_ext = any(
                    d.symbol_id == target_sym and d.is_external
                    for d in self.external_symbols
                )
                if is_var or is_func or is_ext:
                    receiver_sym = None
                    if isinstance(expr.value, ast.Name):
                        rdefs = self._resolve_at(expr.value.lineno, expr.value.col_offset)
                        if rdefs:
                            receiver_sym = _definition_symbol_id_from_jedi(
                                self.all_definitions, rdefs[0]
                            )
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
        """Resolve a Name (Load) to a Variable or Function symbol_id for Read reference.
        Tries Jedi first, then same-file/cross-file fallback. Used for default args and body.
        """
        defs = self._resolve_at(node.lineno, node.col_offset)
        target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
        if not target_sym and defs:
            target_sym = _full_name_from_jedi(defs[0])
            if target_sym:
                self._collect_external_symbol(target_sym, defs[0])
        if target_sym:
            is_var = any(
                d.symbol_id == target_sym and d.kind == SymbolKind.Variable
                for d in self.all_definitions
            )
            is_func = any(
                d.symbol_id == target_sym and d.kind == SymbolKind.Function
                for d in self.all_definitions
            )
            is_ext = any(
                d.symbol_id == target_sym and d.is_external
                for d in self.external_symbols
            )
            if is_var or is_func or is_ext:
                return target_sym
        # Fallback: same-file Variable or Function with same name (Jedi often fails in default-arg context)
        same_file = [
            d for d in self.all_definitions
            if d.kind in (SymbolKind.Variable, SymbolKind.Function)
            and d.name == node.id
            and d.location.file_path == self.rel_path
        ]
        if len(same_file) == 1:
            return same_file[0].symbol_id
        if same_file:
            return same_file[0].symbol_id
        by_name = [
            d for d in self.all_definitions
            if d.kind in (SymbolKind.Variable, SymbolKind.Function) and d.name == node.id
        ]
        if len(by_name) == 1:
            return by_name[0].symbol_id
        # Import-context disambiguation: multiple same-named definitions — use imports to narrow
        if len(by_name) > 1 and self.tree is not None:
            narrowed = self._narrow_by_import(node.id, by_name)
            if narrowed:
                return narrowed
        return None

    def _resolve_import_module_prefix(self, node: ast.ImportFrom) -> Optional[str]:
        """Convert an ImportFrom node (level + module) to an absolute module symbol_id prefix."""
        if node.level == 0:
            return node.module  # absolute import
        # Relative import: go up `level` package levels from the current file's module path
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

    def _narrow_by_import(
        self, name: str, candidates: list[SymbolDefinition]
    ) -> Optional[str]:
        """Given multiple same-named candidates, use import statements in the current file
        to select the one that was actually imported here.
        Returns the unique matching symbol_id, or None if ambiguous / not found.
        """
        if self.tree is None:
            return None
        for imp_node in ast.walk(self.tree):
            if not isinstance(imp_node, ast.ImportFrom):
                continue
            for alias in imp_node.names:
                # alias.asname is the local name (if `import X as Y`); alias.name is the original
                imported_as = alias.asname or alias.name
                if imported_as != name:
                    continue
                prefix = self._resolve_import_module_prefix(imp_node)
                if not prefix:
                    continue
                original_name = alias.name
                matched = [
                    c
                    for c in candidates
                    if c.name == original_name
                    and (
                        c.symbol_id.startswith(prefix + ".")
                        or c.symbol_id == prefix
                    )
                ]
                if len(matched) == 1:
                    return matched[0].symbol_id
        return None

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        enc = self._enclosing_symbol(node)
        func_id = f"{enc}.{node.name}"
        for d in self.all_definitions:
            if d.symbol_id == func_id:
                break
            if d.name == node.name and d.location.file_path == self.rel_path and d.location.line == node.lineno - 1:
                # Use definition's symbol_id when it has same or more segments (e.g. class method).
                # When we have more segments (nested function: api_route.decorator), keep our func_id
                # so references attribute to the outer method; extractor uses class prefix (APIRouter.decorator).
                if d.symbol_id.count(".") >= func_id.count("."):
                    func_id = d.symbol_id
                break
        start = node.lineno
        end = getattr(node, "end_lineno", node.lineno)
        self._func_spans.append((start, end, func_id))
        for dec in node.decorator_list:
            self._visit_decorator(dec, func_id)
        # Explicitly collect references from default argument values (Jedi often misses them).
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
        enc = self._enclosing_symbol(node)
        target_sym = None
        if isinstance(node, ast.Name):
            defs = self._resolve_at(node.lineno, node.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
        elif isinstance(node, ast.Attribute):
            defs = self._resolve_at(node.lineno, node.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
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
            def _extract_type(t_node: ast.AST):
                if isinstance(t_node, (ast.Name, ast.Attribute)):
                    defs = self._resolve_at(t_node.lineno, t_node.col_offset)
                    target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
                    if not target_sym and defs:
                        target_sym = _full_name_from_jedi(defs[0])
                        if target_sym:
                            self._collect_external_symbol(target_sym, defs[0])
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
        """Resolve super().method() to parent class method. Returns target symbol_id or None."""
        # enc is e.g. "module.Child.__init__"; class is "module.Child"
        parts = enc.split(".")
        if len(parts) < 2:
            return None
        class_symbol_id = ".".join(parts[:-1])
        type_def = next(
            (d for d in self.all_definitions if d.kind == SymbolKind.Type and d.symbol_id == class_symbol_id),
            None,
        )
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
                d
                for d in self.all_definitions
                if d.kind == SymbolKind.Type
                and (d.symbol_id == base_full or (d.name == base_name and (not class_module_prefix or d.symbol_id.startswith(class_module_prefix + "."))))
            ),
            None,
        )
        # Cross-module: base may be in another module; only pick when the name is unique project-wide
        # to avoid choosing the wrong class when multiple types share the same short name.
        if not base_type:
            cross_module = [
                d for d in self.all_definitions
                if d.kind == SymbolKind.Type and d.name == base_name
            ]
            if len(cross_module) == 1:
                base_type = cross_module[0]
        # Alias fallback: base_name is an import alias (e.g. 'SansioBlueprint').
        # Walk the AST to find the ClassDef and use Jedi to resolve the actual base type.
        if not base_type and self.tree is not None and type_def.location.file_path == self.rel_path:
            class_line = type_def.location.line  # 0-indexed
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
                        # Use follow_imports=True so Jedi resolves import aliases to their
                        # actual class definitions (e.g. 'SansioBlueprint' → Blueprint)
                        try:
                            defs = self.script.goto(
                                base_node.lineno, base_node.col_offset, follow_imports=True
                            )
                        except Exception:
                            defs = []
                        if defs:
                            canon_id = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
                            if canon_id:
                                base_type = next(
                                    (
                                        d for d in self.all_definitions
                                        if d.symbol_id == canon_id and d.kind == SymbolKind.Type
                                    ),
                                    None,
                                )
                                if base_type:
                                    break
                            # Secondary fallback: match by the name Jedi resolved to
                            if not base_type and defs[0].name:
                                jedi_name = defs[0].name
                                base_type = next(
                                    (
                                        d for d in self.all_definitions
                                        if d.kind == SymbolKind.Type
                                        and d.name == jedi_name
                                        and d.symbol_id != class_symbol_id
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
        # Detect super().method(...) and resolve to parent class method
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
                defs = self._resolve_at(node.func.lineno, node.func.col_offset)
                if defs:
                    target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
                if not target_sym and defs:
                    target_sym = _full_name_from_jedi(defs[0])
                    if target_sym:
                        self._collect_external_symbol(target_sym, defs[0])
        elif target_sym is None and isinstance(node.func, ast.Attribute):
            method_name = node.func.attr
            defs = self._resolve_at(node.func.lineno, node.func.col_offset)
            if defs:
                target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0])
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
                    
            if not target_sym and isinstance(node.func.value, ast.Name):
                receiver_name = node.func.value.id
                enc_def = next((d for d in self.all_definitions if d.symbol_id == enc), None)
                if enc_def and enc_def.kind == SymbolKind.Function and hasattr(enc_def.details, "parameters"):
                    param = next((p for p in enc_def.details.parameters if p.name == receiver_name), None)
                    if param and param.param_type:
                        ptype = param.param_type.split("[")[0].strip()
                        ptype_short = ptype.split(".")[-1]
                        type_def = next((d for d in self.all_definitions if d.kind == SymbolKind.Type and d.name == ptype_short), None)
                        if type_def:
                            method_sym_id = f"{type_def.symbol_id}.{method_name}"
                            target_sym = method_sym_id
                        elif ptype:
                            target_sym = f"{ptype}.{method_name}"
                    elif receiver_name == "self" and "." in enc:
                        parts = enc.split(".")
                        class_prefix = None
                        for d in self.all_definitions:
                            if d.kind == SymbolKind.Type and enc.startswith(d.symbol_id + "."):
                                class_prefix = d.symbol_id
                                break
                        if class_prefix:
                            target_sym = f"{class_prefix}.{method_name}"
                        else:
                            enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                            target_sym = f"{enclosing_class}.{method_name}"
                    elif receiver_name == "cls" and "." in enc:
                        parts = enc.split(".")
                        class_prefix = None
                        for d in self.all_definitions:
                            if d.kind == SymbolKind.Type and enc.startswith(d.symbol_id + "."):
                                class_prefix = d.symbol_id
                                break
                        if class_prefix:
                            target_sym = f"{class_prefix}.{method_name}"
                        else:
                            enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                            target_sym = f"{enclosing_class}.{method_name}"
                # When enc not in definitions (e.g. nested function api_route.decorator), still resolve self/cls.method
                elif receiver_name == "self" and "." in enc:
                    parts = enc.split(".")
                    class_prefix = None
                    for d in self.all_definitions:
                        if d.kind == SymbolKind.Type and enc.startswith(d.symbol_id + "."):
                            class_prefix = d.symbol_id
                            break
                    if class_prefix:
                        target_sym = f"{class_prefix}.{method_name}"
                    else:
                        enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                        target_sym = f"{enclosing_class}.{method_name}"
                elif receiver_name == "cls" and "." in enc:
                    parts = enc.split(".")
                    class_prefix = None
                    for d in self.all_definitions:
                        if d.kind == SymbolKind.Type and enc.startswith(d.symbol_id + "."):
                            class_prefix = d.symbol_id
                            break
                    if class_prefix:
                        target_sym = f"{class_prefix}.{method_name}"
                    else:
                        enclosing_class = ".".join(parts[:-1]) if len(parts) >= 2 else enc
                        target_sym = f"{enclosing_class}.{method_name}"
            # Qualify bare method name from Jedi with class when receiver is self or cls
            if (
                target_sym
                and "." not in target_sym
                and isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id in ("self", "cls")
                and (enc or "")
            ):
                parts = enc.split(".")
                class_prefix = None
                for d in self.all_definitions:
                    if d.kind == SymbolKind.Type and enc.startswith(d.symbol_id + "."):
                        class_prefix = d.symbol_id
                        break
                if class_prefix:
                    target_sym = f"{class_prefix}.{method_name}"
                else:
                    if len(parts) >= 2:
                        enclosing_class = ".".join(parts[:-1])
                        target_sym = f"{enclosing_class}.{method_name}"

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
        # Default-argument references are collected in visit_FunctionDef; skip to avoid duplicates.
        if self._is_inside_default_value(node):
            self.generic_visit(node)
            return
        if isinstance(node.ctx, ast.Load):
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                # Emit Read for Variable (e.g. CONFIG) or Function (e.g. handler as value).
                is_var = any(d.symbol_id == target_sym and d.kind == SymbolKind.Variable for d in self.all_definitions)
                is_func = any(d.symbol_id == target_sym and d.kind == SymbolKind.Function for d in self.all_definitions)
                is_ext = any(d.symbol_id == target_sym and d.is_external for d in self.external_symbols)
                if is_var or is_func or is_ext:
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
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                is_var = any(d.symbol_id == target_sym and d.kind == SymbolKind.Variable for d in self.all_definitions)
                is_ext = any(d.symbol_id == target_sym and d.is_external for d in self.external_symbols)
                if is_var or is_ext:
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
        """Emit both Read and Write for x += 1 (reader needs current value and write)."""
        enc = self._enclosing_symbol(node)
        if not enc:
            self.generic_visit(node)
            return
        target_sym = None
        receiver_sym = None
        if isinstance(node.target, ast.Name):
            defs = self._resolve_at(node.target.lineno, node.target.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                target_sym = self._prefer_same_module_variable(target_sym, node.target.id)
        elif isinstance(node.target, ast.Attribute):
            defs = self._resolve_at(node.target.lineno, node.target.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                target_sym = self._prefer_same_module_variable(target_sym, node.target.attr)
            if isinstance(node.target.value, ast.Name):
                rdefs = self._resolve_at(node.target.value.lineno, node.target.value.col_offset)
                if rdefs:
                    receiver_sym = _definition_symbol_id_from_jedi(self.all_definitions, rdefs[0])
        if target_sym:
            is_var = any(d.symbol_id == target_sym and d.kind == SymbolKind.Variable for d in self.all_definitions)
            is_ext = any(d.symbol_id == target_sym and d.is_external for d in self.external_symbols)
            if is_var or is_ext:
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
        # Default-argument references are collected in visit_FunctionDef; skip to avoid duplicates.
        if self._is_inside_default_value(node):
            self.generic_visit(node)
            return
        if isinstance(node.ctx, ast.Load):
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                is_var = any(d.symbol_id == target_sym and d.kind == SymbolKind.Variable for d in self.all_definitions)
                is_func = any(d.symbol_id == target_sym and d.kind == SymbolKind.Function for d in self.all_definitions)
                is_ext = any(d.symbol_id == target_sym and d.is_external for d in self.external_symbols)
                if is_var or is_func or is_ext:
                    receiver_sym = None
                    if isinstance(node.value, ast.Name):
                        rdefs = self._resolve_at(node.value.lineno, node.value.col_offset)
                        if rdefs:
                            receiver_sym = _definition_symbol_id_from_jedi(self.all_definitions, rdefs[0])
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
            defs = self._resolve_at(node.lineno, node.col_offset)
            target_sym = _definition_symbol_id_from_jedi(self.all_definitions, defs[0]) if defs else None
            if not target_sym and defs:
                target_sym = _full_name_from_jedi(defs[0])
                if target_sym:
                    self._collect_external_symbol(target_sym, defs[0])
            if target_sym:
                is_var = any(d.symbol_id == target_sym and d.kind == SymbolKind.Variable for d in self.all_definitions)
                is_ext = any(d.symbol_id == target_sym and d.is_external for d in self.external_symbols)
                if is_var or is_ext:
                    receiver_sym = None
                    if isinstance(node.value, ast.Name):
                        rdefs = self._resolve_at(node.value.lineno, node.value.col_offset)
                        if rdefs:
                            receiver_sym = _definition_symbol_id_from_jedi(self.all_definitions, rdefs[0])
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
    environment: Optional[Any] = None,
) -> tuple[list[SymbolReference], list[SymbolDefinition]]:
    """Run reference collection with Jedi on a single document. Reuse one environment for all files to avoid too many open FDs."""
    abs_path = os.path.join(project_root, doc.relative_path)
    script = jedi.Script(source, path=abs_path, environment=environment)
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
    collector.tree = tree
    collector.visit(tree)
    return collector.references, collector.external_symbols
