# Glyph IDE

> An AI-native IDE built on Xi Editor principles. Performance-first, intelligence-second.

## Architecture

```
UI Layer (SwiftUI / macOS)
    ↓ Unix Sockets + MessagePack-framed payloads
Editor Core (Rust)
    ↓
Comprehension Engine
    ↓
Agent Runtime / WASM
```

## Quick Start

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run the core
cargo run -p glyph-core

# Run performance guardrails
bash scripts/ci_perf_guard.sh
```

## Crates

| Crate | Purpose |
|-------|---------|
| `glyph-rope` | Rope data structure (forked from xi-rope) |
| `glyph-buffer` | Text buffer management |
| `glyph-patch` | Diff/patch computation |
| `glyph-events` | Intent & editor events |
| `glyph-protocol` | IPC message definitions |
| `glyph-scheduler` | Async task orchestration |
| `glyph-core` | Main coordinator |

## IPC Contract Guarantees

`glyph-protocol` provides a versioned, typed message contract for core/ui IPC:

- UTF-8 byte-offset edits with revision checks (`base_revision` / `revision`)
- Stable error classification (`Error.code`) with recovery hints:
  - `retryable`
  - `should_resync`
- MessagePack and JSON helper support for compatibility/testing

## CKG (MVP) Behavior

- CKG persists to workspace-local SQLite by default at `.glyph/ckg.sqlite`.
- Optional override: `GLYPH_CKG_PATH` (absolute or relative path).
- On startup/open failure, core logs a warning and falls back to in-memory graph.
- `UiToCore::IndexFile` is the explicit refresh trigger and rebuilds that file subgraph deterministically.
- Incremental indexing pipeline:
  - `UiToCore::QueueIndexFile` for debounced background indexing
  - bounded queue/backpressure with `ErrorCode::INDEX_QUEUE_FULL`
  - retry with backoff; queue stats via `UiToCore::GetIndexQueueStats`
  - queue cancellation via `UiToCore::FlushIndexQueue`
- Semantic edge extraction uses AST-backed Rust, JavaScript, and Python extraction for `imports`, `implements`, and local `calls`, with heuristics as fallback for unsupported files or parser failures.
- Indexed edges carry provenance metadata (`source`, `language`, `timestamp_unix`, `confidence`, `extractor_version`).
- Graph query results are deterministically ordered for stable rendering/caching.
- New graph protocol messages:
  - `UiToCore::QueryGraphFile { path }`
  - `UiToCore::SearchGraph { query, limit, offset, kind_filter, file_filter }`
  - `UiToCore::QueryGraphContext { query, context_files?, limit? }`
  - `CoreToUi::GraphData { nodes, edges }`
  - `CoreToUi::GraphContext { summary, items }`
- Chat requests (`UiToCore::Chat`) now automatically include ranked CKG context when available.

## Release

```bash
bash scripts/release_preflight.sh
```

Optional preflight env knobs:
- `GLYPH_RUN_SWIFT_TESTS=1` to run `xcodebuild test` in addition to build.
- `GLYPH_REQUIRE_SWIFT_TESTS=1` to fail preflight when Swift tests fail.
- `GLYPH_DERIVED_DATA_PATH=/custom/path` to override derived data output.
- `GLYPH_SKIP_PATCH_BENCH=1` and/or `GLYPH_SKIP_IPC_BENCH=1` to bypass specific perf checks.

- Release notes: [`RELEASE_NOTES.md`](./RELEASE_NOTES.md)
- Changelog: [`CHANGELOG.md`](./CHANGELOG.md)

## Philosophy

1. **Editor never blocks on AI** — All AI ops are async
2. **Human-in-the-loop always** — Agents propose, humans approve
3. **Performance is non-negotiable** — 16ms frame budget

## License

Apache 2.0
