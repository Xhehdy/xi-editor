# Release Notes

## 0.2.0-dev (2026-02-25)

### Highlights

- Full `cortex` -> `glyph` migration across Rust workspace and sidecar.
- New macOS SwiftUI app (`GlyphApp`) connected to `glyph-core` over framed Unix sockets.
- Revision-safe edit pipeline with stale-revision detection and explicit resync hints.
- IPC contract hardening with stable error codes and retry/resync metadata.
- Performance guardrails for patch engine and process-boundary IPC latency.

### Protocol/Transport Changes

- `UiToCore::ApplyEdit` now includes `base_revision`.
- `CoreToUi::{ViewCreated, ApplyPatch, SetContent}` include `revision`.
- `CoreToUi::Error` now includes:
  - `code`
  - `retryable`
  - `should_resync`

Legacy decoders that only read `message` remain compatible.

### Validation Snapshot

- `cargo test --workspace` (glyph) passes.
- Swift app build (`xcodebuild ... build`) passes.
- `scripts/ci_perf_guard.sh` passes with current guardrails.

### Known Environment Limitations

- `xcodebuild test` may fail in sandboxed environments due `testmanagerd` restrictions, independent of source correctness.
