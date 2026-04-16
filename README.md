# Glyph / Xi Editor Repository

This repository now carries two related tracks:

- Active work on `Glyph`, a macOS-native IDE built on Xi-inspired editor-core ideas.
- Preserved `xi-editor` source, docs, and experiments for historical reference.

The old Xi material still matters here, but it is no longer the whole story for this checkout.

## Current status

- Active implementation work lives in `glyph/` and `GlyphApp/`.
- The current frontend is native macOS SwiftUI/AppKit, not the older Xi Cocoa frontend list.
- The historical Xi code and docs remain in `rust/` and `docs/`.
- Legacy Xi documents are preserved for context; they should be read as archive/reference material unless they explicitly say otherwise.

## Repo map

- `glyph/` — current Rust workspace for the Glyph core, protocol, graph/indexing, and AI/runtime crates.
- `GlyphApp/` — current macOS frontend built with SwiftUI.
- `glyph/sidecar/` — optional Python sidecar used by the current chat/completion path.
- `rust/` — historical Xi core and supporting crates.
- `docs/` — archived Xi documentation site and technical essays.

## Getting started with Glyph

Build and test the active Rust workspace:

```bash
cd glyph
cargo build --workspace
cargo test --workspace
```

Run the current core process:

```bash
cd glyph
cargo run -p glyph-core
```

Build the macOS app:

```bash
xcodebuild \
  -project GlyphApp/Glyph.xcodeproj \
  -scheme Glyph \
  -configuration Debug \
  -derivedDataPath /tmp/GlyphDerived \
  CODE_SIGNING_ALLOWED=NO \
  build
```

Run the local release/preflight checks:

```bash
cd glyph
bash scripts/release_preflight.sh
```

More detailed Glyph setup, sidecar notes, and current feature status live in `glyph/README.md`.

## Legacy Xi archive

The original Xi project material is still here:

- Historical core: `rust/`
- Historical docs site: `docs/`
- Historical sample plugins and experiments: `rust/sample-plugin/`, `python/`, `rfcs/`

That material should be treated as archived reference unless a file clearly documents the current Glyph path.

## Contributing

For current work in this checkout:

- Start with `.github/CONTRIBUTING.md`
- Prefer updating `glyph/README.md` and the root README when current workflows change
- Keep legacy Xi docs accurate as archive/reference material, not as the default product narrative

## License

This project is licensed under Apache 2.0. See `LICENSE`.
