from typing import Any
from typing_extensions import Annotated, Doc

def Body(
    default: Annotated[Any, Doc("The default value")] = ...
) -> Any:
    pass
