#!/usr/bin/env python3
"""
Extract SemanticData from Python codebase.

This script parses Python source files and extracts semantic information
in the format expected by the CF graph builder.

Usage:
    python extract_python_semantics.py <project_root> [--output <file.json>]

Requirements:
    - Python 3.10+
    - No external dependencies (uses stdlib only for initial version)
    - Optional: pyright/mypy for type inference (future enhancement)
"""

import ast
import json
import sys
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional, List, Dict, Set
from enum import Enum


# ============================================================================
# Data Models (matching Rust SemanticData)
# ============================================================================

class SymbolKind(Enum):
    FUNCTION = "Function"
    VARIABLE = "Variable"
    TYPE = "Type"


class VariableScope(Enum):
    GLOBAL = "Global"
    FIELD = "Field"


class Mutability(Enum):
    CONST = "Const"
    IMMUTABLE = "Immutable"
    MUTABLE = "Mutable"


class Visibility(Enum):
    PUBLIC = "Public"
    PRIVATE = "Private"


class TypeKind(Enum):
    CLASS = "Class"
    INTERFACE = "Interface"  # Protocol in Python


class ReferenceRole(Enum):
    CALL = "Call"
    READ = "Read"
    WRITE = "Write"
    TYPE_ANNOTATION = "TypeAnnotation"
    DECORATOR = "Decorator"


@dataclass
class SourceLocation:
    file_path: str
    line: int  # 0-indexed
    column: int  # 0-indexed


@dataclass
class SourceSpan:
    start_line: int
    start_column: int
    end_line: int
    end_column: int


@dataclass
class Parameter:
    name: str
    param_type: Optional[str] = None
    has_default: bool = False
    is_variadic: bool = False


@dataclass
class TypeParam:
    name: str
    bounds: List[str] = field(default_factory=list)


@dataclass
class FunctionModifiers:
    is_async: bool = False
    is_generator: bool = False
    is_static: bool = False
    is_abstract: bool = False
    visibility: str = "Public"


@dataclass
class FunctionDetails:
    parameters: List[Parameter] = field(default_factory=list)
    return_types: List[str] = field(default_factory=list)
    type_params: List[TypeParam] = field(default_factory=list)
    modifiers: FunctionModifiers = field(default_factory=FunctionModifiers)


@dataclass
class VariableDetails:
    var_type: Optional[str] = None
    mutability: str = "Mutable"
    scope: str = "Global"
    visibility: str = "Public"


@dataclass
class Field:
    name: str
    field_type: Optional[str] = None
    mutability: str = "Mutable"
    visibility: str = "Public"
    symbol_id: str = ""


@dataclass
class TypeDetails:
    kind: str = "Class"
    is_abstract: bool = False
    is_final: bool = False
    visibility: str = "Public"
    type_params: List[TypeParam] = field(default_factory=list)
    fields: List[Field] = field(default_factory=list)
    inherits: List[str] = field(default_factory=list)
    implements: List[str] = field(default_factory=list)


@dataclass
class SymbolDefinition:
    symbol_id: str
    kind: str
    name: str
    display_name: str
    location: SourceLocation
    span: SourceSpan
    enclosing_symbol: Optional[str]
    is_external: bool
    documentation: List[str]
    details: Dict  # Will be FunctionDetails, VariableDetails, or TypeDetails
    
    def to_dict(self):
        """Convert to dict with Rust serde-compatible tagged union format."""
        d = asdict(self)
        # Convert details to tagged union format
        # Rust expects: {"Function": {...}} instead of {"kind": "Function", ...}
        if d['kind'] == 'Function':
            details_inner = d['details']
            d['details'] = {'Function': details_inner}
        elif d['kind'] == 'Variable':
            details_inner = d['details']
            d['details'] = {'Variable': details_inner}
        elif d['kind'] == 'Type':
            details_inner = d['details']
            d['details'] = {'Type': details_inner}
        return d


@dataclass
class SymbolReference:
    target_symbol: str
    location: SourceLocation
    enclosing_symbol: str
    role: str
    receiver: Optional[str] = None


@dataclass
class DocumentSemantics:
    relative_path: str
    language: str
    definitions: List[SymbolDefinition]
    references: List[SymbolReference]


@dataclass
class SemanticData:
    project_root: str
    documents: List[DocumentSemantics]
    external_symbols: List[SymbolDefinition]


# ============================================================================
# Python AST Extractor
# ============================================================================

class PythonSemanticExtractor(ast.NodeVisitor):
    """Extract semantic information from Python AST."""
    
    def __init__(self, file_path: Path, project_root: Path):
        self.file_path = file_path
        self.project_root = project_root
        self.relative_path = str(file_path.relative_to(project_root))
        
        self.definitions: List[SymbolDefinition] = []
        self.references: List[SymbolReference] = []
        
        # Track context
        self.current_class: Optional[str] = None
        self.current_function: Optional[str] = None
        self.symbol_counter = 0
        self.module_level = True  # Track if we're at module level
        
        # Read source code for docstrings
        self.source_lines = file_path.read_text().splitlines()
    
    def generate_symbol_id(self, name: str, kind: str) -> str:
        """Generate a unique symbol ID."""
        module = self.relative_path.replace('/', '.').replace('.py', '')
        parts = [module]
        
        if self.current_class:
            # Extract class name from full symbol_id
            class_name = self.current_class.split('#')[0].split('.')[-1]
            parts.append(class_name)
        
        parts.append(name)
        
        return '.'.join(parts) + f"#{kind}"
    
    def get_docstring(self, node: ast.AST) -> List[str]:
        """Extract docstring from a node."""
        docstring = ast.get_docstring(node)
        return [docstring] if docstring else []
    
    def get_span(self, node: ast.AST) -> SourceSpan:
        """Get source span for a node."""
        return SourceSpan(
            start_line=node.lineno - 1,  # Convert to 0-indexed
            start_column=node.col_offset,
            end_line=node.end_lineno - 1 if node.end_lineno else node.lineno - 1,
            end_column=node.end_col_offset if node.end_col_offset else node.col_offset
        )
    
    def visit_Assign(self, node: ast.Assign) -> None:
        """Visit assignment (for global variables)."""
        # Only process module-level assignments (global variables)
        if self.module_level and not self.current_class and not self.current_function:
            for target in node.targets:
                if isinstance(target, ast.Name):
                    var_name = target.id
                    symbol_id = self.generate_symbol_id(var_name, "Variable")
                    
                    # Determine mutability based on naming
                    if var_name.isupper():
                        mutability = "Const"
                    elif var_name.startswith("_"):
                        mutability = "Immutable"
                    else:
                        mutability = "Mutable"
                    
                    var_details = VariableDetails(
                        var_type=None,  # TODO: Infer from value
                        mutability=mutability,
                        scope="Global",
                        visibility=self.get_visibility(var_name)
                    )
                    
                    definition = SymbolDefinition(
                        symbol_id=symbol_id,
                        kind=SymbolKind.VARIABLE.value,
                        name=var_name,
                        display_name=var_name,
                        location=SourceLocation(
                            file_path=self.relative_path,
                            line=node.lineno - 1,
                            column=node.col_offset
                        ),
                        span=self.get_span(node),
                        enclosing_symbol=None,
                        is_external=False,
                        documentation=[],
                        details=asdict(var_details)
                    )
                    
                    self.definitions.append(definition)
        
        self.generic_visit(node)
    
    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        """Visit class definition."""
        self.module_level = False  # Entering class scope
        symbol_id = self.generate_symbol_id(node.name, "Type")
        
        # Determine if it's a Protocol (interface)
        is_abstract = any(
            isinstance(base, ast.Name) and base.id == "Protocol"
            for base in node.bases
        )
        
        # Extract base classes
        inherits = []
        implements = []
        for base in node.bases:
            if isinstance(base, ast.Name):
                base_name = base.id
                if base_name == "Protocol":
                    continue
                if is_abstract:
                    implements.append(base_name)
                else:
                    inherits.append(base_name)
        
        # Extract fields (instance variables in __init__)
        fields = []
        for item in node.body:
            if isinstance(item, ast.FunctionDef) and item.name == "__init__":
                fields = self.extract_fields_from_init(item, symbol_id)
                break
        
        type_details = TypeDetails(
            kind="Interface" if is_abstract else "Class",
            is_abstract=is_abstract,
            is_final=False,
            visibility=self.get_visibility(node.name),
            fields=fields,
            inherits=inherits,
            implements=implements
        )
        
        definition = SymbolDefinition(
            symbol_id=symbol_id,
            kind=SymbolKind.TYPE.value,
            name=node.name,
            display_name=node.name,
            location=SourceLocation(
                file_path=self.relative_path,
                line=node.lineno - 1,
                column=node.col_offset
            ),
            span=self.get_span(node),
            enclosing_symbol=None,
            is_external=False,
            documentation=self.get_docstring(node),
            details=asdict(type_details)
        )
        
        self.definitions.append(definition)
        
        # Visit class body with updated context
        prev_class = self.current_class
        self.current_class = symbol_id
        self.generic_visit(node)
        self.current_class = prev_class
        self.module_level = True  # Back to module scope
    
    def extract_fields_from_init(self, init_node: ast.FunctionDef, class_symbol: str) -> List[Field]:
        """Extract instance variables from __init__ method."""
        fields = []
        seen_fields = set()
        
        for stmt in ast.walk(init_node):
            if isinstance(stmt, ast.Assign):
                for target in stmt.targets:
                    if isinstance(target, ast.Attribute):
                        if isinstance(target.value, ast.Name) and target.value.id == "self":
                            field_name = target.attr
                            if field_name in seen_fields:
                                continue
                            seen_fields.add(field_name)
                            
                            # Generate proper symbol ID
                            class_name = class_symbol.split('#')[0].split('.')[-1]
                            module = '.'.join(class_symbol.split('#')[0].split('.')[:-1])
                            field_symbol_id = f"{module}.{class_name}.{field_name}#Variable"
                            
                            # Try to extract type from annotation
                            field_type = None
                            # TODO: Extract from type comments or infer
                            
                            field = Field(
                                name=field_name,
                                field_type=field_type,
                                mutability="Mutable",
                                visibility=self.get_visibility(field_name),
                                symbol_id=field_symbol_id
                            )
                            fields.append(field)
                            
                            # Also create a Variable definition for this field
                            var_def = SymbolDefinition(
                                symbol_id=field_symbol_id,
                                kind=SymbolKind.VARIABLE.value,
                                name=field_name,
                                display_name=field_name,
                                location=SourceLocation(
                                    file_path=self.relative_path,
                                    line=stmt.lineno - 1 if hasattr(stmt, 'lineno') else 0,
                                    column=stmt.col_offset if hasattr(stmt, 'col_offset') else 0
                                ),
                                span=self.get_span(stmt) if hasattr(stmt, 'lineno') else SourceSpan(0, 0, 0, 0),
                                enclosing_symbol=class_symbol,
                                is_external=False,
                                documentation=[],
                                details=asdict(VariableDetails(
                                    var_type=field_type,
                                    mutability="Mutable",
                                    scope="Field",
                                    visibility=self.get_visibility(field_name)
                                ))
                            )
                            self.definitions.append(var_def)
        
        return fields
    
    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        """Visit function/method definition."""
        self.visit_function_common(node, is_async=False)
    
    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        """Visit async function definition."""
        self.visit_function_common(node, is_async=True)
    
    def visit_function_common(self, node: ast.FunctionDef | ast.AsyncFunctionDef, is_async: bool) -> None:
        """Common logic for function/method visiting."""
        symbol_id = self.generate_symbol_id(node.name, "Function")
        
        # Extract parameters
        parameters = []
        for arg in node.args.args:
            if arg.arg == "self" or arg.arg == "cls":
                continue  # Skip receiver parameter
            
            param_type = None
            if arg.annotation:
                param_type = self.annotation_to_string(arg.annotation)
            
            parameters.append(Parameter(
                name=arg.arg,
                param_type=param_type,
                has_default=False,  # TODO: Check defaults
                is_variadic=False
            ))
        
        # Extract return type
        return_types = []
        if node.returns:
            ret_type = self.annotation_to_string(node.returns)
            if ret_type:
                return_types.append(ret_type)
        
        # Determine modifiers
        is_static = any(
            isinstance(d, ast.Name) and d.id == "staticmethod"
            for d in node.decorator_list
        )
        
        is_abstract = any(
            isinstance(d, ast.Name) and d.id == "abstractmethod"
            for d in node.decorator_list
        )
        
        is_generator = any(
            isinstance(n, ast.Yield) or isinstance(n, ast.YieldFrom)
            for n in ast.walk(node)
        )
        
        modifiers = FunctionModifiers(
            is_async=is_async,
            is_generator=is_generator,
            is_static=is_static,
            is_abstract=is_abstract,
            visibility=self.get_visibility(node.name)
        )
        
        func_details = FunctionDetails(
            parameters=parameters,
            return_types=return_types,
            modifiers=modifiers
        )
        
        definition = SymbolDefinition(
            symbol_id=symbol_id,
            kind=SymbolKind.FUNCTION.value,
            name=node.name,
            display_name=f"{node.name}({', '.join(p.name for p in parameters)})",
            location=SourceLocation(
                file_path=self.relative_path,
                line=node.lineno - 1,
                column=node.col_offset
            ),
            span=self.get_span(node),
            enclosing_symbol=self.current_class,
            is_external=False,
            documentation=self.get_docstring(node),
            details=asdict(func_details)
        )
        
        self.definitions.append(definition)
        
        # Track references in function body
        prev_function = self.current_function
        self.current_function = symbol_id
        self.generic_visit(node)
        self.current_function = prev_function
    
    def visit_Call(self, node: ast.Call) -> None:
        """Visit function call to create reference."""
        if isinstance(node.func, ast.Name):
            # Direct call: foo()
            self.references.append(SymbolReference(
                target_symbol=node.func.id,  # TODO: Resolve to full symbol
                location=SourceLocation(
                    file_path=self.relative_path,
                    line=node.lineno - 1,
                    column=node.col_offset
                ),
                enclosing_symbol=self.current_function or "__module__",
                role=ReferenceRole.CALL.value,
                receiver=None
            ))
        elif isinstance(node.func, ast.Attribute):
            # Method call: obj.method()
            receiver = None
            if isinstance(node.func.value, ast.Name):
                receiver = node.func.value.id
            
            self.references.append(SymbolReference(
                target_symbol=node.func.attr,  # TODO: Resolve to full symbol
                location=SourceLocation(
                    file_path=self.relative_path,
                    line=node.lineno - 1,
                    column=node.col_offset
                ),
                enclosing_symbol=self.current_function or "__module__",
                role=ReferenceRole.CALL.value,
                receiver=receiver
            ))
        
        self.generic_visit(node)
    
    def annotation_to_string(self, annotation: ast.AST) -> Optional[str]:
        """Convert type annotation AST to string."""
        if isinstance(annotation, ast.Name):
            return annotation.id
        elif isinstance(annotation, ast.Constant):
            return str(annotation.value)
        elif isinstance(annotation, ast.Subscript):
            # Generic type like List[int]
            base = self.annotation_to_string(annotation.value)
            arg = self.annotation_to_string(annotation.slice)
            return f"{base}[{arg}]" if base and arg else base
        else:
            # Fallback: use ast.unparse if available (Python 3.9+)
            try:
                return ast.unparse(annotation)
            except:
                return None
    
    def get_visibility(self, name: str) -> str:
        """Determine visibility from naming convention."""
        if name.startswith("__") and not name.endswith("__"):
            return Visibility.PRIVATE.value
        elif name.startswith("_"):
            return Visibility.PRIVATE.value
        else:
            return Visibility.PUBLIC.value


# ============================================================================
# Main Extraction Logic
# ============================================================================

def extract_from_file(file_path: Path, project_root: Path) -> DocumentSemantics:
    """Extract semantics from a single Python file."""
    source = file_path.read_text()
    tree = ast.parse(source, filename=str(file_path))
    
    extractor = PythonSemanticExtractor(file_path, project_root)
    extractor.visit(tree)
    
    return DocumentSemantics(
        relative_path=str(file_path.relative_to(project_root)),
        language="python",
        definitions=extractor.definitions,
        references=extractor.references
    )


def extract_from_project(project_root: Path) -> SemanticData:
    """Extract semantics from entire Python project."""
    documents = []
    
    # Find all Python files
    for py_file in project_root.rglob("*.py"):
        # Skip test files and virtual environments
        if "test" in py_file.parts or "venv" in py_file.parts or ".venv" in py_file.parts:
            continue
        
        try:
            doc = extract_from_file(py_file, project_root)
            documents.append(doc)
        except Exception as e:
            print(f"Error processing {py_file}: {e}", file=sys.stderr)
    
    return SemanticData(
        project_root=str(project_root.absolute()),
        documents=documents,
        external_symbols=[]  # TODO: Extract from imports
    )


# ============================================================================
# CLI
# ============================================================================

def main():
    if len(sys.argv) < 2:
        print("Usage: extract_python_semantics.py <project_root> [--output <file.json>]")
        sys.exit(1)
    
    project_root = Path(sys.argv[1])
    output_file = None
    
    if "--output" in sys.argv:
        idx = sys.argv.index("--output")
        if idx + 1 < len(sys.argv):
            output_file = Path(sys.argv[idx + 1])
    
    print(f"Extracting semantics from {project_root}...", file=sys.stderr)
    semantic_data = extract_from_project(project_root)
    
    # Convert to JSON with proper tagged union format
    output = {
        'project_root': semantic_data.project_root,
        'documents': [
            {
                'relative_path': doc.relative_path,
                'language': doc.language,
                'definitions': [defn.to_dict() for defn in doc.definitions],
                'references': [asdict(ref) for ref in doc.references]
            }
            for doc in semantic_data.documents
        ],
        'external_symbols': [defn.to_dict() for defn in semantic_data.external_symbols]
    }
    json_str = json.dumps(output, indent=2)
    
    if output_file:
        output_file.write_text(json_str)
        print(f"Wrote semantics to {output_file}", file=sys.stderr)
    else:
        print(json_str)


if __name__ == "__main__":
    main()
