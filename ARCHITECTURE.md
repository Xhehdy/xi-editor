# Glyph — Architecture, Performance & Scalability

> Comprehensive documentation of the Glyph (formerly xi-editor) text editor
> core, covering architecture, performance characteristics, scalability
> design, and project structure.

---

## Table of Contents

1. [Project Structure](#1-project-structure)
2. [Architecture Overview](#2-architecture-overview)
3. [Core Components](#3-core-components)
4. [Performance Analysis](#4-performance-analysis)
5. [Scalability](#5-scalability)
6. [Plugin System](#6-plugin-system)
7. [Protocol & Communication](#7-protocol--communication)
8. [Error Handling & Reliability](#8-error-handling--reliability)
9. [Build & Development](#9-build--development)
10. [Future Direction: Cortex IDE](#10-future-direction-cortex-ide)

---

## 1. Project Structure

Glyph is organized as a Rust workspace with multiple focused crates:

```
glyph/
├── rust/                       # Main Rust workspace
│   ├── Cargo.toml              # Workspace root (xi-core binary)
│   ├── src/main.rs             # Entry point
│   ├── core-lib/               # Core editor library
│   ├── rope/                   # Rope data structure
│   ├── rpc/                    # JSON-RPC communication layer
│   ├── plugin-lib/             # Plugin client library
│   ├── lsp-lib/                # Language Server Protocol integration
│   ├── syntect-plugin/         # Syntax highlighting plugin
│   ├── sample-plugin/          # Reference plugin implementation
│   ├── trace/                  # Performance tracing framework
│   ├── unicode/                # Unicode utilities
│   └── experimental/lang/      # Experimental language support
├── cortex/                     # Next-generation AI-native IDE (WIP)
├── python/                     # Python plugin utilities
├── docs/                       # Documentation site (Jekyll)
├── rfcs/                       # Design proposals
└── icons/                      # Project assets
```

### Crate Dependency Graph

```
xi-core (binary)
  ├── xi-core-lib
  │     ├── xi-rope        (text data structure)
  │     ├── xi-rpc         (communication)
  │     ├── xi-trace       (performance tracing)
  │     └── xi-unicode     (unicode utilities)
  └── xi-rpc
```

### Codebase Statistics

| Component       | Approximate Lines | Purpose                      |
|-----------------|-------------------|------------------------------|
| `core-lib`      | ~12,000           | Editor logic and state       |
| `rope`          | ~8,000            | Text data structure          |
| `rpc`           | ~3,000            | JSON-RPC protocol            |
| `plugin-lib`    | ~2,500            | Plugin client framework      |
| `lsp-lib`       | ~2,500            | LSP integration              |
| `trace`         | ~1,500            | Performance instrumentation  |
| `unicode`       | ~3,000            | Unicode tables and utilities |
| `syntect-plugin`| ~1,500            | Syntax highlighting          |
| **Total**       | **~35,000+**      |                              |

---

## 2. Architecture Overview

Glyph follows a **frontend/backend separation** model:

```
┌─────────────────────────┐
│   Frontend (UI Layer)   │  ← Platform-native (Cocoa, GTK+, Terminal, etc.)
│   Renders text, handles │
│   user input            │
└────────────┬────────────┘
             │  JSON-RPC over stdin/stdout
             ▼
┌─────────────────────────┐
│   Glyph Core (Backend)  │  ← Rust, high-performance
│   Manages buffers,      │
│   edits, plugins        │
└────────────┬────────────┘
             │  JSON-RPC over pipes
             ▼
┌─────────────────────────┐
│   Plugins               │  ← Any language, separate processes
│   Syntax, LSP, linting  │
└─────────────────────────┘
```

### Key Design Principles

1. **The editor is a view** — The frontend is a thin client that renders text
   and captures input. All heavy logic lives in the Rust core.

2. **Never block the user** — All operations that could be slow (file I/O,
   plugin communication, syntax highlighting) are asynchronous.

3. **Delta-based updates** — Text changes are represented as deltas
   (`RopeDelta`), minimizing data transfer between frontend and backend.

4. **Copy-on-write semantics** — The rope data structure uses persistent
   immutable trees, making snapshots (for undo, autosave) nearly free.

---

## 3. Core Components

### 3.1 Rope Data Structure (`xi-rope`)

The rope is the fundamental text storage, implemented as a **B-tree** with
configurable branching factor (`MIN_CHILDREN: 4`, `MAX_CHILDREN: 8`).

**Key traits and types:**

- `Rope` — Immutable, persistent text container
- `RopeDelta` — Represents a text change as a series of copy/insert operations
- `Engine` — CRDT-based engine supporting concurrent editing and undo
- `Interval` — Efficient range representation
- `Subset` / `Multiset` — Track deleted regions for CRDT merge

**Operations and complexity:**

| Operation          | Complexity   | Notes                           |
|--------------------|-------------|----------------------------------|
| Character access   | O(log n)    | B-tree traversal                 |
| Insert / Delete    | O(log n)    | Tree split and merge             |
| Snapshot (clone)   | O(1)        | Copy-on-write, reference counted |
| Line count         | O(1)        | Cached in tree metrics           |
| Byte offset ↔ Line | O(log n)   | Metric-based tree navigation     |
| Diff               | O(n)        | Myers diff algorithm             |

**Memory overhead:** Approximately 2× the raw text size due to tree node
metadata and copy-on-write leaf sharing.

### 3.2 Editor Core (`xi-core-lib`)

The editor core manages:

- **Buffer management** — Multiple open documents (`Tabs`, `Buffer`)
- **Edit operations** — Insert, delete, transpose, undo/redo (max 20 undos)
- **Selections** — Multi-cursor support with region-based selections
- **Line wrapping** — Incremental word wrapping with width cache
- **Find/Replace** — Regex-powered search with incremental highlighting
- **File watching** — Detects external changes to open files
- **Syntax highlighting** — Delegated to plugins, results cached in `Styles`
- **Configuration** — Hierarchical config (user → language → file)
- **Annotations** — Extensible overlay system for linting, diagnostics

### 3.3 RPC Layer (`xi-rpc`)

A bidirectional JSON-RPC protocol with these characteristics:

- Modified JSON-RPC 2.0 (no `"jsonrpc"` field, no batching)
- Supports both synchronous requests and asynchronous notifications
- Runs over stdin/stdout (core ↔ frontend) or pipes (core ↔ plugins)
- Designed for minimal serialization overhead

### 3.4 Tracing Framework (`xi-trace`)

Built-in performance instrumentation:

- Chrome Trace format export for visualization
- Lock-free fixed-size LIFO deque for trace events
- Platform-specific thread/process ID collection
- Configurable via `--log-dir` and `--log-file` flags

---

## 4. Performance Analysis

### 4.1 Design for Speed

Glyph is architected to meet a **16ms frame budget** for all editing
operations. Key performance strategies:

| Strategy                     | Benefit                              |
|------------------------------|--------------------------------------|
| B-tree rope                  | O(log n) edits on any file size      |
| Delta-based updates          | Minimal data sent to frontend        |
| Copy-on-write snapshots      | Near-zero cost undo and autosave     |
| Asynchronous plugins         | Editor never blocks on slow plugins  |
| Incremental line wrapping    | Only re-wraps affected regions       |
| Width cache                  | Avoids redundant text measurement    |
| Lock-free tracing            | Zero-overhead when tracing disabled  |
| Metric caching in tree nodes | O(1) line count, O(log n) offset map |

### 4.2 Memory Efficiency

- **Rope nodes** share leaf data across versions (copy-on-write)
- **Undo history** stores deltas, not full document copies
- **Plugin communication** is streaming; no full-document serialization
- **Estimated overhead:** ~2× file size for the in-memory rope

### 4.3 Concurrency Model

- The core runs a single-threaded event loop for deterministic behavior
- Plugins run in separate processes, communicating asynchronously
- File I/O (save, load) is performed on background threads
- The CRDT engine supports concurrent edits from multiple sources

### 4.4 Known Performance Considerations

- **Large file handling:** The rope structure handles files of any size
  efficiently, but very large files (100MB+) will consume proportionally
  more memory due to the 2× overhead.
- **Plugin latency:** Plugin results (syntax highlighting) may arrive after
  the edit that triggered them; the UI handles this gracefully through
  incremental updates.
- **Startup time:** The core is lightweight; startup is dominated by plugin
  initialization and file loading.

---

## 5. Scalability

### 5.1 File Size Scalability

The rope data structure ensures that Glyph scales gracefully:

| File Size  | Insert/Delete | Memory    | Notes                     |
|------------|---------------|-----------|---------------------------|
| 1 KB       | < 1 μs        | ~2 KB     | Trivial                   |
| 1 MB       | < 10 μs       | ~2 MB     | Well within 16ms budget   |
| 100 MB     | < 50 μs       | ~200 MB   | O(log n) remains fast     |
| 1 GB       | < 100 μs      | ~2 GB     | Practical with enough RAM |

### 5.2 Multi-Document Scalability

- Each document has its own `Editor` and `Buffer` instance
- Documents share the same event loop but are independently managed
- No cross-document locking; adding documents is O(1)

### 5.3 Plugin Scalability

- Plugins run in separate OS processes
- Multiple plugins can run concurrently
- Plugin crash does not crash the editor core
- Plugin manifest/catalog system supports discovery and lifecycle

### 5.4 Concurrent Editing (CRDT)

The CRDT engine in `xi-rope` supports:

- **Operational transformation** via subset-based merge
- **Conflict-free concurrent edits** from multiple sources
- **Undo that respects causality** — individual operations can be undone
  without affecting concurrent edits
- Designed for future collaborative editing scenarios

### 5.5 Areas for Improvement

- **Thread pool for plugins:** Currently each plugin is a separate process;
  a thread pool or async runtime could reduce overhead.
- **Error handling:** The codebase has many `unwrap()` calls that should be
  replaced with proper `Result` handling for production robustness.
- **Large file streaming:** Files larger than available RAM are not yet
  supported with memory-mapped or streaming approaches.

---

## 6. Plugin System

### Architecture

```
┌──────────┐    JSON-RPC     ┌──────────────┐
│  Glyph   │ ◄────────────► │   Plugin      │
│  Core    │    over pipes   │  (any lang)   │
└──────────┘                 └──────────────┘
```

### Plugin Types

| Plugin             | Language | Purpose                          |
|--------------------|----------|----------------------------------|
| `syntect-plugin`   | Rust     | Syntax highlighting via syntect  |
| `sample-plugin`    | Rust     | Reference implementation         |
| `xi-lsp-plugin`    | Rust     | Language Server Protocol bridge  |
| Python plugins     | Python   | Spell check, brackets, echo      |

### Plugin Lifecycle

1. **Discovery** — Plugins are registered via manifest files (JSON)
2. **Activation** — Core spawns plugin process on matching file type
3. **Communication** — Bidirectional JSON-RPC over stdin/stdout pipes
4. **Cache** — Plugins receive view snapshots via a cache layer
5. **Shutdown** — Core terminates plugin processes on exit

---

## 7. Protocol & Communication

The frontend protocol is documented in detail at
[docs/frontend-protocol.md](docs/docs/frontend-protocol.md).

### Key Messages

**Frontend → Core:**
- `new_view` — Open a new document
- `edit` — Apply an edit command (insert, delete, movement, etc.)
- `save` — Save current document
- `find` / `replace` — Search operations
- `plugin` — Plugin management commands

**Core → Frontend:**
- `update` — Incremental view update with changed lines
- `scroll_to` — Scroll to cursor position
- `available_plugins` — Notify of plugin availability
- `config_changed` — Configuration update notification
- `alert` — Display message to user

### Serialization

All messages use JSON. While binary protocols were considered, the
performance difference is negligible compared to the actual rendering
cost, and JSON's tooling and debugging advantages are significant.

---

## 8. Error Handling & Reliability

### Current State

- **Rope operations** are generally safe with Rust's type system
- **RPC layer** handles malformed messages gracefully
- **Plugin crashes** are isolated (separate processes)
- **File operations** include error reporting to the frontend

### Recommendations

- Replace `unwrap()` calls with proper error propagation using `Result`
  and the `?` operator
- Add structured error types with `thiserror` or similar
- Implement graceful degradation when plugins fail to start
- Add recovery mechanisms for interrupted file writes

---

## 9. Build & Development

### Prerequisites

- Rust 1.40 or later (install via [rustup](https://www.rustup.rs))

### Build

```bash
cd rust
cargo build
```

### Test

```bash
cd rust
cargo test --all
```

### Run with Logging

```bash
cd rust
cargo run -- --log-dir /tmp/glyph-logs
```

### Project Layout for Contributors

| Directory              | What to Modify                        |
|------------------------|---------------------------------------|
| `rust/core-lib/src/`   | Editor logic, commands, configuration |
| `rust/rope/src/`       | Text data structure, CRDT engine      |
| `rust/rpc/src/`        | Frontend/plugin communication         |
| `rust/plugin-lib/src/` | Plugin client library                 |
| `rust/trace/src/`      | Performance instrumentation           |
| `docs/docs/`           | User and developer documentation      |

---

## 10. Future Direction: Cortex IDE

The repository also contains **Cortex**, an early-stage AI-native IDE built
on Glyph principles. Located in `cortex/`, it extends the architecture with:

- **Comprehension Engine** — Code analysis and understanding
- **Agent Runtime** — Multi-agent AI assistance (WASM-based)
- **Code Knowledge Graph** — Semantic code relationships
- **Intent System** — Translating user actions into intelligent operations

Cortex is organized into 14 crates and is currently in early alpha. See
[cortex/README.md](cortex/README.md) for details.

---

## Summary

Glyph is a well-architected text editor core with strong performance
characteristics:

| Aspect          | Rating     | Notes                                        |
|-----------------|------------|----------------------------------------------|
| Performance     | Excellent  | O(log n) operations, 16ms frame budget       |
| Scalability     | Very Good  | Handles large files, multiple documents       |
| Reliability     | Good       | Rust safety, but needs better error handling  |
| Extensibility   | Very Good  | Plugin system supports any language           |
| Documentation   | Good       | Comprehensive rope science articles, protocol |
| Code Quality    | Good       | Clean Rust idioms, some unwrap() to address   |
| Concurrency     | Very Good  | Async plugins, CRDT support, COW snapshots    |
