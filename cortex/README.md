# Cortex IDE

> An AI-native IDE built on Xi Editor principles. Performance-first, intelligence-second.

## Architecture

```
UI Layer (Tauri)
    ↓ Unix Sockets + MessagePack
Editor Core (Rust)
    ↓
Comprehension Engine (Phase 2)
    ↓
Agent Runtime / WASM (Phase 2)
```

## Quick Start

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run the core (when ready)
cargo run -p cortex-core
```

## Crates

| Crate | Purpose |
|-------|---------|
| `cortex-rope` | Rope data structure (forked from xi-rope) |
| `cortex-buffer` | Text buffer management |
| `cortex-patch` | Diff/patch computation |
| `cortex-events` | Intent & editor events |
| `cortex-protocol` | IPC message definitions |
| `cortex-scheduler` | Async task orchestration |
| `cortex-core` | Main coordinator |

## Philosophy

1. **Editor never blocks on AI** — All AI ops are async
2. **Human-in-the-loop always** — Agents propose, humans approve
3. **Performance is non-negotiable** — 16ms frame budget

## License

Apache 2.0
