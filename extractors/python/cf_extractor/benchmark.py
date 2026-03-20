"""
Benchmark helper for comparing resolver backends.
"""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .resolver_backend import DEFAULT_RESOLVER_BACKEND, RESOLVER_BACKENDS


@dataclass(slots=True)
class BenchmarkSample:
    dataset: str
    backend: str
    metrics: dict[str, Any] | None = None
    error: str | None = None


def _parse_dataset(value: str) -> tuple[str, str]:
    name, sep, path = value.partition("=")
    if not sep or not name or not path:
        raise argparse.ArgumentTypeError("datasets must use NAME=PATH")
    return name, path


def _summarize_process_error(output: str) -> str:
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    filtered = [line for line in lines if not line.startswith("[1/2]") and not line.startswith("[2/2]")]
    if filtered:
        return filtered[-1]
    if lines:
        return lines[-1]
    return "unknown error"


def _run_once(
    package_root: Path,
    dataset_name: str,
    dataset_path: str,
    backend: str,
    *,
    include_tests: bool,
    ty_path: str | None,
    pyrefly_path: str | None,
) -> BenchmarkSample:
    dataset_path = str(Path(dataset_path).resolve())
    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        metrics_path = Path(tmp.name)

    cmd = [
        sys.executable,
        "-m",
        "cf_extractor.main",
        dataset_path,
        "--resolver-backend",
        backend,
        "--metrics-out",
        str(metrics_path),
    ]
    if include_tests:
        cmd.append("--include-tests")
    if ty_path:
        cmd.extend(["--ty-path", ty_path])
    if pyrefly_path:
        cmd.extend(["--pyrefly-path", pyrefly_path])

    try:
        proc = subprocess.run(
            cmd,
            cwd=package_root,
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            return BenchmarkSample(
                dataset=dataset_name,
                backend=backend,
                error=_summarize_process_error(proc.stderr or proc.stdout or f"exit {proc.returncode}"),
            )
        return BenchmarkSample(
            dataset=dataset_name,
            backend=backend,
            metrics=json.loads(metrics_path.read_text(encoding="utf-8")),
        )
    except Exception as exc:
        return BenchmarkSample(dataset=dataset_name, backend=backend, error=str(exc))
    finally:
        metrics_path.unlink(missing_ok=True)


def _aggregate(samples: list[BenchmarkSample]) -> dict[tuple[str, str], dict[str, Any]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = {}
    errors: dict[tuple[str, str], str] = {}
    for sample in samples:
        key = (sample.dataset, sample.backend)
        if sample.error:
            errors[key] = sample.error
            continue
        grouped.setdefault(key, []).append(sample.metrics or {})

    aggregated: dict[tuple[str, str], dict[str, Any]] = {}
    for key, items in grouped.items():
        aggregated[key] = {
            "runs": len(items),
            "definition_phase_seconds_avg": statistics.mean(item["definition_phase_seconds"] for item in items),
            "reference_phase_seconds_avg": statistics.mean(item["reference_phase_seconds"] for item in items),
            "total_seconds_avg": statistics.mean(item["total_seconds"] for item in items),
            "peak_rss_kb_max": max(item["peak_rss_kb"] for item in items if item.get("peak_rss_kb") is not None),
            "resolved_reference_count": items[-1]["resolved_reference_count"],
            "unresolved_reference_count": items[-1]["unresolved_reference_count"],
            "reference_count": items[-1]["reference_count"],
            "definition_count": items[-1]["definition_count"],
            "external_symbol_count": items[-1]["external_symbol_count"],
        }
    for key, error in errors.items():
        aggregated[key] = {"error": error}
    return aggregated


def _format_report(
    datasets: list[str],
    backends: list[str],
    aggregated: dict[tuple[str, str], dict[str, Any]],
) -> str:
    baseline_backend = DEFAULT_RESOLVER_BACKEND
    lines = [
        "# Resolver Backend Benchmark",
        "",
        "| Dataset | Backend | Defs (s) | Refs (s) | Total (s) | Peak RSS (KB) | Resolved | Unresolved | Verdict |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
    ]

    for dataset in datasets:
        baseline_metrics = aggregated.get((dataset, baseline_backend))
        for backend in backends:
            metrics = aggregated.get((dataset, backend), {})
            if metrics.get("error"):
                lines.append(
                    f"| {dataset} | {backend} | - | - | - | - | - | - | error: {metrics['error'].replace('|', '/')} |"
                )
                continue
            verdict = ""
            if (
                backend != baseline_backend
                and baseline_metrics
                and not baseline_metrics.get("error")
                and metrics["reference_phase_seconds_avg"] > 0
            ):
                ref_speedup = baseline_metrics["reference_phase_seconds_avg"] / metrics["reference_phase_seconds_avg"]
                coverage_ok = (
                    metrics["resolved_reference_count"] >= baseline_metrics["resolved_reference_count"]
                    and metrics["unresolved_reference_count"] <= baseline_metrics["unresolved_reference_count"]
                )
                verdict = "pass" if ref_speedup >= 2.0 and coverage_ok else "needs review"
            lines.append(
                "| "
                + " | ".join(
                    [
                        dataset,
                        backend,
                        f"{metrics['definition_phase_seconds_avg']:.3f}",
                        f"{metrics['reference_phase_seconds_avg']:.3f}",
                        f"{metrics['total_seconds_avg']:.3f}",
                        str(metrics["peak_rss_kb_max"]),
                        str(metrics["resolved_reference_count"]),
                        str(metrics["unresolved_reference_count"]),
                        verdict,
                    ]
                )
                + " |"
            )

        if baseline_metrics and not baseline_metrics.get("error"):
            lines.extend(
                [
                    "",
                    f"## {dataset}",
                    "",
                    (
                        f"- {baseline_backend} resolved/unresolved: "
                        f"{baseline_metrics['resolved_reference_count']} / {baseline_metrics['unresolved_reference_count']}"
                    ),
                ]
            )
            for backend in backends:
                metrics = aggregated.get((dataset, backend), {})
                if backend == baseline_backend or metrics.get("error") or metrics["reference_phase_seconds_avg"] <= 0:
                    continue
                lines.append(
                    (
                        f"- {backend} vs {baseline_backend}: "
                        f"reference speed ratio {baseline_metrics['reference_phase_seconds_avg'] / metrics['reference_phase_seconds_avg']:.2f}x, "
                        f"resolved/unresolved {metrics['resolved_reference_count']} / {metrics['unresolved_reference_count']}"
                    )
                )
            lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Benchmark cf-extractor resolver backends.")
    parser.add_argument(
        "--dataset",
        action="append",
        dest="datasets",
        required=True,
        type=_parse_dataset,
        help="Dataset in NAME=PATH form. Repeat for multiple datasets.",
    )
    parser.add_argument(
        "--backend",
        action="append",
        dest="backends",
        choices=RESOLVER_BACKENDS,
        default=None,
        help="Backend(s) to benchmark.",
    )
    parser.add_argument("--iterations", type=int, default=1, help="Number of runs per dataset/backend.")
    parser.add_argument("--include-tests", action="store_true", help="Pass --include-tests to cf-extract.")
    parser.add_argument("--ty-path", help="Path to the ty executable for ty backend runs.")
    parser.add_argument("--pyrefly-path", help="Path to the pyrefly executable for pyrefly backend runs.")
    parser.add_argument("--report-out", help="Optional path to write a Markdown report.")
    args = parser.parse_args()
    backends = args.backends or [DEFAULT_RESOLVER_BACKEND, "jedi"]

    package_root = Path(__file__).resolve().parent.parent
    samples: list[BenchmarkSample] = []
    for dataset_name, dataset_path in args.datasets:
        for backend in backends:
            for _ in range(args.iterations):
                samples.append(
                    _run_once(
                        package_root,
                        dataset_name,
                        dataset_path,
                        backend,
                        include_tests=args.include_tests,
                        ty_path=args.ty_path,
                        pyrefly_path=args.pyrefly_path,
                    )
                )

    aggregated = _aggregate(samples)
    report = _format_report([name for name, _ in args.datasets], list(dict.fromkeys(backends)), aggregated)
    if args.report_out:
        Path(args.report_out).write_text(report, encoding="utf-8")
    print(report, end="")


if __name__ == "__main__":
    main()
