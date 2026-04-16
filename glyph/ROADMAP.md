# Glyph Roadmap

## Goal

Turn Glyph into a trustworthy private alpha on macOS first, then push it toward a serious VS Code challenger by layering graph-aware and AI-native workflows on top of solid IDE fundamentals.

## Product Stance

- macOS first, with core/protocol choices that keep later frontends possible.
- LSP-heavy for correctness in language tooling.
- Graph and AI differentiate the product; they do not replace core IDE basics.
- Human-in-the-loop by default for AI-assisted changes.

## Current Snapshot

- Implemented now:
  - revision-safe edit pipeline between `GlyphApp` and `glyph-core`
  - atomic save behavior with explicit save conflict errors
  - dirty-state and disk-change surfacing in the macOS UI
  - on-open indexing, symbol search, and graph-backed chat context
  - optional sidecar-backed chat and ghost-text completion
- Still notably missing:
  - autosave
  - workspace-wide crawl/watcher-driven indexing
  - first-class graph inspection UI
  - LSP-backed IDE workflows
  - preview-first AI action flows

## Current Focus

- [ ] Phase 1: trustworthy editor foundations
- [ ] Phase 2: workspace indexing as a product feature
- [ ] Phase 3: private alpha IDE loop
- [ ] Phase 4: challenger differentiation

## Phase 1: Trustworthy Editor Foundations

### Scope

- [x] Real save behavior in core with atomic disk writes
- [ ] Autosave with debounce
- [x] External file change detection in core
- [x] Reload/conflict handling for dirty buffers
- [x] Dirty-state and save/reload UX in the macOS app
- [x] Preserve deterministic edit and revision guarantees

### Exit Criteria

- [ ] Open, edit, save, and relaunch without data loss
- [x] External edits are detected and surfaced clearly
- [x] Save failures and conflict states are explicit and recoverable

## Phase 2: Workspace Indexing as a Product

### Scope

- [ ] Workspace crawl and initial indexing flow
- [x] Queue-backed incremental reindex
- [ ] Watcher-triggered invalidation and refresh
- [ ] Visible index progress, queue health, and last error
- [ ] First-class graph search and file graph inspection in the UI
- [x] Deterministic graph cleanup and repeated reindex stability

### Exit Criteria

- [ ] Full workspace indexing can be started and observed from the UI
- [ ] File changes reindex automatically
- [ ] Users can inspect graph/search results outside chat

## Phase 3: Private Alpha IDE Loop

### Scope

- [ ] LSP-backed diagnostics for Rust, Swift, TypeScript/JavaScript, Python, and Go
- [ ] Hover, definition, references, rename, and formatting
- [ ] Problems panel and navigation/jump history
- [ ] Symbol-to-location jump flows
- [ ] Lightweight command surface for common actions
- [ ] Clear setup/error states when language servers are missing

### Exit Criteria

- [ ] Small alpha users can do real work in supported languages
- [ ] Missing or failed language servers are surfaced clearly
- [ ] IDE workflows are more reliable than the current editor-plus-chat loop

## Phase 4: Challenger Differentiation

### Scope

- [ ] Richer graph-aware chat with visible provenance
- [ ] Better inline completion grounded in project context
- [ ] Preview-first explain/refactor/fix flows with explicit approval
- [ ] Intent-driven action entry points from chat/comments
- [ ] AI action history with recoverable diff previews
- [ ] Search/navigation/session polish
- [ ] Minimal source-control awareness if time remains

### Exit Criteria

- [ ] Glyph offers a distinct graph-aware workflow users prefer
- [ ] AI actions are auditable and safe enough for repeated use
- [ ] The alpha can expand toward beta without rewriting the architecture

## Verification Gates

- [ ] `cargo test --workspace`
- [ ] `xcodebuild -project GlyphApp/Glyph.xcodeproj -scheme Glyph -configuration Debug -derivedDataPath /tmp/GlyphDerived CODE_SIGNING_ALLOWED=NO build`
- [ ] Protocol roundtrip coverage for new message families
- [ ] Existing performance guardrails remain intact

## Notes

- Use this file as the working checklist for roadmap execution.
- Mark work complete only after code, tests, and build verification land together.
- The snapshot above is intended to keep this checklist aligned with the current repository state.
