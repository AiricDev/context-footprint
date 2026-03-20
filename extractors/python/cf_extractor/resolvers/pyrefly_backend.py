from __future__ import annotations

import shutil
from typing import Any

from .lsp_common import LspProjectResolverBackend, python_executable_from_env


def build_pyrefly_initialization_options(python_env: str | None) -> dict[str, Any]:
    initialization_options: dict[str, Any] = {
        "pyrefly": {
            "displayTypeErrors": "force-off",
            "disableLanguageServices": False,
            "disabledLanguageServices": {
                "definition": False,
                "declaration": True,
                "typeDefinition": True,
                "codeAction": True,
                "completion": True,
                "documentHighlight": True,
                "references": False,
                "rename": True,
                "signatureHelp": True,
                "hover": False,
                "inlayHint": True,
                "documentSymbol": False,
                "semanticTokens": True,
                "implementation": True,
            },
        }
    }
    python_path = python_executable_from_env(python_env)
    if python_path:
        initialization_options["pythonPath"] = python_path
    return initialization_options


class PyreflyLspProjectResolverBackend(LspProjectResolverBackend):
    def __init__(self, project_root: str, *, pyrefly_path: str | None = None, python_env: str | None = None):
        executable = pyrefly_path or shutil.which("pyrefly")
        if not executable:
            raise RuntimeError(
                "pyrefly backend requested but no 'pyrefly' executable was found. "
                "Install pyrefly or pass --pyrefly-path."
            )
        super().__init__(
            project_root,
            command=[executable, "lsp"],
            server_name="pyrefly",
            python_env=python_env,
            initialization_options=build_pyrefly_initialization_options(python_env),
        )
