import typing
from typing import Annotated

def Body(
    default: Annotated[
        typing.Any,
        Doc(
            """
            Default value if the parameter field is not set.
            """
        ),
    ] = ...,
    *,
    media_type: Annotated[
        str,
        Doc("The media type.")
    ] = "application/json"
) -> typing.Any:
    pass
