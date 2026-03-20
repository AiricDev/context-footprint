"""
Resolver backends for symbol navigation.

The extractor keeps AST-based role classification and uses these backends only for
"position to definition" style queries. Jedi remains the default backend; ty is
an experimental LSP-backed backend for evaluation on larger projects.
"""

from __future__ import annotations

import abc
import json
import os
import re
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional
from urllib.parse import unquote, urlparse
from urllib.request import pathname2url, url2pathname

import jedi


_IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def _path_to_uri(path: str) -> str:
    return f"file://{pathname2url(os.path.abspath(path))}"


def _uri_to_path(uri: str | None) -> Optional[str]:
    if not uri:
        return None
    parsed = urlparse(uri)
    if parsed.scheme != "file":
        return None
    netloc = f"//{parsed.netloc}" if parsed.netloc else ""
    return os.path.abspath(url2pathname(f"{netloc}{unquote(parsed.path)}"))


def _extract_marked_up_text(contents: Any) -> list[str]:
    if contents is None:
        return []
    if isinstance(contents, str):
        return [contents]
    if isinstance(contents, list):
        out: list[str] = []
        for item in contents:
            out.extend(_extract_marked_up_text(item))
        return out
    if isinstance(contents, dict):
        value = contents.get("value")
        if isinstance(value, str):
            return [value]
    return []


def _guess_symbol_name(path: str | None, line: int, column: int) -> Optional[str]:
    if not path or not os.path.exists(path):
        return None
    try:
        line_text = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()[line]
    except Exception:
        return None

    for match in _IDENT_RE.finditer(line_text):
        if match.start() <= column < match.end():
            return match.group(0)
        if match.start() >= column:
            return match.group(0)
    return None


@dataclass(slots=True)
class ResolvedTarget:
    path: Optional[str]
    line: int
    column: int
    name: Optional[str] = None
    full_name: Optional[str] = None
    kind: Optional[str] = None
    documentation: list[str] = field(default_factory=list)
    signature: Optional[str] = None


class DocumentResolver(abc.ABC):
    @abc.abstractmethod
    def goto(self, line: int, column: int, *, follow_imports: bool = False) -> list[ResolvedTarget]:
        raise NotImplementedError

    def references(self, line: int, column: int) -> list[ResolvedTarget]:
        return []

    def document_symbols(self) -> list[dict[str, Any]]:
        return []

    def workspace_symbols(self, query: str) -> list[dict[str, Any]]:
        return []

    def hover(self, line: int, column: int) -> list[str]:
        return []


class ProjectResolverBackend(abc.ABC):
    @abc.abstractmethod
    def open_document(self, file_path: str, source: str) -> DocumentResolver:
        raise NotImplementedError

    def close(self) -> None:
        return None


class JediDocumentResolver(DocumentResolver):
    def __init__(self, script: jedi.Script):
        self._script = script

    @staticmethod
    def _to_target(definition: Any) -> ResolvedTarget:
        path = str(definition.module_path) if definition.module_path else None
        line = max(0, (definition.line or 1) - 1)
        column = definition.column or 0
        docs: list[str] = []
        signature = None
        try:
            doc_str = definition.docstring()
            if doc_str:
                docs.append(doc_str)
        except Exception:
            pass
        try:
            sigs = definition.get_signatures()
            if sigs:
                signature = sigs[0].to_string()
        except Exception:
            pass
        return ResolvedTarget(
            path=path,
            line=line,
            column=column,
            name=getattr(definition, "name", None),
            full_name=getattr(definition, "full_name", None),
            kind=getattr(definition, "type", None),
            documentation=docs,
            signature=signature,
        )

    def goto(self, line: int, column: int, *, follow_imports: bool = False) -> list[ResolvedTarget]:
        try:
            defs = self._script.goto(line, column, follow_imports=follow_imports)
        except Exception:
            return []
        return [self._to_target(definition) for definition in defs]


class JediProjectResolverBackend(ProjectResolverBackend):
    def __init__(self, *, venv_path: str | None = None):
        self._environment = jedi.create_environment(venv_path, safe=False) if venv_path else None

    def open_document(self, file_path: str, source: str) -> DocumentResolver:
        return JediDocumentResolver(jedi.Script(source, path=file_path, environment=self._environment))


class _JsonRpcClient:
    def __init__(
        self,
        command: list[str],
        root_uri: str,
        *,
        cwd: str | None = None,
        env: dict[str, str] | None = None,
    ):
        self._proc = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=cwd,
            env=env,
        )
        self._next_id = 1
        self._request(
            "initialize",
            {
                "processId": os.getpid(),
                "rootUri": root_uri,
                "capabilities": {},
                "clientInfo": {"name": "cf-extractor", "version": "0.1.2"},
            },
        )
        self._notify("initialized", {})

    def _write(self, payload: dict[str, Any]) -> None:
        body = json.dumps(payload).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
        assert self._proc.stdin is not None
        self._proc.stdin.write(header)
        self._proc.stdin.write(body)
        self._proc.stdin.flush()

    def _read(self) -> dict[str, Any]:
        assert self._proc.stdout is not None
        headers: dict[str, str] = {}
        while True:
            line = self._proc.stdout.readline()
            if not line:
                raise RuntimeError("ty server exited unexpectedly")
            if line == b"\r\n":
                break
            header = line.decode("ascii").strip()
            key, _, value = header.partition(":")
            headers[key.lower()] = value.strip()
        content_length = int(headers["content-length"])
        body = self._proc.stdout.read(content_length)
        return json.loads(body.decode("utf-8"))

    def _request(self, method: str, params: dict[str, Any]) -> Any:
        request_id = self._next_id
        self._next_id += 1
        self._write({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        while True:
            message = self._read()
            if message.get("id") != request_id:
                continue
            if "error" in message:
                raise RuntimeError(f"ty server {method} failed: {message['error']}")
            return message.get("result")

    def _notify(self, method: str, params: dict[str, Any]) -> None:
        self._write({"jsonrpc": "2.0", "method": method, "params": params})

    def close(self) -> None:
        if self._proc.poll() is None:
            try:
                self._request("shutdown", {})
            except Exception:
                pass
            try:
                self._notify("exit", {})
            except Exception:
                pass
            try:
                self._proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self._proc.terminate()
                self._proc.wait(timeout=2)


class TyLspDocumentResolver(DocumentResolver):
    def __init__(self, backend: "TyLspProjectResolverBackend", file_path: str):
        self._backend = backend
        self._file_path = os.path.abspath(file_path)

    def goto(self, line: int, column: int, *, follow_imports: bool = False) -> list[ResolvedTarget]:
        result = self._backend.request_definition(self._file_path, line, column)
        return self._backend.targets_from_locations(result)

    def references(self, line: int, column: int) -> list[ResolvedTarget]:
        result = self._backend.request_references(self._file_path, line, column)
        return self._backend.targets_from_locations(result)

    def document_symbols(self) -> list[dict[str, Any]]:
        return self._backend.request_document_symbols(self._file_path)

    def workspace_symbols(self, query: str) -> list[dict[str, Any]]:
        return self._backend.request_workspace_symbols(query)

    def hover(self, line: int, column: int) -> list[str]:
        return self._backend.request_hover(self._file_path, line, column)


class TyLspProjectResolverBackend(ProjectResolverBackend):
    def __init__(self, project_root: str, *, ty_path: str | None = None, python_env: str | None = None):
        executable = ty_path or shutil.which("ty")
        if not executable:
            raise RuntimeError(
                "ty backend requested but no 'ty' executable was found. "
                "Install ty or pass --ty-path."
            )
        self._project_root = os.path.abspath(project_root)
        self._client = _JsonRpcClient(
            [executable, "server"],
            _path_to_uri(self._project_root),
            cwd=self._project_root,
            env=_build_ty_environment(python_env),
        )
        self._opened_documents: set[str] = set()

    @staticmethod
    def _to_lsp_line(line: int) -> int:
        return max(0, line - 1)

    def _ensure_open(self, file_path: str, source: str | None = None) -> None:
        abs_path = os.path.abspath(file_path)
        if abs_path in self._opened_documents:
            return
        if source is None:
            source = Path(abs_path).read_text(encoding="utf-8", errors="replace")
        self._client._notify(
            "textDocument/didOpen",
            {
                "textDocument": {
                    "uri": _path_to_uri(abs_path),
                    "languageId": "python",
                    "version": 1,
                    "text": source,
                }
            },
        )
        self._opened_documents.add(abs_path)

    def _location_to_target(
        self,
        location: dict[str, Any],
    ) -> ResolvedTarget:
        uri = location.get("uri") or location.get("targetUri")
        range_data = location.get("range") or location.get("targetSelectionRange") or location.get("targetRange") or {}
        start = range_data.get("start", {})
        line = int(start.get("line", 0))
        column = int(start.get("character", 0))
        path = _uri_to_path(uri)
        name = _guess_symbol_name(path, line, column)
        return ResolvedTarget(
            path=path,
            line=line,
            column=column,
            name=name,
            full_name=None,
            kind=None,
            documentation=[],
        )

    def targets_from_locations(
        self,
        result: Any,
    ) -> list[ResolvedTarget]:
        if result is None:
            return []
        if isinstance(result, dict):
            return [self._location_to_target(result)]
        if isinstance(result, list):
            return [self._location_to_target(item) for item in result]
        return []

    def request_definition(self, file_path: str, line: int, column: int) -> Any:
        self._ensure_open(file_path)
        return self._client._request(
            "textDocument/definition",
            {
                "textDocument": {"uri": _path_to_uri(file_path)},
                "position": {"line": self._to_lsp_line(line), "character": column},
            },
        )

    def request_references(self, file_path: str, line: int, column: int) -> Any:
        self._ensure_open(file_path)
        return self._client._request(
            "textDocument/references",
            {
                "textDocument": {"uri": _path_to_uri(file_path)},
                "position": {"line": self._to_lsp_line(line), "character": column},
                "context": {"includeDeclaration": True},
            },
        )

    def request_document_symbols(self, file_path: str) -> list[dict[str, Any]]:
        self._ensure_open(file_path)
        result = self._client._request("textDocument/documentSymbol", {"textDocument": {"uri": _path_to_uri(file_path)}})
        return result or []

    def request_workspace_symbols(self, query: str) -> list[dict[str, Any]]:
        result = self._client._request("workspace/symbol", {"query": query})
        return result or []

    def request_hover(self, file_path: str, line: int, column: int) -> list[str]:
        if not file_path:
            return []
        self._ensure_open(file_path)
        result = self._client._request(
            "textDocument/hover",
            {
                "textDocument": {"uri": _path_to_uri(file_path)},
                "position": {"line": self._to_lsp_line(line), "character": column},
            },
        )
        if not isinstance(result, dict):
            return []
        return _extract_marked_up_text(result.get("contents"))

    def open_document(self, file_path: str, source: str) -> DocumentResolver:
        self._ensure_open(file_path, source)
        return TyLspDocumentResolver(self, file_path)

    def close(self) -> None:
        self._client.close()


def build_project_resolver_backend(
    backend_name: str,
    *,
    project_root: str,
    venv_path: str | None = None,
    ty_path: str | None = None,
) -> ProjectResolverBackend:
    if backend_name == "jedi":
        return JediProjectResolverBackend(venv_path=venv_path)
    if backend_name == "ty":
        return TyLspProjectResolverBackend(project_root, ty_path=ty_path, python_env=venv_path)
    raise ValueError(f"Unsupported resolver backend: {backend_name}")


def _build_ty_environment(python_env: str | None) -> dict[str, str]:
    env = os.environ.copy()
    if not python_env:
        return env

    resolved = os.path.abspath(python_env)
    venv_root = resolved
    if os.path.isfile(resolved):
        venv_root = os.path.dirname(os.path.dirname(resolved))

    env["VIRTUAL_ENV"] = venv_root
    bin_dir = os.path.join(venv_root, "Scripts" if os.name == "nt" else "bin")
    if os.path.isdir(bin_dir):
        env["PATH"] = os.pathsep.join([bin_dir, env.get("PATH", "")]).strip(os.pathsep)
    return env
