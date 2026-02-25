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

## Release

```bash
bash scripts/release_preflight.sh
```

- Release notes: [`RELEASE_NOTES.md`](./RELEASE_NOTES.md)
- Changelog: [`CHANGELOG.md`](./CHANGELOG.md)

## Philosophy

1. **Editor never blocks on AI** — All AI ops are async
2. **Human-in-the-loop always** — Agents propose, humans approve
3. **Performance is non-negotiable** — 16ms frame budget

## License

Apache 2.0
