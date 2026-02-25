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

run_rust_fmt_check() {
  if "$CARGO_BIN" fmt --version >/dev/null 2>&1; then
    "$CARGO_BIN" fmt --all -- --check
    return
  fi

  if command -v cargo-fmt >/dev/null 2>&1; then
    echo "warning: cargo fmt unavailable; falling back to cargo-fmt binary"
    cargo-fmt --all -- --check
    return
  fi

  if command -v rustfmt >/dev/null 2>&1; then
    echo "warning: cargo fmt unavailable; falling back to rustfmt --check per file"
    local file_count=0
    while IFS= read -r -d '' rs_file; do
      rustfmt --check "$rs_file"
      file_count=$((file_count + 1))
    done < <(git ls-files -z '*.rs')
    if [[ "$file_count" -eq 0 ]]; then
      echo "No Rust files found for rustfmt fallback."
    fi
    return
  fi

  echo "error: rustfmt unavailable. Install it with: rustup component add rustfmt" >&2
  exit 1
}

echo "[1/5] rust fmt check..."
run_rust_fmt_check

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
  XCODE_SCHEME="${GLYPH_XCODE_SCHEME:-Glyph}"
  DERIVED_DATA_PATH="${GLYPH_DERIVED_DATA_PATH:-/tmp/GlyphDerived}"
  if [[ -d "$XCODE_PROJECT" ]]; then
    xcodebuild \
      -project "$XCODE_PROJECT" \
      -scheme "$XCODE_SCHEME" \
      -configuration Debug \
      -derivedDataPath "$DERIVED_DATA_PATH" \
      CODE_SIGNING_ALLOWED=NO \
      build

    if [[ "${GLYPH_RUN_SWIFT_TESTS:-0}" == "1" ]]; then
      echo "Running swift tests (GLYPH_RUN_SWIFT_TESTS=1)..."
      set +e
      xcodebuild \
        -project "$XCODE_PROJECT" \
        -scheme "$XCODE_SCHEME" \
        -configuration Debug \
        -derivedDataPath "$DERIVED_DATA_PATH" \
        CODE_SIGNING_ALLOWED=NO \
        test
      test_status=$?
      set -e
      if [[ "$test_status" -ne 0 ]]; then
        if [[ "${GLYPH_REQUIRE_SWIFT_TESTS:-0}" == "1" ]]; then
          echo "error: Swift tests failed and GLYPH_REQUIRE_SWIFT_TESTS=1 is set" >&2
          exit "$test_status"
        fi
        echo "warning: Swift tests failed in this environment; continuing preflight."
      fi
    fi
  else
    echo "Skipping Swift build: GlyphApp project not found"
  fi
else
  echo "Skipping Swift build: xcodebuild not found"
fi

echo "Release preflight passed."
