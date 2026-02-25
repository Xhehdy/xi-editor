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
