# Quick Fixes & Immediate Improvements

This document lists the most impactful fixes that can be done quickly to improve the xi-editor codebase.

---

## 🔴 Critical Issues (Fix Immediately)

### 1. Fix CI - Clippy Warning
**Impact**: HIGH - CI is currently broken  
**Effort**: 5 minutes  
**File**: `rust/core-lib/src/find.rs:169`

**Problem**:
```rust
if self.search_string.is_some() {
    let is_multiline = LinesMetric::next(self.search_string.as_ref().unwrap(), 0).is_some();
    // ^^^ unnecessary unwrap after is_some check
}
```

**Fix**:
```rust
if let Some(search_string) = &self.search_string {
    let is_multiline = LinesMetric::next(search_string, 0).is_some();
}
```

**Verification**:
```bash
cd rust && cargo clippy --all -- -D warnings
```

---

### 2. Remove Unused Cargo.toml Key
**Impact**: MEDIUM - Causes warnings  
**Effort**: 1 minute  
**File**: `rust/Cargo.toml:10`

**Problem**:
```toml
rust = "1.40"  # Unused manifest key
```

**Fix**: Delete line 10 or change to:
```toml
[package]
name = "xi-core"
version = "0.4.0"
license = "Apache-2.0"
authors = ["Raph Levien <raph@google.com>"]
description = "Main process for xi-core, based on json-rpc"
categories = ["text-editors"]
repository = "https://github.com/xi-editor/xi-editor"
edition = '2018'
rust-version = "1.40"  # Correct key name
```

---

### 3. Fix Cortex Compiler Warnings
**Impact**: MEDIUM - Clean build  
**Effort**: 10 minutes  

**Files & Fixes**:

**a) `cortex/crates/cortex-core/src/lib.rs:289`**
```rust
// Remove mut from:
if let Ok(mut graph) = self.graph.lock() {
// Change to:
if let Ok(graph) = self.graph.lock() {
```

**b) `cortex/crates/cortex-agents/src/lib.rs:10`**
```rust
// Remove unused import:
use tracing::{info, warn};  // Remove 'warn'
// Change to:
use tracing::info;
```

**c) `cortex/crates/cortex-agents/src/lib.rs:96,145`**
```rust
// Prefix unused variables with underscore:
let module = Module::new(&self.engine, wasm_bytes)
// Change to:
let _module = Module::new(&self.engine, wasm_bytes)

let instance = linker.instantiate(&mut store, &module)
// Change to:
let _instance = linker.instantiate(&mut store, &module)
```

**d) `cortex/crates/cortex-agents/src/lib.rs:64-65`**
```rust
// Either use these fields or mark with underscore:
pub struct Agent {
    pub manifest: AgentManifest,
    store: Store<WasiP1Ctx>,       // unused
    instance: Instance,             // unused
}
// Change to:
pub struct Agent {
    pub manifest: AgentManifest,
    _store: Store<WasiP1Ctx>,
    _instance: Instance,
}
```

**Verification**:
```bash
cd cortex && cargo build --workspace
```

---

## 🟡 High-Priority Improvements (Do This Week)

### 4. Add Security Audit to CI
**Impact**: HIGH - Catch vulnerable dependencies  
**Effort**: 15 minutes  

**File**: `.cirrus.yml` (or `.github/workflows/ci.yml` if migrating to GH Actions)

**Add these lines**:
```yaml
  security_audit_script:
    - cargo install cargo-audit
    - cargo audit
```

**Or for GitHub Actions**:
```yaml
- name: Security Audit
  run: |
    cargo install cargo-audit
    cargo audit
```

---

### 5. Update Outdated Dependencies
**Impact**: MEDIUM - Security + features  
**Effort**: 30 minutes  

**File**: `rust/Cargo.toml` and workspace crates

**Commands**:
```bash
cd rust
cargo update --workspace        # Update within semver
cargo outdated                  # Check for major updates
```

**Key updates needed**:
```toml
# Current → Suggested
chrono = "0.4.5"    → "0.4.38"
regex = "1.3.7"     → "1.11.0"
notify = "5.0.0-pre.1" → "6.1.1" (major version available)
```

**Test after**:
```bash
cargo test --all
```

---

### 6. Add .gitignore Entries
**Impact**: LOW - Clean git status  
**Effort**: 2 minutes  

**File**: `.gitignore`

**Add**:
```
# Rust
target/
Cargo.lock  # Already ignored, but verify
**/*.rs.bk

# Cortex
cortex/target/

# IDE
.vscode/
.idea/
*.swp
*.swo
*~

# OS
.DS_Store
Thumbs.db
```

---

### 7. Document the Git Patch for Onig
**Impact**: LOW - Maintainability  
**Effort**: 5 minutes  

**File**: `rust/Cargo.toml`

**Current**:
```toml
# Avoid libllvm/libclang dep. See https://github.com/rust-onig/rust-onig/pull/108
[patch.crates-io]
onig = { git="https://github.com/kornelski/rust-onig", branch="bindgen" }
```

**Improve with**:
```toml
# Avoid libllvm/libclang dependency during build
# See: https://github.com/rust-onig/rust-onig/pull/108
# TODO: Remove this patch when rust-onig publishes a crates.io version without bindgen
# Tracking: https://github.com/rust-onig/rust-onig/issues/XXX
[patch.crates-io]
onig = { git="https://github.com/kornelski/rust-onig", branch="bindgen" }
onig_sys = { git="https://github.com/kornelski/rust-onig", branch="bindgen" }
```

---

## 🟢 Nice to Have (When You Have Time)

### 8. Add Minimal Tests for Cortex
**Impact**: MEDIUM - Prevent regressions  
**Effort**: 2-4 hours  

**Create**: `cortex/crates/cortex-rope/tests/basic_tests.rs`

```rust
use cortex_rope::Rope;

#[test]
fn test_rope_creation() {
    let rope = Rope::from("Hello, world!");
    assert_eq!(rope.len(), 13);
}

#[test]
fn test_rope_slice() {
    let rope = Rope::from("Hello, world!");
    let slice = rope.slice(0..5);
    assert_eq!(slice, "Hello");
}

// Add more tests...
```

**Run**:
```bash
cd cortex && cargo test
```

---

### 9. Consolidate CI Systems
**Impact**: MEDIUM - Reduce maintenance  
**Effort**: 1-2 hours  

**Action**: Choose one CI system

**Option A: Keep Cirrus CI**
- Delete `.travis.yml`
- Update README.md to remove Travis badge
- Add Cirrus badge

**Option B: Migrate to GitHub Actions**
- Create `.github/workflows/ci.yml`
- Delete `.travis.yml` and `.cirrus.yml`
- Modern, free, well-integrated

**GitHub Actions template**:
```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        rust: [stable, nightly]
    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v3
        with:
          submodules: recursive
      
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: ${{ matrix.rust }}
          components: rustfmt, clippy
          override: true
      
      - name: Build
        run: cd rust && cargo build --all
      
      - name: Test
        run: cd rust && cargo test --all
      
      - name: Clippy
        if: matrix.rust == 'stable'
        run: cd rust && cargo clippy --all -- -D warnings
      
      - name: Rustfmt
        if: matrix.rust == 'stable'
        run: cd rust && cargo fmt --all -- --check
```

---

### 10. Add Basic Property Tests
**Impact**: HIGH - Catch subtle bugs  
**Effort**: 4-8 hours  

**Add dependency**:
```toml
[dev-dependencies]
proptest = "1.0"
```

**Example test** (`rust/rope/tests/property_tests.rs`):
```rust
use proptest::prelude::*;
use xi_rope::{Rope, Delta};

proptest! {
    #[test]
    fn rope_insert_doesnt_panic(s in ".*", pos in 0usize..1000, insert in ".*") {
        let mut rope = Rope::from(s);
        let pos = pos % (rope.len() + 1);
        rope.edit(pos..pos, insert);
        // Should not panic
    }

    #[test]
    fn delta_composition_is_associative(
        s in ".*",
        d1 in arbitrary_delta(),
        d2 in arbitrary_delta(),
        d3 in arbitrary_delta()
    ) {
        // (d1 ∘ d2) ∘ d3 = d1 ∘ (d2 ∘ d3)
        let compose_left = (d1.compose(&d2)).compose(&d3);
        let compose_right = d1.compose(&(d2.compose(&d3)));
        assert_eq!(compose_left, compose_right);
    }
}
```

---

## Summary of Quick Wins

| Fix | Impact | Effort | Priority |
|-----|--------|--------|----------|
| 1. Fix clippy warning | HIGH | 5min | 🔴 Critical |
| 2. Remove unused key | MEDIUM | 1min | 🔴 Critical |
| 3. Fix Cortex warnings | MEDIUM | 10min | 🔴 Critical |
| 4. Add security audit | HIGH | 15min | 🟡 High |
| 5. Update dependencies | MEDIUM | 30min | 🟡 High |
| 6. Improve .gitignore | LOW | 2min | 🟡 High |
| 7. Document git patch | LOW | 5min | 🟡 High |
| 8. Add Cortex tests | MEDIUM | 2-4hr | 🟢 Nice |
| 9. Consolidate CI | MEDIUM | 1-2hr | 🟢 Nice |
| 10. Property tests | HIGH | 4-8hr | 🟢 Nice |

**Total time for critical fixes**: ~15 minutes  
**Total time for high-priority**: ~1 hour  
**Total time for nice-to-haves**: ~8-15 hours  

---

## Execution Order

### Day 1 (30 minutes)
1. ✅ Fix clippy warning (5min)
2. ✅ Remove unused Cargo key (1min)
3. ✅ Fix Cortex warnings (10min)
4. ✅ Test builds pass (5min)
5. ✅ Add security audit to CI (15min)

### Week 1 (2-3 hours)
6. ✅ Update dependencies (30min)
7. ✅ Test after updates (30min)
8. ✅ Improve .gitignore (2min)
9. ✅ Document git patch (5min)
10. ✅ Consolidate CI systems (1-2hr)

### Week 2-3 (Optional, 8-15 hours)
11. ✅ Add basic Cortex tests (2-4hr)
12. ✅ Add property tests for rope (4-8hr)
13. ✅ Add property tests for delta (2-4hr)

---

## Commands Cheat Sheet

```bash
# 1. Fix & verify Xi-Editor
cd rust
cargo clippy --all -- -D warnings  # Should pass after fix
cargo test --all                    # Should pass
cargo fmt --all -- --check          # Should pass

# 2. Fix & verify Cortex
cd ../cortex
cargo build --workspace             # Should build without warnings
cargo test --workspace              # Add tests first

# 3. Update dependencies
cd ../rust
cargo update --workspace
cargo test --all                    # Verify nothing broke

# 4. Security audit
cargo install cargo-audit
cargo audit                         # Check for vulnerabilities

# 5. Check outdated
cargo install cargo-outdated
cargo outdated                      # See what can be updated

# 6. Git cleanup
git status                          # Should be clean
git add .
git commit -m "Fix critical issues and warnings"
git push
```

---

**Document created**: February 11, 2026  
**Priority**: Execute critical fixes (15min) before any other work
