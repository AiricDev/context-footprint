"""
Compatibility facade for resolver backends.

Concrete resolver implementations live under ``cf_extractor.resolvers`` so each
backend can evolve independently while existing imports keep working.
"""

from __future__ import annotations

from .resolvers import (
    DEFAULT_RESOLVER_BACKEND,
    RESOLVER_BACKENDS,
    DocumentResolver,
    ProjectResolverBackend,
    ResolvedTarget,
    build_project_resolver_backend,
)

__all__ = [
    "DEFAULT_RESOLVER_BACKEND",
    "RESOLVER_BACKENDS",
    "DocumentResolver",
    "ProjectResolverBackend",
    "ResolvedTarget",
    "build_project_resolver_backend",
]
