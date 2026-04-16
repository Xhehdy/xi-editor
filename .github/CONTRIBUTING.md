# Contributing

This repository contains both current Glyph work and preserved Xi material. Please keep those two tracks clearly separated when you make changes.

## Where current work lives

- Current product code: `glyph/`
- Current macOS frontend: `GlyphApp/`
- Current optional AI sidecar: `glyph/sidecar/`

## What is preserved as archive/reference

- Historical Xi core: `rust/`
- Historical Xi docs site: `docs/`
- Historical Xi plugins, RFCs, and experiments: `python/`, `rfcs/`, `rust/sample-plugin/`

Archived Xi content is valuable, but it should not be presented as the default current product story unless a change is intentionally about the legacy project.

## Before you open a PR

- Scope the change clearly:
  - current Glyph/macOS work
  - legacy Xi/archive maintenance
  - documentation cleanup across both
- Update docs when behavior or setup changes:
  - `README.md` for the repository-level story
  - `glyph/README.md` for active setup and workflow changes
  - archived Xi docs only when you are intentionally correcting or preserving that historical material

## Suggested checks

Run the checks that match the code you touched.

For Rust workspace changes in `glyph/`:

```bash
cd glyph
cargo test --workspace
```

For broader Glyph validation:

```bash
cd glyph
bash scripts/release_preflight.sh
```

If you touch the macOS app:

```bash
xcodebuild \
  -project GlyphApp/Glyph.xcodeproj \
  -scheme Glyph \
  -configuration Debug \
  -derivedDataPath /tmp/GlyphDerived \
  CODE_SIGNING_ALLOWED=NO \
  build
```

If you touch the optional sidecar, verify the setup notes in `glyph/README.md` still match the code and dependencies in `glyph/sidecar/`.

## Review expectations

- Prefer small, intention-revealing diffs.
- Call out whether a doc describes current Glyph behavior or archived Xi behavior.
- If a change affects protocol or persistence semantics, document the failure modes and compatibility story.
- If a change updates user-visible setup, include the exact command path that was verified.

## Documentation guidance

- Preserve legacy Xi material with respect.
- Add clear archive labels when a historical page could be mistaken for the current implementation.
- Prefer current Glyph docs in `README.md` and `glyph/README.md` for new setup information.
