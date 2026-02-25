#!/usr/bin/env python3
"""Validate IPC benchmark results against static latency guardrails."""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import sys


def load_json(path: pathlib.Path) -> dict:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def pct_delta(current: float, baseline: float) -> float:
    if baseline == 0:
        return math.inf
    return ((current - baseline) / baseline) * 100.0


def main() -> int:
    parser = argparse.ArgumentParser(description="IPC guardrail checker")
    parser.add_argument(
        "--observed",
        required=True,
        help="Observed benchmark JSON from bench_ipc_apply_edit.py",
    )
    parser.add_argument(
        "--guardrails",
        default="scripts/ipc_perf_guardrails.json",
        help="Guardrail config JSON",
    )
    args = parser.parse_args()

    observed = load_json(pathlib.Path(args.observed))
    guardrails = load_json(pathlib.Path(args.guardrails))
    max_regression_pct = float(guardrails.get("max_regression_pct", 20.0))
    benchmarks = guardrails.get("benchmarks", {})

    if not benchmarks:
        print("No IPC guardrails configured.", file=sys.stderr)
        return 2

    failures: list[str] = []
    print("IPC performance guardrails")
    print(f"max_regression_pct: {max_regression_pct:.2f}%")
    print("benchmark | mean_us | baseline_mean_us | delta_mean(%) | p95_us | baseline_p95_us | delta_p95(%)")

    for key, baseline in benchmarks.items():
        if key not in observed:
            failures.append(f"missing observed benchmark: {key}")
            continue

        current_mean = float(observed[key]["mean_us"])
        current_p95 = float(observed[key]["p95_us"])
        baseline_mean = float(baseline["mean_us"])
        baseline_p95 = float(baseline["p95_us"])

        mean_delta = pct_delta(current_mean, baseline_mean)
        p95_delta = pct_delta(current_p95, baseline_p95)

        print(
            f"{key} | "
            f"{current_mean:.3f} | {baseline_mean:.3f} | {mean_delta:.2f}% | "
            f"{current_p95:.3f} | {baseline_p95:.3f} | {p95_delta:.2f}%"
        )

        if mean_delta > max_regression_pct:
            failures.append(
                f"{key}: mean regression {mean_delta:.2f}% > {max_regression_pct:.2f}%"
            )
        if p95_delta > max_regression_pct:
            failures.append(
                f"{key}: p95 regression {p95_delta:.2f}% > {max_regression_pct:.2f}%"
            )

    if failures:
        print("\nIPC regression guardrail failures:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print("\nAll IPC performance guardrails passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
