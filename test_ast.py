import ast

code = """
def Body(
    default: Annotated[
        Any,
        Doc(
            "Default value if the parameter field is not set."
        ),
    ] = ...
) -> Any:
    pass
"""

print(ast.dump(ast.parse(code), indent=2))
