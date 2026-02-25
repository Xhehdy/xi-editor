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

export PATH="$(dirname "$CARGO_BIN"):$PATH"

echo "[1/5] rust fmt check..."
"$CARGO_BIN" fmt --all -- --check

echo "[2/5] rust clippy..."
"$CARGO_BIN" clippy --all -- -D warnings

echo "[3/5] rust tests..."
"$CARGO_BIN" test --all

echo "[4/5] perf guardrails..."
bash scripts/ci_perf_guard.sh

echo "[5/5] swift build (if available)..."
if command -v xcodebuild >/dev/null 2>&1; then
  REPO_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
  XCODE_PROJECT="$REPO_ROOT/GlyphApp/Glyph.xcodeproj"
  if [[ -d "$XCODE_PROJECT" ]]; then
    xcodebuild \
      -project "$XCODE_PROJECT" \
      -scheme "Glyph" \
      -configuration Debug \
      -derivedDataPath /tmp/GlyphDerived \
      CODE_SIGNING_ALLOWED=NO \
      build
  else
    echo "Skipping Swift build: GlyphApp project not found"
  fi
else
  echo "Skipping Swift build: xcodebuild not found"
fi

echo "Release preflight passed."
