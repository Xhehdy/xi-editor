#!/usr/bin/env python3
"""Validate benchmark regressions against static guardrails.

Expected criterion output layout:
  <criterion_root>/<benchmark_key>/new/estimates.json
"""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import sys
from dataclasses import dataclass


@dataclass
class Observed:
    mean_ns: float
    p50_ns: float
    p95_ns: float


def load_json(path: pathlib.Path) -> dict:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def load_observed(criterion_root: pathlib.Path, benchmark_key: str) -> Observed:
    estimates_path = criterion_root / benchmark_key / "new" / "estimates.json"
    if not estimates_path.exists():
        raise FileNotFoundError(f"Missing Criterion estimates: {estimates_path}")

    estimates = load_json(estimates_path)
    mean = float(estimates["mean"]["point_estimate"])
    p50 = float(estimates["median"]["point_estimate"])
    std_dev = float(estimates["std_dev"]["point_estimate"])
    p95 = mean + (1.645 * std_dev)
    return Observed(mean_ns=mean, p50_ns=p50, p95_ns=p95)


def pct_delta(current: float, baseline: float) -> float:
    if baseline == 0:
        return math.inf
    return ((current - baseline) / baseline) * 100.0


def main() -> int:
    parser = argparse.ArgumentParser(description="Benchmark regression guardrails")
    parser.add_argument(
        "--guardrails",
        default="scripts/perf_guardrails.json",
        help="Path to guardrails JSON file",
    )
    parser.add_argument(
        "--criterion-root",
        default="crates/glyph-patch/target/criterion",
        help="Criterion output root directory",
    )
    args = parser.parse_args()

    guardrails_path = pathlib.Path(args.guardrails)
    criterion_root = pathlib.Path(args.criterion_root)

    config = load_json(guardrails_path)
    max_regression_pct = float(config.get("max_regression_pct", 10.0))
    benchmarks = config.get("benchmarks", {})

    if not benchmarks:
        print("No guardrail benchmarks configured.", file=sys.stderr)
        return 2

    print("Performance guardrails")
    print(f"max_regression_pct: {max_regression_pct:.2f}%")
    print(
        "benchmark | mean(ns) | baseline_mean(ns) | delta_mean(%) | p95(ns) | baseline_p95(ns) | delta_p95(%)"
    )

    failures: list[str] = []

    for benchmark_key, limits in benchmarks.items():
        baseline_mean = float(limits["mean_ns"])
        baseline_p95 = float(limits["p95_ns"])
        observed = load_observed(criterion_root, benchmark_key)

        mean_delta = pct_delta(observed.mean_ns, baseline_mean)
        p95_delta = pct_delta(observed.p95_ns, baseline_p95)

        print(
            f"{benchmark_key} | "
            f"{observed.mean_ns:.1f} | {baseline_mean:.1f} | {mean_delta:.2f}% | "
            f"{observed.p95_ns:.1f} | {baseline_p95:.1f} | {p95_delta:.2f}%"
        )

        if mean_delta > max_regression_pct:
            failures.append(
                f"{benchmark_key}: mean regression {mean_delta:.2f}% > {max_regression_pct:.2f}%"
            )
        if p95_delta > max_regression_pct:
            failures.append(
                f"{benchmark_key}: p95 regression {p95_delta:.2f}% > {max_regression_pct:.2f}%"
            )

    if failures:
        print("\nRegression guardrail failures:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print("\nAll performance guardrails passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
