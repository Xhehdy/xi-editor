#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CARGO_BIN="${CARGO_BIN:-$(command -v cargo || true)}"
if [[ -z "$CARGO_BIN" ]] && command -v rustup >/dev/null 2>&1; then
  CARGO_BIN="$(rustup which cargo 2>/dev/null || true)"
fi

if [[ -z "$CARGO_BIN" ]]; then
  echo "error: cargo not found on PATH and rustup could not locate it" >&2
  exit 1
fi

# Ensure sibling Rust toolchain binaries (rustc, rustdoc) are resolvable.
export PATH="$(dirname "$CARGO_BIN"):$PATH"

echo "Running glyph performance benchmarks..."
"$CARGO_BIN" bench -p glyph-patch --bench diff_apply -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2

echo "Checking benchmark guardrails..."
CRITERION_ROOT="crates/glyph-patch/target/criterion"
if [[ ! -d "$CRITERION_ROOT" && -d "target/criterion" ]]; then
  CRITERION_ROOT="target/criterion"
fi

python3 scripts/check_perf_regression.py \
  --guardrails scripts/perf_guardrails.json \
  --criterion-root "$CRITERION_ROOT"

echo "Building glyph core release binary for IPC benchmark..."
"$CARGO_BIN" build -p glyph-core --release

echo "Running IPC benchmark..."
IPC_RESULTS="$(mktemp -t glyph_ipc_bench)"
python3 scripts/bench_ipc_apply_edit.py \
  --spawn-core \
  --core-binary target/release/glyph \
  --output "$IPC_RESULTS"

echo "Checking IPC benchmark guardrails..."
python3 scripts/check_ipc_guardrails.py \
  --observed "$IPC_RESULTS" \
  --guardrails scripts/ipc_perf_guardrails.json
