from __future__ import annotations

from .base import (
    DEFAULT_RESOLVER_BACKEND,
    RESOLVER_BACKENDS,
    DocumentResolver,
    ProjectResolverBackend,
    ResolvedTarget,
)
from .jedi_backend import JediProjectResolverBackend
from .pyrefly_backend import PyreflyLspProjectResolverBackend
from .ty_backend import TyLspProjectResolverBackend


def build_project_resolver_backend(
    backend_name: str,
    *,
    project_root: str,
    venv_path: str | None = None,
    ty_path: str | None = None,
    pyrefly_path: str | None = None,
) -> ProjectResolverBackend:
    if backend_name == "jedi":
        return JediProjectResolverBackend(venv_path=venv_path)
    if backend_name == "ty":
        return TyLspProjectResolverBackend(project_root, ty_path=ty_path, python_env=venv_path)
    if backend_name == "pyrefly":
        return PyreflyLspProjectResolverBackend(project_root, pyrefly_path=pyrefly_path, python_env=venv_path)
    raise ValueError(f"Unsupported resolver backend: {backend_name}")
