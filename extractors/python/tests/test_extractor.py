"""Basic tests for the Python extractor prototype."""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

# Add package to path when running tests without install
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from cf_extractor.main import find_python_files, run_extract
from cf_extractor.schema import ReferenceRole, SemanticData, SymbolKind


FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures"


def test_find_python_files_include_pattern():
    """Include filter restricts to matching paths only."""
    files = find_python_files(str(FIXTURES_DIR), include_tests=True, include=["simple.py"])
    assert files == ["simple.py"]


def test_find_python_files_exclude_pattern():
    """Exclude filter skips matching paths."""
    all_files = find_python_files(str(FIXTURES_DIR), include_tests=True)
    files = find_python_files(str(FIXTURES_DIR), include_tests=True, exclude=["test_*"])
    assert len(files) < len(all_files)
    assert not any("test_" in f for f in files)
    assert "simple.py" in files


def test_find_python_files_include_and_exclude():
    """Include and exclude can be combined."""
    files = find_python_files(
        str(FIXTURES_DIR),
        include_tests=True,
        include=["*_call.py"],
        exclude=["test_nested*"],
    )
    assert "test_self_call.py" in files
    assert "test_nested_call.py" not in files


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

def test_method_call_on_type_hinted_parameter():
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [r for doc in data.documents for r in doc.references if r.enclosing_symbol == "test_method_resolve.create_image_edit"]
    
    # We expect a Call reference to RelayImageUseCase.execute
    execute_calls = [r for r in refs if r.role == ReferenceRole.Call and r.target_symbol == "test_method_resolve.RelayImageUseCase.execute"]
    if not execute_calls:
        print("REFS for test_method_resolve.create_image_edit:", refs)
    assert len(execute_calls) == 1, "Should resolve use_case.execute() to RelayImageUseCase.execute"

def test_except_handler_type_reference():
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [r for doc in data.documents for r in doc.references if r.enclosing_symbol == "test_except_resolve.create_image_edit"]
    
    # We expect a Read reference to QuotaError
    except_refs = [r for r in refs if r.role == ReferenceRole.Read and r.target_symbol == "test_except_resolve.QuotaError"]
    if not except_refs:
        print("REFS for test_except_resolve.create_image_edit:", refs)
    assert len(except_refs) >= 1, "Should resolve 'except QuotaError' as a Read reference to QuotaError"


def test_self_call_resolution():
    """put() calls self.api_route() -> should resolve to APIRouter.api_route for CF traversal."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "test_self_call.APIRouter.put"
    ]
    api_route_calls = [
        r
        for r in refs
        if r.role == ReferenceRole.Call and r.target_symbol == "test_self_call.APIRouter.api_route"
    ]
    if not api_route_calls:
        print("REFS for test_self_call.APIRouter.put:", refs)
    assert len(api_route_calls) == 1, "Should resolve self.api_route() to APIRouter.api_route"


def test_nested_function_call_attributed_to_outer():
    """Call inside nested function (decorator) should have enclosing_symbol = api_route, not api_route.decorator."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.role == ReferenceRole.Call and r.target_symbol == "test_nested_call.APIRouter.add_api_route"
    ]
    from_api_route = [r for r in refs if r.enclosing_symbol == "test_nested_call.APIRouter.api_route"]
    if not from_api_route:
        print("Call refs to add_api_route:", [(r.enclosing_symbol, r.target_symbol) for r in refs])
    assert len(from_api_route) >= 1, "Call from nested decorator should be attributed to api_route"


def test_annotated_doc_extraction():
    """Ensure PEP 727 Doc() strings inside Annotated are extracted as documentation."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    body_def = next(
        (d for doc in data.documents for d in doc.definitions if d.name == "Body" and "test_annotated_doc" in doc.relative_path),
        None,
    )
    assert body_def is not None, "Body function definition not found"
    assert len(body_def.documentation) == 2, "Should extract exactly 2 Doc() strings"
    assert "Default value if the parameter field is not set." in body_def.documentation[0]
    assert "The media type." in body_def.documentation[1]


def test_annotated_style_factory_use_signature_only_for_size():
    """Annotated-style factory (Doc() in params + trivial body) gets use_signature_only_for_size=True."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    body_def = next(
        (d for doc in data.documents for d in doc.definitions if d.name == "Body" and "test_annotated_doc" in doc.relative_path),
        None,
    )
    assert body_def is not None, "Body function definition not found"
    assert body_def.details.modifiers.use_signature_only_for_size is True, (
        "Body() has Doc() in Annotated params and trivial body (pass); "
        "should set use_signature_only_for_size so CF uses signature-only size"
    )


def test_default_arg_reference_emitted_as_read():
    """References in default argument values (e.g. SENTINEL) must be emitted as Read so CF includes them."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "test_default_arg_ref.foo"
    ]
    sentinel_reads = [
        r
        for r in refs
        if r.role == ReferenceRole.Read and r.target_symbol == "test_default_arg_ref.SENTINEL"
    ]
    if not sentinel_reads:
        print("REFS for test_default_arg_ref.foo:", refs)
    assert len(sentinel_reads) >= 1, (
        "Default arg 'flag: bool = SENTINEL' should produce a Read reference from foo to SENTINEL"
    )


def test_func_as_value_emitted_as_read():
    """Function used as value (e.g. register(handler), callback = handler) must produce Read ref."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "test_func_as_value.setup"
    ]
    handler_reads = [
        r
        for r in refs
        if r.role == ReferenceRole.Read and r.target_symbol == "test_func_as_value.handler"
    ]
    if not handler_reads:
        print("REFS for test_func_as_value.setup:", refs)
    assert len(handler_reads) >= 1, (
        "setup() uses handler as value (register(handler), callback=handler); "
        "should produce at least one Read reference from setup to handler"
    )


def test_super_call_resolves_to_parent_method():
    """super().__init__(...) must produce Call ref from Child.__init__ to Base.__init__."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "test_super_call.Child.__init__"
    ]
    super_calls = [
        r
        for r in refs
        if r.role == ReferenceRole.Call and r.target_symbol == "test_super_call.Base.__init__"
    ]
    if not super_calls:
        print("REFS for test_super_call.Child.__init__:", refs)
    assert len(super_calls) >= 1, (
        "super().__init__(name) should produce Call reference from Child.__init__ to Base.__init__"
    )


def test_augassign_emits_read_and_write():
    """counter += 1 must produce both Read and Write refs from increment to counter."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "test_augassign.increment"
    ]
    counter_reads = [
        r for r in refs if r.role == ReferenceRole.Read and r.target_symbol == "test_augassign.counter"
    ]
    counter_writes = [
        r for r in refs if r.role == ReferenceRole.Write and r.target_symbol == "test_augassign.counter"
    ]
    if not counter_reads or not counter_writes:
        print("REFS for test_augassign.increment:", refs)
    assert len(counter_reads) >= 1, "counter += 1 should produce Read from increment to counter"
    assert len(counter_writes) >= 1, "counter += 1 should produce Write from increment to counter"


def test_cls_call_resolves_to_init():
    """cls(name) in classmethod create() must produce Call ref from MyClass.create to MyClass.__init__."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "test_cls_call.MyClass.create"
    ]
    init_calls = [
        r
        for r in refs
        if r.role == ReferenceRole.Call and r.target_symbol == "test_cls_call.MyClass.__init__"
    ]
    if not init_calls:
        print("REFS for test_cls_call.MyClass.create:", refs)
    assert len(init_calls) >= 1, (
        "return cls(name) in classmethod create() should produce Call from MyClass.create to MyClass.__init__"
    )


def test_super_call_cross_module_resolves_to_parent_method():
    """super().__init__() in child_super.Child (base in base_super) must produce Call to base_super.Base.__init__."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "child_super.Child.__init__"
    ]
    super_calls = [
        r
        for r in refs
        if r.role == ReferenceRole.Call and r.target_symbol == "base_super.Base.__init__"
    ]
    if not super_calls:
        print("REFS for child_super.Child.__init__:", refs)
    assert len(super_calls) >= 1, (
        "super().__init__() in child_super.Child (Base from base_super) should produce Call to base_super.Base.__init__"
    )


def test_default_arg_cross_module_emitted_as_read():
    """Default arg SENTINEL imported from sentinel_def must produce Read from foo to sentinel_def.SENTINEL."""
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "default_arg_cross.foo"
    ]
    sentinel_reads = [
        r for r in refs if r.role == ReferenceRole.Read and r.target_symbol == "sentinel_def.SENTINEL"
    ]
    if not sentinel_reads:
        print("REFS for default_arg_cross.foo:", refs)
    assert len(sentinel_reads) >= 1, (
        "def foo(..., flag=SENTINEL) with SENTINEL from sentinel_def should produce Read from foo to sentinel_def.SENTINEL"
    )


def test_super_call_alias_base_resolves_to_parent_method():
    """super().__init__() in child_alias.Child (base imported as AliasBase) must produce Call to base_alias.Base.__init__.

    This simulates Flask's 'from .sansio.blueprints import Blueprint as SansioBlueprint'
    pattern where the base class is referenced by an alias name that differs from the
    actual class name.
    """
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "child_alias.Child.__init__"
    ]
    super_calls = [
        r
        for r in refs
        if r.role == ReferenceRole.Call and r.target_symbol == "base_alias.Base.__init__"
    ]
    if not super_calls:
        print("REFS for child_alias.Child.__init__:", refs)
    assert len(super_calls) >= 1, (
        "super().__init__() in child_alias.Child (AliasBase = base_alias.Base) should produce Call to base_alias.Base.__init__"
    )


def test_default_arg_ambiguous_sentinel_uses_import_context():
    """Default arg _sentinel with two same-named defs must pick the imported one (sentinel_a._sentinel).

    This simulates Flask's situation where both sansio/scaffold.py and ctx.py define
    _sentinel, and blueprints.py imports specifically from sansio.scaffold.
    """
    data = run_extract(str(FIXTURES_DIR), include_tests=True)
    refs = [
        r
        for doc in data.documents
        for r in doc.references
        if r.enclosing_symbol == "default_arg_ambig.bar"
    ]
    sentinel_reads = [
        r for r in refs if r.role == ReferenceRole.Read and r.target_symbol == "sentinel_a._sentinel"
    ]
    if not sentinel_reads:
        print("REFS for default_arg_ambig.bar:", refs)
    assert len(sentinel_reads) >= 1, (
        "def bar(..., flag=_sentinel) with _sentinel from sentinel_a (and sentinel_b also existing) "
        "should use import context to produce Read from bar to sentinel_a._sentinel"
    )
