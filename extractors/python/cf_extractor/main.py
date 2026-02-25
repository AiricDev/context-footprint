"""
CLI entrypoint: scan project directory, extract SemanticData, print JSON to stdout.
"""

import argparse
import json
import os
import sys
from pathlib import Path

from .extractor import extract_definitions_from_file
from .jedi_resolver import collect_references
from .schema import DocumentSemantics, SemanticData, SymbolDefinition


def find_python_files(project_root: str) -> list[str]:
    """Return relative paths of all .py files under project_root."""
    root = Path(project_root)
    out = []
    for p in root.rglob("*.py"):
        try:
            rel = p.relative_to(root)
            out.append(rel.as_posix())
        except ValueError:
            continue
    return sorted(out)


def run_extract(project_root: str, venv_path: str | None = None) -> SemanticData:
    """Extract SemanticData from a project directory."""
    project_root = os.path.abspath(project_root)
    docs: list[DocumentSemantics] = []
    all_definitions: list[SymbolDefinition] = []
    external_symbols: dict[str, SymbolDefinition] = {}

    py_files = find_python_files(project_root)
    for rel_path in py_files:
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

    for i, doc in enumerate(docs):
        rel_path = doc.relative_path
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
                venv_path=venv_path,
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
        required=True,
        help="Project root directory",
    )
    parser.add_argument(
        "--venv",
        help="Path to virtual environment (auto-detected as .venv in project root if not specified)",
    )

    args = parser.parse_args()
    project_root = os.path.abspath(args.project_root)
    
    venv_path = args.venv
    if not venv_path:
        default_venv = os.path.join(project_root, ".venv")
        if os.path.isdir(default_venv):
            venv_path = default_venv
            print(f"Auto-detected venv at: {venv_path}", file=sys.stderr)

    data = run_extract(project_root, venv_path=venv_path)
    print(data.model_dump_json(indent=2))


if __name__ == "__main__":
    main()
