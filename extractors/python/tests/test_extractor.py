"""Basic tests for the Python extractor prototype."""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

# Add package to path when running tests without install
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from cf_extractor.main import run_extract
from cf_extractor.schema import ReferenceRole, SemanticData, SymbolKind


FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures"


def test_run_extract_produces_valid_json():
    data = run_extract(str(FIXTURES_DIR))
    out = data.model_dump_json()
    parsed = json.loads(out)
    assert "project_root" in parsed
    assert "documents" in parsed
    assert isinstance(parsed["documents"], list)


def test_fixtures_have_definitions():
    data = run_extract(str(FIXTURES_DIR))
    assert len(data.documents) >= 1
    defs = [d for doc in data.documents for d in doc.definitions]
    assert len(defs) >= 2
    kinds = {d.kind for d in defs}
    assert SymbolKind.Function in kinds


def test_fixtures_have_references():
    data = run_extract(str(FIXTURES_DIR))
    refs = [r for doc in data.documents for r in doc.references]
    assert len(refs) >= 1
    roles = {r.role for r in refs}
    assert ReferenceRole.Call in roles


def test_schema_details_tagged_for_rust():
    data = run_extract(str(FIXTURES_DIR))
    raw = json.loads(data.model_dump_json())
    for doc in raw["documents"]:
        for d in doc["definitions"]:
            details = d["details"]
            assert isinstance(details, dict)
            assert len(details) == 1
            assert list(details.keys())[0] in ("Function", "Variable", "Type")
