# Changelog

All notable changes to this project are documented here.

## 0.2.0-dev (2026-02-25)

- Migrated workspace namespace from `cortex-*` to `glyph-*`.
- Added `GlyphApp` macOS SwiftUI client and socket IPC integration.
- Added revision-aware edit contract:
  - `UiToCore::ApplyEdit.base_revision`
  - `CoreToUi::{ViewCreated,ApplyPatch,SetContent}.revision`
- Hardened IPC error contract with stable error codes and recovery hints:
  - `CoreToUi::Error.code`
  - `CoreToUi::Error.retryable`
  - `CoreToUi::Error.should_resync`
- Added replay/out-of-order/reconnect contract tests in `glyph-core`.
- Added performance benchmark guardrails (diff/patch + IPC RTT) and CI wiring.
- Added release preflight script for local ship checks.
- Added CKG MVP persistence in `glyph-core`:
  - default graph DB path `.glyph/ckg.sqlite`
  - optional `GLYPH_CKG_PATH` override
  - startup fallback to in-memory graph on SQLite init failure
- Added deterministic CKG reindex flow on `UiToCore::IndexFile` with per-file delete+rebuild.
- Added graph query protocol surface:
  - `UiToCore::{QueryGraphFile,SearchGraph}`
  - `CoreToUi::GraphData`
  - `ErrorCode::GraphQueryFailed`
- Added Rust + Swift tests for graph protocol roundtrips, decoding, persistence, limits, and error mapping.
- Added initial semantic CKG edge extraction during `IndexFile`:
  - AST-backed Rust `imports` edges from `use_declaration`
  - AST-backed Rust `implements` edges from `impl_item`
  - AST-backed local Rust `calls` edges from `call_expression` / `method_call_expression`
  - AST-backed JavaScript `imports`, class inheritance `implements`, and local `calls`
  - AST-backed Python `imports`, class inheritance `implements`, and local `calls`
  - heuristic fallback for unsupported files or parser failures
- Added edge provenance metadata persisted in graph edge payloads:
  - `source`, `language`, `timestamp_unix`, `confidence`, `extractor_version`
- Added incremental indexing pipeline (core + protocol):
  - `UiToCore::{QueueIndexFile,GetIndexQueueStats,FlushIndexQueue}`
  - `CoreToUi::{IndexQueued,IndexQueueStats,IndexQueueFlushed}`
  - bounded pending queue with debounce, retry/backoff, and cancellation support
  - new error code `ErrorCode::IndexQueueFull`
- Added graph query hardening (core + protocol):
  - deterministic node/edge ordering for `QueryGraphFile` and `SearchGraph`
  - `UiToCore::SearchGraph` optional pagination/filter fields:
    `offset`, `kind_filter`, `file_filter`
  - backward-compatible decoding for legacy `SearchGraph { query, limit }` payloads
- Added CKG context-ranking API and chat consumption path:
  - `UiToCore::QueryGraphContext { query, context_files?, limit? }`
  - `CoreToUi::GraphContext { summary, items }`
  - deterministic ranking with file-scope hints, graph degree scoring, and bounded limits
  - `UiToCore::Chat` now attaches ranked CKG context when available
- Hardened preflight/perf tooling scripts for CI portability:
  - `release_preflight.sh` now has rustfmt fallbacks and clearer install guidance
  - optional Swift test gating via `GLYPH_RUN_SWIFT_TESTS` / `GLYPH_REQUIRE_SWIFT_TESTS`
  - configurable derived data path via `GLYPH_DERIVED_DATA_PATH`
  - perf script now supports target-dir/core-binary overrides and optional stage skips
