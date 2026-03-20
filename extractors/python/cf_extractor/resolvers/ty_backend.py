from __future__ import annotations

import shutil

from .lsp_common import LspProjectResolverBackend


class TyLspProjectResolverBackend(LspProjectResolverBackend):
    def __init__(self, project_root: str, *, ty_path: str | None = None, python_env: str | None = None):
        executable = ty_path or shutil.which("ty")
        if not executable:
            raise RuntimeError(
                "ty backend requested but no 'ty' executable was found. "
                "Install ty or pass --ty-path."
            )
        super().__init__(
            project_root,
            command=[executable, "server"],
            server_name="ty",
            python_env=python_env,
        )
