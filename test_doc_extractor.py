import ast
from typing import Optional

def _extract_doc_from_annotation(node: ast.expr) -> list[str]:
    docs = []
    for child in ast.walk(node):
        if isinstance(child, ast.Call):
            func_id = None
            if isinstance(child.func, ast.Name):
                func_id = child.func.id
            elif isinstance(child.func, ast.Attribute):
                func_id = child.func.attr
            if func_id == "Doc" and child.args:
                arg = child.args[0]
                if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                    docs.append(arg.value)
    return docs

def _get_docstring(node: ast.AST) -> list[str]:
    docs = []
    doc = ast.get_docstring(node)
    if doc:
        docs.append(doc)
    
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        for arg in node.args.args + getattr(node.args, 'kwonlyargs', []) + getattr(node.args, 'posonlyargs', []):
            if arg.annotation:
                docs.extend(_extract_doc_from_annotation(arg.annotation))
        if node.args.vararg and node.args.vararg.annotation:
            docs.extend(_extract_doc_from_annotation(node.args.vararg.annotation))
        if node.args.kwarg and node.args.kwarg.annotation:
            docs.extend(_extract_doc_from_annotation(node.args.kwarg.annotation))
        if node.returns:
            docs.extend(_extract_doc_from_annotation(node.returns))
            
    return docs

code = """
def Body(
    default: Annotated[
        Any,
        Doc("Default value if the parameter field is not set.")
    ] = ...,
    *,
    media_type: Annotated[
        str,
        Doc("The media type.")
    ] = "application/json"
) -> Any:
    pass
"""

tree = ast.parse(code)
print(_get_docstring(tree.body[0]))
