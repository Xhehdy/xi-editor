//! Glyph Patch - Diff and patch computation for text buffers
//!
//! Computes minimal diffs between text versions and applies patches.

use glyph_rope::diff::{Diff, LineHashDiff};
use glyph_rope::{DeltaElement, Rope};
use serde::{Deserialize, Serialize};
use std::ops::Range;
use thiserror::Error;

/// Hybrid diff strategy thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffPolicy {
    /// Use contiguous replace for windows up to this size (bytes).
    pub simple_edit_max_bytes: usize,
    /// Route to line-hash diff when either document exceeds this size (bytes).
    pub large_doc_line_hash_threshold_bytes: usize,
    /// Consider minimal-window fallback only when combined changed window is <= this size (bytes).
    pub small_window_max_bytes: usize,
    /// Maximum DP matrix cells allowed in minimal-window fallback.
    pub small_window_max_dp_cells: usize,
}

impl DiffPolicy {
    pub const DEFAULT: Self = Self {
        simple_edit_max_bytes: 96,
        large_doc_line_hash_threshold_bytes: 128 * 1024,
        small_window_max_bytes: 8 * 1024,
        small_window_max_dp_cells: 500_000,
    };
}

impl Default for DiffPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Patch-related errors
#[derive(Debug, Error)]
pub enum PatchError {
    #[error("Patch failed to apply at offset {0}")]
    ApplyFailed(usize),
    #[error("Patched output is not valid UTF-8")]
    InvalidUtf8,
}

/// Represents a single operation in a patch
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchOp {
    /// Keep n bytes unchanged
    Retain(usize),
    /// Insert new text
    Insert(String),
    /// Delete n bytes
    Delete(usize),
}

/// A patch representing changes from one text version to another
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Patch {
    ops: Vec<PatchOp>,
}

impl Patch {
    /// Creates a new empty patch
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a retain operation
    pub fn retain(&mut self, n: usize) {
        if n > 0 {
            if let Some(PatchOp::Retain(last)) = self.ops.last_mut() {
                *last += n;
            } else {
                self.ops.push(PatchOp::Retain(n));
            }
        }
    }

    /// Adds an insert operation
    pub fn insert(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() {
            if let Some(PatchOp::Insert(last)) = self.ops.last_mut() {
                last.push_str(&text);
            } else {
                self.ops.push(PatchOp::Insert(text));
            }
        }
    }

    /// Adds a delete operation
    pub fn delete(&mut self, n: usize) {
        if n > 0 {
            if let Some(PatchOp::Delete(last)) = self.ops.last_mut() {
                *last += n;
            } else {
                self.ops.push(PatchOp::Delete(n));
            }
        }
    }

    /// Returns the operations in this patch
    pub fn ops(&self) -> &[PatchOp] {
        &self.ops
    }

    /// Applies this patch to a string
    pub fn apply(&self, text: &str) -> Result<String, PatchError> {
        let mut result = Vec::with_capacity(text.len());
        let mut cursor = 0usize;
        let bytes = text.as_bytes();

        for op in &self.ops {
            match op {
                PatchOp::Retain(n) => {
                    let end = cursor
                        .checked_add(*n)
                        .ok_or(PatchError::ApplyFailed(cursor))?;
                    if end > bytes.len() {
                        return Err(PatchError::ApplyFailed(cursor));
                    }
                    result.extend_from_slice(&bytes[cursor..end]);
                    cursor = end;
                }
                PatchOp::Insert(s) => {
                    result.extend_from_slice(s.as_bytes());
                }
                PatchOp::Delete(n) => {
                    let end = cursor
                        .checked_add(*n)
                        .ok_or(PatchError::ApplyFailed(cursor))?;
                    if end > bytes.len() {
                        return Err(PatchError::ApplyFailed(cursor));
                    }
                    cursor = end;
                }
            }
        }

        // Append remaining text
        if cursor < bytes.len() {
            result.extend_from_slice(&bytes[cursor..]);
        }

        String::from_utf8(result).map_err(|_| PatchError::InvalidUtf8)
    }
}

#[derive(Debug, Clone, Copy)]
struct DiffWindow {
    prefix_len: usize,
    suffix_len: usize,
    old_len: usize,
    new_len: usize,
}

impl DiffWindow {
    fn between(old: &str, new: &str) -> Self {
        let old_bytes = old.as_bytes();
        let new_bytes = new.as_bytes();

        let mut prefix_len = 0usize;
        let prefix_limit = old_bytes.len().min(new_bytes.len());
        while prefix_len < prefix_limit && old_bytes[prefix_len] == new_bytes[prefix_len] {
            prefix_len += 1;
        }
        while prefix_len > 0
            && (!old.is_char_boundary(prefix_len) || !new.is_char_boundary(prefix_len))
        {
            prefix_len -= 1;
        }

        let mut suffix_len = 0usize;
        while old_bytes.len() > prefix_len + suffix_len
            && new_bytes.len() > prefix_len + suffix_len
            && old_bytes[old_bytes.len() - 1 - suffix_len]
                == new_bytes[new_bytes.len() - 1 - suffix_len]
        {
            suffix_len += 1;
        }
        while suffix_len > 0 {
            let old_suffix_start = old_bytes.len() - suffix_len;
            let new_suffix_start = new_bytes.len() - suffix_len;
            if old_suffix_start >= prefix_len
                && new_suffix_start >= prefix_len
                && old.is_char_boundary(old_suffix_start)
                && new.is_char_boundary(new_suffix_start)
            {
                break;
            }
            suffix_len -= 1;
        }

        Self {
            prefix_len,
            suffix_len,
            old_len: old_bytes.len(),
            new_len: new_bytes.len(),
        }
    }

    fn old_mid_len(&self) -> usize {
        self.old_len - self.prefix_len - self.suffix_len
    }

    fn new_mid_len(&self) -> usize {
        self.new_len - self.prefix_len - self.suffix_len
    }

    fn old_mid_range(&self) -> Range<usize> {
        self.prefix_len..(self.old_len - self.suffix_len)
    }

    fn new_mid_range(&self) -> Range<usize> {
        self.prefix_len..(self.new_len - self.suffix_len)
    }

    fn old_mid<'a>(&self, old: &'a str) -> &'a str {
        &old[self.old_mid_range()]
    }

    fn new_mid<'a>(&self, new: &'a str) -> &'a str {
        &new[self.new_mid_range()]
    }
}

fn line_hash_diff(old: &str, new: &str) -> Patch {
    let old_rope = Rope::from(old);
    let new_rope = Rope::from(new);
    let delta = LineHashDiff::compute_delta(&old_rope, &new_rope);

    let mut patch = Patch::new();
    let mut cursor = 0usize;
    let mut conversion_failed = false;

    for element in delta.els {
        match element {
            DeltaElement::Copy(begin, end) => {
                if begin >= cursor {
                    if begin > cursor {
                        patch.delete(begin - cursor);
                    }
                    patch.retain(end - begin);
                    cursor = end;
                    continue;
                }

                // Non-monotonic copy ranges cannot be represented as backward retains.
                // Materialize the already-consumed portion as insertion from old text.
                if end <= cursor {
                    if let Some(text) = old.get(begin..end) {
                        patch.insert(text);
                    } else {
                        conversion_failed = true;
                        break;
                    }
                } else {
                    if let Some(text) = old.get(begin..cursor) {
                        patch.insert(text);
                    } else {
                        conversion_failed = true;
                        break;
                    }
                    patch.retain(end - cursor);
                    cursor = end;
                }
            }
            DeltaElement::Insert(node) => {
                patch.insert(String::from(node));
            }
        }
    }

    if !conversion_failed && cursor < delta.base_len {
        patch.delete(delta.base_len - cursor);
    }

    if conversion_failed || patch.apply(old).ok().as_deref() != Some(new) {
        return simple_window_patch(old, new, DiffWindow::between(old, new));
    }

    debug_assert_eq!(patch.apply(old).ok().as_deref(), Some(new));
    patch
}

fn simple_window_patch(old: &str, new: &str, window: DiffWindow) -> Patch {
    let mut patch = Patch::new();
    patch.retain(window.prefix_len);
    patch.delete(window.old_mid_len());
    patch.insert(window.new_mid(new));
    debug_assert_eq!(patch.apply(old).ok().as_deref(), Some(new));
    patch
}

#[derive(Debug, Clone, Copy)]
enum WindowOp {
    Retain(usize),
    Delete(usize),
    Insert(char),
}

fn minimal_window_patch(
    old: &str,
    new: &str,
    window: DiffWindow,
    policy: DiffPolicy,
) -> Option<Patch> {
    let old_mid = window.old_mid(old);
    let new_mid = window.new_mid(new);

    let old_chars: Vec<(char, usize)> = old_mid.chars().map(|ch| (ch, ch.len_utf8())).collect();
    let new_chars: Vec<(char, usize)> = new_mid.chars().map(|ch| (ch, ch.len_utf8())).collect();

    let cols = new_chars.len() + 1;
    let rows = old_chars.len() + 1;
    let cells = rows.checked_mul(cols)?;
    if cells > policy.small_window_max_dp_cells {
        return None;
    }

    let mut dp = vec![0usize; cells];
    let idx = |row: usize, col: usize| -> usize { row * cols + col };

    for row in 0..rows {
        dp[idx(row, 0)] = row;
    }
    for col in 0..cols {
        dp[idx(0, col)] = col;
    }

    for row in 1..rows {
        for col in 1..cols {
            if old_chars[row - 1].0 == new_chars[col - 1].0 {
                dp[idx(row, col)] = dp[idx(row - 1, col - 1)];
            } else {
                let delete_cost = dp[idx(row - 1, col)] + 1;
                let insert_cost = dp[idx(row, col - 1)] + 1;
                dp[idx(row, col)] = delete_cost.min(insert_cost);
            }
        }
    }

    let mut ops_rev = Vec::with_capacity(old_chars.len() + new_chars.len());
    let mut row = old_chars.len();
    let mut col = new_chars.len();

    while row > 0 || col > 0 {
        if row > 0
            && col > 0
            && old_chars[row - 1].0 == new_chars[col - 1].0
            && dp[idx(row, col)] == dp[idx(row - 1, col - 1)]
        {
            ops_rev.push(WindowOp::Retain(old_chars[row - 1].1));
            row -= 1;
            col -= 1;
            continue;
        }

        if row > 0 && dp[idx(row, col)] == dp[idx(row - 1, col)] + 1 {
            ops_rev.push(WindowOp::Delete(old_chars[row - 1].1));
            row -= 1;
            continue;
        }

        if col > 0 && dp[idx(row, col)] == dp[idx(row, col - 1)] + 1 {
            ops_rev.push(WindowOp::Insert(new_chars[col - 1].0));
            col -= 1;
            continue;
        }

        if row > 0 && col > 0 {
            ops_rev.push(WindowOp::Delete(old_chars[row - 1].1));
            ops_rev.push(WindowOp::Insert(new_chars[col - 1].0));
            row -= 1;
            col -= 1;
        } else if row > 0 {
            ops_rev.push(WindowOp::Delete(old_chars[row - 1].1));
            row -= 1;
        } else if col > 0 {
            ops_rev.push(WindowOp::Insert(new_chars[col - 1].0));
            col -= 1;
        }
    }

    let mut patch = Patch::new();
    patch.retain(window.prefix_len);

    let mut pending_insert = String::new();
    for op in ops_rev.into_iter().rev() {
        match op {
            WindowOp::Retain(bytes) => {
                if !pending_insert.is_empty() {
                    patch.insert(std::mem::take(&mut pending_insert));
                }
                patch.retain(bytes);
            }
            WindowOp::Delete(bytes) => {
                if !pending_insert.is_empty() {
                    patch.insert(std::mem::take(&mut pending_insert));
                }
                patch.delete(bytes);
            }
            WindowOp::Insert(ch) => pending_insert.push(ch),
        }
    }
    if !pending_insert.is_empty() {
        patch.insert(pending_insert);
    }

    debug_assert_eq!(patch.apply(old).ok().as_deref(), Some(new));
    Some(patch)
}

/// Computes a diff between two strings using a hybrid strategy:
/// - direct small-window edits: contiguous replace patch
/// - large documents: line-hash rope diff
/// - small changed windows: minimal character-level fallback
pub fn diff(old: &str, new: &str) -> Patch {
    diff_with_policy(old, new, DiffPolicy::DEFAULT)
}

/// Computes a diff using an explicit hybrid strategy policy.
pub fn diff_with_policy(old: &str, new: &str, policy: DiffPolicy) -> Patch {
    if old == new {
        return Patch::new();
    }

    let window = DiffWindow::between(old, new);
    if window.old_mid_len().max(window.new_mid_len()) <= policy.simple_edit_max_bytes {
        return simple_window_patch(old, new, window);
    }

    if old.len().max(new.len()) >= policy.large_doc_line_hash_threshold_bytes {
        return line_hash_diff(old, new);
    }

    if window.old_mid_len() + window.new_mid_len() <= policy.small_window_max_bytes {
        if let Some(patch) = minimal_window_patch(old, new, window, policy) {
            return patch;
        }
    }

    line_hash_diff(old, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_diff_roundtrip(old: &str, new: &str) {
        let patch = diff(old, new);
        let applied = patch.apply(old).unwrap();
        assert_eq!(applied, new);
    }

    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *seed
    }

    fn pick_char(seed: &mut u64) -> char {
        const CHARS: &[char] = &[
            'a', 'b', 'c', 'x', 'y', 'z', '\n', ' ', 'é', 'ê', 'β', '你', '👋', '🌍',
        ];
        let idx = (lcg(seed) as usize) % CHARS.len();
        CHARS[idx]
    }

    fn random_string(seed: &mut u64, max_chars: usize) -> String {
        let len = (lcg(seed) as usize) % (max_chars + 1);
        let mut out = String::new();
        for _ in 0..len {
            out.push(pick_char(seed));
        }
        out
    }

    fn random_boundary(s: &str, seed: &mut u64) -> usize {
        if s.is_empty() {
            return 0;
        }

        let mut points = Vec::new();
        points.push(0usize);
        for (idx, _) in s.char_indices() {
            points.push(idx);
        }
        points.push(s.len());
        points.sort_unstable();
        points.dedup();
        points[(lcg(seed) as usize) % points.len()]
    }

    fn apply_random_edit(base: &str, seed: &mut u64) -> String {
        let mut s = base.to_string();
        let edits = 1 + ((lcg(seed) as usize) % 2);
        for _ in 0..edits {
            let a = random_boundary(&s, seed);
            let b = random_boundary(&s, seed);
            let (start, end) = if a <= b { (a, b) } else { (b, a) };
            let inserted = random_string(seed, 10);
            s.replace_range(start..end, &inserted);
        }
        s
    }

    #[test]
    fn test_patch_apply() {
        // Transform "Hello World!" -> "Hello, Glyph!"
        // "Hello" (5) + " World" (6) + "!" (1) = 12 chars
        let mut patch = Patch::new();
        patch.retain(5); // Keep "Hello"
        patch.delete(6); // Delete " World"
        patch.insert(", Glyph"); // Insert ", Glyph"
        patch.retain(1); // Keep "!"

        let result = patch.apply("Hello World!").unwrap();
        assert_eq!(result, "Hello, Glyph!");
    }

    #[test]
    fn test_simple_insert() {
        let mut patch = Patch::new();
        patch.retain(5);
        patch.insert("!");

        let result = patch.apply("Hello").unwrap();
        assert_eq!(result, "Hello!");
    }

    #[test]
    fn test_diff_roundtrip_ascii() {
        assert_diff_roundtrip("fn main() {}\n", "fn main() {\n    println!(\"hi\");\n}\n");
    }

    #[test]
    fn test_diff_roundtrip_multiline() {
        assert_diff_roundtrip(
            "line one\nline two\nline three\n",
            "line one\nline 2\nline three\nline four\n",
        );
    }

    #[test]
    fn test_diff_roundtrip_unicode() {
        assert_diff_roundtrip("hi 👋\nmañana\n", "hi 👋🏽\nmañana!\n");
    }

    #[test]
    fn test_diff_noop_returns_empty_patch() {
        let patch = diff("unchanged", "unchanged");
        assert!(patch.ops().is_empty());
    }

    #[test]
    fn test_diff_with_policy_default_matches_diff() {
        let old = "prefix\nalpha beta\ngamma\n";
        let new = "prefix\nalpha BETA\ngamma tail\n";

        let via_default = diff(old, new);
        let via_policy = diff_with_policy(old, new, DiffPolicy::DEFAULT);
        assert_eq!(via_default.ops(), via_policy.ops());
    }

    #[test]
    fn test_diff_with_policy_roundtrips_when_minimal_window_is_disabled() {
        let policy = DiffPolicy {
            simple_edit_max_bytes: 8,
            large_doc_line_hash_threshold_bytes: usize::MAX,
            small_window_max_bytes: 4096,
            small_window_max_dp_cells: 1,
        };

        let old = format!("prefix:{}:middle:{}:suffix", "A".repeat(20), "B".repeat(20));
        let new = format!("prefix:{}:middle:{}:suffix", "C".repeat(20), "D".repeat(20));
        let patch = diff_with_policy(&old, &new, policy);
        assert_eq!(patch.apply(&old).unwrap(), new);
    }

    #[test]
    fn test_diff_uses_simple_patch_for_small_direct_edit() {
        let old = "let value = 1;";
        let new = "let value = 42;";
        let patch = diff(old, new);

        assert_eq!(
            patch.ops(),
            &[
                PatchOp::Retain(12),
                PatchOp::Delete(1),
                PatchOp::Insert("42".to_string()),
            ]
        );
        assert_eq!(patch.apply(old).unwrap(), new);
    }

    #[test]
    fn test_diff_uses_minimal_window_for_multi_region_small_change() {
        let old = format!("prefix:{}:middle:{}:suffix", "A".repeat(40), "B".repeat(40));
        let new = format!("prefix:{}:middle:{}:suffix", "C".repeat(40), "D".repeat(40));

        let policy = DiffPolicy {
            simple_edit_max_bytes: 8,
            large_doc_line_hash_threshold_bytes: usize::MAX,
            small_window_max_bytes: 4096,
            small_window_max_dp_cells: 500_000,
        };
        let patch = diff_with_policy(&old, &new, policy);
        let retain_count = patch
            .ops()
            .iter()
            .filter(|op| matches!(op, PatchOp::Retain(_)))
            .count();
        assert!(
            retain_count >= 2,
            "expected internal retain for middle segment"
        );
        assert_eq!(patch.apply(&old).unwrap(), new);
    }

    #[test]
    fn test_diff_preserves_utf8_boundaries_for_shared_prefix_byte() {
        // First byte is shared between these code points; boundary logic must remain valid.
        let old = "é";
        let new = "ê";
        let patch = diff(old, new);
        assert_eq!(patch.apply(old).unwrap(), new);
    }

    #[test]
    fn test_diff_large_document_roundtrip() {
        let old = "line\n".repeat(40_000);
        let mut new = old.clone();
        new.push_str("tail\n");

        assert_diff_roundtrip(&old, &new);
    }

    #[test]
    fn test_line_hash_conversion_handles_non_monotonic_copy_order() {
        fn build_fixture(lines: usize) -> (String, String) {
            let mut old = String::with_capacity(lines * 64);
            for i in 0..lines {
                old.push_str(&format!("fn item_{i}() {{ let value = {i}; }}\n"));
            }

            let mut new = old.clone();
            let midpoint = lines / 2;
            new = new.replacen(
                &format!("item_{midpoint}"),
                &format!("item_{midpoint}_x"),
                1,
            );
            if let Some(head_newline) = new.find('\n') {
                new.replace_range(..=head_newline, "");
            }
            new.push_str("fn appended_tail() { println!(\"tail\"); }\n");
            (old, new)
        }

        let (old, new) = build_fixture(1_000);
        let forced_line_hash = DiffPolicy {
            simple_edit_max_bytes: 0,
            large_doc_line_hash_threshold_bytes: usize::MAX,
            small_window_max_bytes: 0,
            small_window_max_dp_cells: 0,
        };

        let patch = diff_with_policy(&old, &new, forced_line_hash);
        assert_eq!(patch.apply(&old).unwrap(), new);
    }

    #[test]
    fn test_diff_roundtrip_randomized_unicode_edits() {
        let mut seed = 0xC0FFEE_u64;

        for _ in 0..700 {
            let old = random_string(&mut seed, 100);
            let new = apply_random_edit(&old, &mut seed);
            assert_diff_roundtrip(&old, &new);
        }

        for _ in 0..20 {
            let mut old = String::new();
            while old.len() < 180_000 {
                old.push_str("line-αβ👋\n");
            }
            let new = apply_random_edit(&old, &mut seed);
            assert_diff_roundtrip(&old, &new);
        }
    }

    #[test]
    fn test_patch_coalesces_adjacent_ops() {
        let mut patch = Patch::new();
        patch.retain(2);
        patch.retain(3);
        patch.delete(1);
        patch.delete(2);
        patch.insert("ab");
        patch.insert("cd");

        assert_eq!(
            patch.ops(),
            &[
                PatchOp::Retain(5),
                PatchOp::Delete(3),
                PatchOp::Insert("abcd".to_string()),
            ]
        );
    }

    #[test]
    fn test_patch_apply_invalid_utf8_boundary_returns_error() {
        let mut patch = Patch::new();
        patch.delete(1);

        let err = patch.apply("é").unwrap_err();
        assert!(matches!(err, PatchError::InvalidUtf8));
    }
}
