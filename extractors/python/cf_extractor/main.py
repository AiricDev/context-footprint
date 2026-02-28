"""
CLI entrypoint: scan project directory, extract SemanticData, print JSON to stdout.
"""

import argparse
import fnmatch
import json
import os
import sys
from pathlib import Path

import jedi

from .extractor import extract_definitions_from_file
from .jedi_resolver import collect_references
from .schema import DocumentSemantics, SemanticData, SymbolDefinition


_TEST_DIR_NAMES = ("test", "tests", "testing", "__tests__", "spec")
# Virtualenv / env dirs: skip to avoid analyzing installed packages
_SKIP_DIR_NAMES = frozenset({".venv", "venv", ".virtualenv", "virtualenv", "env"})


def _is_skipped_path(rel_path: str) -> bool:
    """True if path is under a skipped dir (e.g. .venv, venv)."""
    parts = rel_path.replace("\\", "/").split("/")
    return any(part in _SKIP_DIR_NAMES for part in parts)


def _is_test_path(rel_path: str) -> bool:
    """True if path is under a test dir or filename matches test_*.py / *_test.py."""
    parts = rel_path.replace("\\", "/").split("/")
    for part in parts[:-1]:
        if part.lower() in _TEST_DIR_NAMES:
            return True
    name = parts[-1] if parts else ""
    return name.startswith("test_") and name.endswith(".py") or name.endswith("_test.py")


def find_python_files(
    project_root: str,
    *,
    include_tests: bool = False,
    include: list[str] | None = None,
    exclude: list[str] | None = None,
) -> list[str]:
    """Return relative paths of all .py files under project_root. By default excludes test paths."""
    root = Path(project_root)
    out = []
    for p in root.rglob("*.py"):
        try:
            rel = p.relative_to(root)
            path_str = rel.as_posix()
            if _is_skipped_path(path_str):
                continue
            if include_tests or not _is_test_path(path_str):
                if include is not None and len(include) > 0:
                    if not any(fnmatch.fnmatch(path_str, pat) for pat in include):
                        continue
                if exclude is not None and len(exclude) > 0:
                    if any(fnmatch.fnmatch(path_str, pat) for pat in exclude):
                        continue
                out.append(path_str)
        except ValueError:
            continue
    return sorted(out)


def run_extract(
    project_root: str,
    venv_path: str | None = None,
    *,
    include_tests: bool = False,
    include: list[str] | None = None,
    exclude: list[str] | None = None,
) -> SemanticData:
    """Extract SemanticData from a project directory."""
    project_root = os.path.abspath(project_root)
    docs: list[DocumentSemantics] = []
    all_definitions: list[SymbolDefinition] = []
    external_symbols: dict[str, SymbolDefinition] = {}

    py_files = find_python_files(
        project_root,
        include_tests=include_tests,
        include=include,
        exclude=exclude,
    )
    total_def = len(py_files)
    for idx, rel_path in enumerate(py_files, 1):
        print(f"[1/2] Definitions ({idx}/{total_def}): {rel_path}", file=sys.stderr)
        abs_path = os.path.join(project_root, rel_path)
        try:
            with open(abs_path, "r", encoding="utf-8", errors="replace") as f:
                source = f.read()
        except Exception as e:
            print(f"Warning: skip {rel_path}: {e}", file=sys.stderr)
            continue
        try:
            doc = extract_definitions_from_file(abs_path, source, project_root)
            docs.append(doc)
            all_definitions.extend(doc.definitions)
        except Exception as e:
            print(f"Warning: extract defs {rel_path}: {e}", file=sys.stderr)
            docs.append(
                DocumentSemantics(relative_path=rel_path, language="python", definitions=[], references=[])
            )

    # One Jedi environment for all files to avoid "Too many open files"
    jedi_env = jedi.create_environment(venv_path, safe=False) if venv_path else None
    total_ref = len(docs)
    for i, doc in enumerate(docs):
        rel_path = doc.relative_path
        print(f"[2/2] References ({i + 1}/{total_ref}): {rel_path}", file=sys.stderr)
        abs_path = os.path.join(project_root, rel_path)
        try:
            with open(abs_path, "r", encoding="utf-8", errors="replace") as f:
                source = f.read()
        except Exception:
            continue
        try:
            refs, ext_syms = collect_references(
                doc,
                abs_path,
                source,
                project_root,
                all_definitions,
                module_symbol_id=Path(rel_path).with_suffix("").as_posix().replace("/", ".") or "__main__",
                environment=jedi_env,
            )
            for ext in ext_syms:
                external_symbols[ext.symbol_id] = ext
            docs[i] = DocumentSemantics(
                relative_path=doc.relative_path,
                language=doc.language,
                definitions=doc.definitions,
                references=refs,
            )
        except Exception as e:
            print(f"Warning: references {rel_path}: {e}", file=sys.stderr)

    return SemanticData(
        project_root=project_root,
        documents=docs,
        external_symbols=list(external_symbols.values()),
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Extract Context-Footprint SemanticData from a Python project.")
    parser.add_argument(
        "project_root",
        nargs="?",
        help="Project root directory",
    )
    parser.add_argument(
        "--venv",
        help="Path to virtual environment (auto-detected as .venv in project root if not specified)",
    )
    parser.add_argument(
        "--include-tests",
        action="store_true",
        help="Include test files (default: exclude paths under test/tests/testing/__tests__/spec and test_*.py, *_test.py)",
    )
    parser.add_argument(
        "--include",
        nargs="*",
        metavar="PATTERN",
        help="Glob patterns for paths to include (relative to project root). If empty, all paths pass.",
    )
    parser.add_argument(
        "--exclude",
        nargs="*",
        metavar="PATTERN",
        help="Glob patterns for paths to exclude (relative to project root). Excluded paths are skipped.",
    )

    args = parser.parse_args()
    project_root = os.path.abspath(args.project_root)
    
    venv_path = args.venv
    if not venv_path:
        default_venv = os.path.join(project_root, ".venv")
        if os.path.isdir(default_venv):
            venv_path = default_venv
            print(f"Auto-detected venv at: {venv_path}", file=sys.stderr)

    data = run_extract(
        project_root,
        venv_path=venv_path,
        include_tests=args.include_tests,
        include=args.include,
        exclude=args.exclude,
    )
    print(data.model_dump_json(indent=2))


if __name__ == "__main__":
    main()
