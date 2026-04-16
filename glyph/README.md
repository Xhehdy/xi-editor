# Glyph IDE

> A macOS-native IDE built on Xi-inspired editor-core ideas: performance first, intelligence second.

This README describes the current Glyph implementation. Historical Xi documentation is still preserved in the repository root and `../docs/`, but the active product surface today is `glyph/` plus `../GlyphApp/`.

## Current shape

```text
SwiftUI / AppKit macOS app
    ↓ framed Unix socket IPC
Rust core (`glyph-core`)
    ↓
Comprehension + indexing + graph layers
    ↓
Optional AI sidecar for chat/completion
```

## Workspace layout

- `crates/glyph-core` — main core process and editor coordination.
- `crates/glyph-protocol` — typed IPC contract between UI and core.
- `crates/glyph-buffer`, `crates/glyph-rope`, `crates/glyph-patch` — editing primitives.
- `crates/glyph-ce`, `crates/glyph-graph`, `crates/glyph-vectors` — comprehension, graph, and retrieval layers.
- `crates/glyph-llm` — core-side HTTP client for AI calls.
- `crates/glyph-agents` — early WASM runtime groundwork.
- `sidecar/` — optional Python sidecar used by the current chat and ghost-text flows.
- `../GlyphApp/` — macOS-native SwiftUI frontend for the current system.

## Prerequisites

- Recent stable Rust toolchain
- Python 3 for the optional sidecar and benchmark scripts
- Xcode if you want to build or run the macOS app

## Quick start

All Rust commands below assume you are inside this directory:

```bash
cd glyph
```

Build the workspace:

```bash
cargo build --workspace
```

Run the Rust tests:

```bash
cargo test --workspace
```

Run the core:

```bash
cargo run -p glyph-core
```

The core listens on `/tmp/glyph.sock`.

## Running the macOS app

Build the core first so the app can auto-launch it from a standard path:

```bash
cd glyph
cargo build -p glyph-core
```

Then build the app from the repository root:

```bash
xcodebuild \
  -project GlyphApp/Glyph.xcodeproj \
  -scheme Glyph \
  -configuration Debug \
  -derivedDataPath /tmp/GlyphDerived \
  CODE_SIGNING_ALLOWED=NO \
  build
```

Useful notes:

- `GlyphApp` tries to auto-launch the core if it finds an executable at `glyph/target/debug/glyph`, `glyph/target/release/glyph`, or `GLYPH_CORE_BIN`.
- If the core is already running, the app will connect to `/tmp/glyph.sock`.
- Normal editing works without the AI sidecar; chat and inline completion do not.

## Running the AI sidecar

The current checked-in sidecar is optional, but it powers the current chat and ghost-text completion path.

Install dependencies:

```bash
cd glyph/sidecar
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

Start it:

```bash
cd glyph/sidecar
export OPENAI_API_KEY=your-key
export GLYPH_OPENAI_MODEL=gpt-4o-mini
python3 main.py
```

Important nuance on models/providers:

- The current bundled sidecar uses `langchain_openai`, so the checked-in adapter is OpenAI-compatible today.
- Model choice is still configurable via `GLYPH_OPENAI_MODEL`.
- The broader Glyph direction is not tied to a single model family, but this repository does not yet ship dedicated in-tree adapters and setup docs for every provider.

If the sidecar is not available at `http://127.0.0.1:8000`, the core still runs, but chat/completion return retryable AI errors.

## Current capabilities

- Revision-safe, UTF-8 byte-offset editing over a typed protocol
- Atomic file save behavior with explicit conflict detection
- Dirty-state and disk-change surfacing in the macOS editor UI
- On-open file indexing and symbol search
- Workspace-local CKG persistence at `.glyph/ckg.sqlite`
- Ranked graph context attachment for chat requests when graph data exists

## Protocol notes

`glyph-protocol` provides a typed IPC contract with:

- Revision-aware edits via `base_revision` and response `revision`
- Stable error codes plus `retryable` / `should_resync` hints
- MessagePack on the live socket path, with JSON helpers retained for compatibility/testing

## CKG behavior

- Default graph DB path: `.glyph/ckg.sqlite`
- Override path with `GLYPH_CKG_PATH`
- If graph initialization fails, the core logs a warning and falls back to an in-memory graph
- `UiToCore::IndexFile` performs deterministic per-file rebuilds
- Queue-based incremental indexing exists in the core/protocol, even though the current UI mostly drives explicit file indexing

## Validation and release checks

Performance guardrails:

```bash
cd glyph
bash scripts/ci_perf_guard.sh
```

Release preflight:

```bash
cd glyph
bash scripts/release_preflight.sh
```

Optional preflight env knobs:

- `GLYPH_RUN_SWIFT_TESTS=1` to run `xcodebuild test` in addition to build
- `GLYPH_REQUIRE_SWIFT_TESTS=1` to fail preflight when Swift tests fail
- `GLYPH_DERIVED_DATA_PATH=/custom/path` to override derived data output
- `GLYPH_SKIP_PATCH_BENCH=1` and/or `GLYPH_SKIP_IPC_BENCH=1` to bypass specific perf checks

## Release tracking

- Release notes: `RELEASE_NOTES.md`
- Changelog: `CHANGELOG.md`
- Execution checklist: `ROADMAP.md`

## License

Apache 2.0
