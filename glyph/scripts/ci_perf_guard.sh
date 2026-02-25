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
TARGET_DIR="${CARGO_TARGET_DIR:-target}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required for benchmark guardrail checks" >&2
  exit 1
fi

echo "Running glyph performance benchmarks..."
if [[ "${GLYPH_SKIP_PATCH_BENCH:-0}" != "1" ]]; then
  "$CARGO_BIN" bench -p glyph-patch --bench diff_apply -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2
else
  echo "Skipping patch benchmark because GLYPH_SKIP_PATCH_BENCH=1"
fi

echo "Checking benchmark guardrails..."
CRITERION_ROOT="${GLYPH_CRITERION_ROOT:-}"
if [[ -z "$CRITERION_ROOT" ]]; then
  for candidate in "$TARGET_DIR/criterion" "crates/glyph-patch/target/criterion" "target/criterion"; do
    if [[ -d "$candidate" ]]; then
      CRITERION_ROOT="$candidate"
      break
    fi
  done
fi
if [[ -z "$CRITERION_ROOT" || ! -d "$CRITERION_ROOT" ]]; then
  echo "error: criterion output directory not found. Set GLYPH_CRITERION_ROOT to override." >&2
  exit 1
fi

python3 scripts/check_perf_regression.py \
  --guardrails scripts/perf_guardrails.json \
  --criterion-root "$CRITERION_ROOT"

if [[ "${GLYPH_SKIP_IPC_BENCH:-0}" != "1" ]]; then
  echo "Building glyph core release binary for IPC benchmark..."
  "$CARGO_BIN" build -p glyph-core --release

  CORE_BINARY="${GLYPH_CORE_BINARY:-$TARGET_DIR/release/glyph}"
  if [[ ! -x "$CORE_BINARY" ]]; then
    echo "error: core binary not found/executable at '$CORE_BINARY'" >&2
    exit 1
  fi

  echo "Running IPC benchmark..."
  IPC_RESULTS="$(mktemp -t glyph_ipc_bench.XXXXXX)"
  trap 'rm -f "$IPC_RESULTS"' EXIT
  python3 scripts/bench_ipc_apply_edit.py \
    --spawn-core \
    --core-binary "$CORE_BINARY" \
    --output "$IPC_RESULTS"

  echo "Checking IPC benchmark guardrails..."
  python3 scripts/check_ipc_guardrails.py \
    --observed "$IPC_RESULTS" \
    --guardrails scripts/ipc_perf_guardrails.json
else
  echo "Skipping IPC benchmark because GLYPH_SKIP_IPC_BENCH=1"
fi
