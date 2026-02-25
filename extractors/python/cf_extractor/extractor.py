"""
Pass 1: AST-based extraction of definitions (Types, Functions, Variables).
Only module-level and class-level symbols; no local variables.
"""

import ast
import os
from pathlib import Path
from typing import Optional

from .schema import (
    DocumentSemantics,
    FunctionDetails,
    FunctionModifiers,
    Mutability,
    Parameter,
    SourceLocation,
    SourceSpan,
    SymbolDefinition,
    SymbolKind,
    TypeDetails,
    TypeKind,
    VariableDetails,
    VariableScope,
    Visibility,
)


def _visibility_from_name(name: str) -> Visibility:
    if name.startswith("__") and not name.endswith("__"):
        return Visibility.Private
    if name.startswith("_"):
        return Visibility.Private
    return Visibility.Public


def _span_from_node(node: ast.AST, source_lines: list[str]) -> SourceSpan:
    """Build 0-based inclusive start, exclusive end span. AST uses 1-based lines."""
    start_line = node.lineno - 1
    start_col = getattr(node, "col_offset", 0) or 0
    end_lineno = getattr(node, "end_lineno", node.lineno)
    end_col = getattr(node, "end_col_offset", start_col) or start_col
    return SourceSpan(
        start_line=start_line,
        start_column=start_col,
        end_line=end_lineno,
        end_column=end_col,
    )


def _location_from_node(node: ast.AST, file_path: str) -> SourceLocation:
    return SourceLocation(
        file_path=file_path,
        line=node.lineno - 1,
        column=getattr(node, "col_offset", 0) or 0,
    )


def _get_docstring(node: ast.AST) -> list[str]:
    doc = ast.get_docstring(node)
    if doc:
        return [doc]
    return []


def _annotation_to_typeref(annotation: Optional[ast.expr]) -> Optional[str]:
    if annotation is None:
        return None
    if isinstance(annotation, ast.Constant):
        return str(annotation.value) if annotation.value is not None else None
    if isinstance(annotation, ast.Name):
        return annotation.id
    if isinstance(annotation, ast.Subscript):
        base = annotation.value
        if isinstance(base, ast.Name):
            return base.id
        return None
    if isinstance(annotation, ast.BinOp) and isinstance(annotation.op, ast.BitOr):
        return None
    return None


class DefinitionCollector(ast.NodeVisitor):
    """Collects Type, Function, and Variable definitions from a single file."""

    def __init__(self, file_path: str, source: str, module_symbol_id: str):
        self.file_path = file_path
        self.source = source
        self.source_lines = source.splitlines()
        self.module_symbol_id = module_symbol_id
        self.definitions: list[SymbolDefinition] = []
        self._class_stack: list[tuple[str, str]] = []

    def _current_scope_prefix(self) -> str:
        if not self._class_stack:
            return self.module_symbol_id
        return self._class_stack[-1][1]

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        type_id = f"{self._current_scope_prefix()}.{node.name}"
        bases: list[str] = []
        for b in node.bases:
            ref = _annotation_to_typeref(b)
            if ref:
                bases.append(ref)

        is_abstract = any(
            ast.get_source_segment(self.source, n) == "abstractmethod"
            for dec in node.decorator_list
            for n in [dec] if isinstance(dec, ast.Name)
        ) or (ast.get_source_segment(self.source, node) or "").find("ABC") >= 0

        type_details = TypeDetails(
            kind=TypeKind.Class,
            is_abstract=is_abstract,
            is_final=False,
            visibility=_visibility_from_name(node.name),
            type_params=[],
            fields=[],
            inherits=bases,
            implements=[],
        )
        span = _span_from_node(node, self.source_lines)
        loc = _location_from_node(node, self.file_path)
        self.definitions.append(
            SymbolDefinition(
                symbol_id=type_id,
                kind=SymbolKind.Type,
                name=node.name,
                display_name=node.name,
                location=loc,
                span=span,
                enclosing_symbol=None if not self._class_stack else self._class_stack[-1][1],
                is_external=False,
                documentation=_get_docstring(node),
                details=type_details,
            )
        )
        self._class_stack.append((node.name, type_id))
        self.generic_visit(node)
        self._class_stack.pop()

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self._visit_function_def(node, is_async=False)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self._visit_function_def(node, is_async=True)

    def _visit_function_def(
        self, node: ast.FunctionDef | ast.AsyncFunctionDef, is_async: bool
    ) -> None:
        prefix = self._current_scope_prefix()
        func_id = f"{prefix}.{node.name}"
        params: list[Parameter] = []
        num_args = len(node.args.args)
        num_defaults = len(node.args.defaults) if node.args.defaults else 0
        def_start = num_args - num_defaults
        vararg_arg = getattr(node.args.vararg, "arg", None) if node.args.vararg else None
        for i, arg in enumerate(node.args.args):
            if arg.arg in ("self", "cls"):
                continue
            param_type = _annotation_to_typeref(getattr(arg, "annotation", None))
            has_default = i >= def_start
            is_variadic = arg.arg == vararg_arg
            params.append(
                Parameter(
                    name=arg.arg,
                    param_type=param_type,
                    has_default=has_default,
                    is_variadic=is_variadic,
                )
            )
        return_ann = getattr(node, "returns", None)
        return_types = []
        if return_ann:
            rt = _annotation_to_typeref(return_ann)
            if rt:
                return_types.append(rt)
        is_abstract = any(
            (isinstance(d, ast.Name) and d.id == "abstractmethod")
            or (isinstance(d, ast.Attribute) and getattr(d, "attr", None) == "abstractmethod")
            for d in node.decorator_list
        )
        is_static = any(
            isinstance(d, ast.Name) and d.id == "staticmethod"
            for d in node.decorator_list
        )
        is_classmethod = any(
            isinstance(d, ast.Name) and d.id == "classmethod"
            for d in node.decorator_list
        )
        is_constructor = node.name == "__init__"
        modifiers = FunctionModifiers(
            is_async=is_async,
            is_generator=any(isinstance(n, ast.Yield) for n in ast.walk(node)),
            is_static=is_static or is_classmethod,
            is_abstract=is_abstract,
            is_constructor=is_constructor,
            is_di_wired=False,
            visibility=_visibility_from_name(node.name),
        )
        span = _span_from_node(node, self.source_lines)
        loc = _location_from_node(node, self.file_path)
        enclosing = self._class_stack[-1][1] if self._class_stack else None
        self.definitions.append(
            SymbolDefinition(
                symbol_id=func_id,
                kind=SymbolKind.Function,
                name=node.name,
                display_name=node.name,
                location=loc,
                span=span,
                enclosing_symbol=enclosing,
                is_external=False,
                documentation=_get_docstring(node),
                details=FunctionDetails(
                    parameters=params,
                    return_types=return_types,
                    type_params=[],
                    modifiers=modifiers,
                ),
            )
        )
        self.generic_visit(node)

    def visit_Assign(self, node: ast.Assign) -> None:
        for target in node.targets:
            if isinstance(target, ast.Name):
                if self._class_stack:
                    scope = VariableScope.Field
                    prefix = self._class_stack[-1][1]
                    sym_id = f"{prefix}.{target.id}"
                    enclosing = prefix
                else:
                    scope = VariableScope.Global
                    sym_id = f"{self.module_symbol_id}.{target.id}"
                    enclosing = None
                span = _span_from_node(node, self.source_lines)
                loc = _location_from_node(node, self.file_path)
                self.definitions.append(
                    SymbolDefinition(
                        symbol_id=sym_id,
                        kind=SymbolKind.Variable,
                        name=target.id,
                        display_name=target.id,
                        location=loc,
                        span=span,
                        enclosing_symbol=enclosing,
                        is_external=False,
                        documentation=[],
                        details=VariableDetails(
                            var_type=None,
                            mutability=Mutability.Mutable,
                            scope=scope,
                            visibility=_visibility_from_name(target.id),
                        ),
                    )
                )
        self.generic_visit(node)

    def visit_AnnAssign(self, node: ast.AnnAssign) -> None:
        if isinstance(node.target, ast.Name):
            var_type = _annotation_to_typeref(node.annotation)
            if self._class_stack:
                scope = VariableScope.Field
                prefix = self._class_stack[-1][1]
                sym_id = f"{prefix}.{node.target.id}"
                enclosing = prefix
            else:
                scope = VariableScope.Global
                sym_id = f"{self.module_symbol_id}.{node.target.id}"
                enclosing = None
            span = _span_from_node(node, self.source_lines)
            loc = _location_from_node(node, self.file_path)
            self.definitions.append(
                SymbolDefinition(
                    symbol_id=sym_id,
                    kind=SymbolKind.Variable,
                    name=node.target.id,
                    display_name=node.target.id,
                    location=loc,
                    span=span,
                    enclosing_symbol=enclosing,
                    is_external=False,
                    documentation=[],
                    details=VariableDetails(
                        var_type=var_type,
                        mutability=Mutability.Mutable,
                        scope=scope,
                        visibility=_visibility_from_name(node.target.id),
                    ),
                )
            )
        self.generic_visit(node)


def extract_definitions_from_file(
    file_path: str, source: str, project_root: str
) -> DocumentSemantics:
    """Parse one Python file and return its definitions (no references yet)."""
    rel_path = os.path.relpath(file_path, project_root).replace("\\", "/")
    module_name = Path(rel_path).with_suffix("").as_posix().replace("/", ".")
    if module_name == ".":
        module_name = "__main__"
    module_symbol_id = module_name
    tree = ast.parse(source, filename=file_path)
    collector = DefinitionCollector(file_path=rel_path, source=source, module_symbol_id=module_symbol_id)
    collector.visit(tree)
    return DocumentSemantics(
        relative_path=rel_path,
        language="python",
        definitions=collector.definitions,
        references=[],
    )
