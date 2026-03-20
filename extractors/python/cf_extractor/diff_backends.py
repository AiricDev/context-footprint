"""
Compare resolver backend outputs for the same project and emit a Markdown diff report.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from .main import run_extract
from .resolver_backend import DEFAULT_RESOLVER_BACKEND, RESOLVER_BACKENDS


@dataclass(frozen=True, slots=True)
class RefRow:
    file: str
    line: int
    column: int
    enclosing_symbol: str
    role: str
    target_symbol: str | None
    receiver: str | None
    method_name: str | None

    @property
    def location_key(self) -> tuple[str, int, int, str, str]:
        return (self.file, self.line, self.column, self.enclosing_symbol, self.role)


def _collect_rows(
    project_root: str,
    backend: str,
    *,
    include_tests: bool,
    ty_path: str | None,
    pyrefly_path: str | None,
) -> list[RefRow]:
    data = run_extract(
        project_root,
        include_tests=include_tests,
        resolver_backend=backend,
        ty_path=ty_path,
        pyrefly_path=pyrefly_path,
    )
    rows: list[RefRow] = []
    for document in data.documents:
        for reference in document.references:
            rows.append(
                RefRow(
                    file=document.relative_path,
                    line=reference.location.line,
                    column=reference.location.column,
                    enclosing_symbol=reference.enclosing_symbol,
                    role=reference.role.value,
                    target_symbol=reference.target_symbol,
                    receiver=reference.receiver,
                    method_name=reference.method_name,
                )
            )
    return rows


def _format_ref(row: RefRow) -> str:
    return (
        f"{row.file}:{row.line}:{row.column} "
        f"{row.enclosing_symbol} {row.role} -> {row.target_symbol} "
        f"(receiver={row.receiver}, method={row.method_name})"
    )


def _target_tuple(row: RefRow) -> tuple[str, str, str]:
    return (
        row.target_symbol or "None",
        row.receiver or "None",
        row.method_name or "None",
    )


def _top_counts(rows: Iterable[RefRow], *, attr: str, limit: int = 10) -> list[tuple[str, int]]:
    counter = Counter(str(getattr(row, attr)) for row in rows)
    return counter.most_common(limit)


def _report(
    left_name: str,
    right_name: str,
    left_rows: list[RefRow],
    right_rows: list[RefRow],
    *,
    example_limit: int,
) -> str:
    left_set = set(left_rows)
    right_set = set(right_rows)
    only_left = [row for row in left_rows if row not in right_set]
    only_right = [row for row in right_rows if row not in left_set]

    left_by_loc: dict[tuple[str, int, int, str, str], list[RefRow]] = defaultdict(list)
    right_by_loc: dict[tuple[str, int, int, str, str], list[RefRow]] = defaultdict(list)
    for row in left_rows:
        left_by_loc[row.location_key].append(row)
    for row in right_rows:
        right_by_loc[row.location_key].append(row)

    different_targets: list[tuple[tuple[str, int, int, str, str], list[RefRow], list[RefRow]]] = []
    for key in sorted(set(left_by_loc) & set(right_by_loc)):
        left_targets = sorted(_target_tuple(row) for row in left_by_loc[key])
        right_targets = sorted(_target_tuple(row) for row in right_by_loc[key])
        if left_targets != right_targets:
            different_targets.append((key, left_by_loc[key], right_by_loc[key]))

    lines = [
        "# Resolver Backend Diff",
        "",
        f"- Left backend: `{left_name}`",
        f"- Right backend: `{right_name}`",
        f"- Total refs: `{left_name}`={len(left_rows)}, `{right_name}`={len(right_rows)}",
        f"- Only in `{left_name}`: {len(only_left)}",
        f"- Only in `{right_name}`: {len(only_right)}",
        f"- Same location, different targets: {len(different_targets)}",
        "",
        "## Summary",
        "",
        f"- Only in `{left_name}` by role: {_top_counts(only_left, attr='role')}",
        f"- Only in `{right_name}` by role: {_top_counts(only_right, attr='role')}",
        f"- Only in `{left_name}` by file: {_top_counts(only_left, attr='file')}",
        f"- Only in `{right_name}` by file: {_top_counts(only_right, attr='file')}",
        "",
        f"## Only In {left_name}",
        "",
    ]
    for row in only_left[:example_limit]:
        lines.append(f"- {_format_ref(row)}")
    if not only_left:
        lines.append("- none")

    lines.extend(["", f"## Only In {right_name}", ""])
    for row in only_right[:example_limit]:
        lines.append(f"- {_format_ref(row)}")
    if not only_right:
        lines.append("- none")

    lines.extend(["", "## Different Targets At Same Location", ""])
    for key, left_group, right_group in different_targets[:example_limit]:
        lines.append(f"- Location: `{key}`")
        lines.append(f"  {left_name}: {[_target_tuple(row) for row in left_group]}")
        lines.append(f"  {right_name}: {[_target_tuple(row) for row in right_group]}")
    if not different_targets:
        lines.append("- none")

    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Diff cf-extractor resolver backends.")
    parser.add_argument("project_root", help="Project root to analyze")
    parser.add_argument("--left", default="jedi", choices=RESOLVER_BACKENDS, help="Left backend")
    parser.add_argument("--right", default=DEFAULT_RESOLVER_BACKEND, choices=RESOLVER_BACKENDS, help="Right backend")
    parser.add_argument("--include-tests", action="store_true", help="Include test files")
    parser.add_argument("--ty-path", help="Path to ty executable")
    parser.add_argument("--pyrefly-path", help="Path to pyrefly executable")
    parser.add_argument("--example-limit", type=int, default=20, help="Max examples per section")
    parser.add_argument("--report-out", help="Optional path to write the Markdown report")
    args = parser.parse_args()

    left_rows = _collect_rows(
        args.project_root,
        args.left,
        include_tests=args.include_tests,
        ty_path=args.ty_path,
        pyrefly_path=args.pyrefly_path,
    )
    right_rows = _collect_rows(
        args.project_root,
        args.right,
        include_tests=args.include_tests,
        ty_path=args.ty_path,
        pyrefly_path=args.pyrefly_path,
    )
    report = _report(args.left, args.right, left_rows, right_rows, example_limit=args.example_limit)
    if args.report_out:
        Path(args.report_out).write_text(report, encoding="utf-8")
    print(report, end="")


if __name__ == "__main__":
    main()
