//! Cortex Patch - Diff and patch computation for text buffers
//!
//! Computes minimal diffs between text versions and applies patches.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Patch-related errors
#[derive(Debug, Error)]
pub enum PatchError {
    #[error("Patch failed to apply at offset {0}")]
    ApplyFailed(usize),
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
            self.ops.push(PatchOp::Retain(n));
        }
    }

    /// Adds an insert operation
    pub fn insert(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() {
            self.ops.push(PatchOp::Insert(text));
        }
    }

    /// Adds a delete operation
    pub fn delete(&mut self, n: usize) {
        if n > 0 {
            self.ops.push(PatchOp::Delete(n));
        }
    }

    /// Returns the operations in this patch
    pub fn ops(&self) -> &[PatchOp] {
        &self.ops
    }

    /// Applies this patch to a string
    pub fn apply(&self, text: &str) -> Result<String, PatchError> {
        let mut result = String::new();
        let mut cursor = 0;
        let bytes = text.as_bytes();

        for op in &self.ops {
            match op {
                PatchOp::Retain(n) => {
                    if cursor + n > bytes.len() {
                        return Err(PatchError::ApplyFailed(cursor));
                    }
                    result.push_str(&text[cursor..cursor + n]);
                    cursor += n;
                }
                PatchOp::Insert(s) => {
                    result.push_str(s);
                }
                PatchOp::Delete(n) => {
                    if cursor + n > bytes.len() {
                        return Err(PatchError::ApplyFailed(cursor));
                    }
                    cursor += n;
                }
            }
        }

        // Append remaining text
        if cursor < bytes.len() {
            result.push_str(&text[cursor..]);
        }

        Ok(result)
    }
}

/// Computes a simple diff between two strings (placeholder implementation)
pub fn diff(old: &str, new: &str) -> Patch {
    // Simplified: just delete all and insert all
    // TODO: Implement proper diff algorithm (Myers, patience, etc.)
    let mut patch = Patch::new();
    patch.delete(old.len());
    patch.insert(new);
    patch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_apply() {
        // Transform "Hello World!" -> "Hello, Cortex!"
        // "Hello" (5) + " World" (6) + "!" (1) = 12 chars
        let mut patch = Patch::new();
        patch.retain(5);       // Keep "Hello"
        patch.delete(6);       // Delete " World"
        patch.insert(", Cortex");  // Insert ", Cortex"
        patch.retain(1);       // Keep "!"

        let result = patch.apply("Hello World!").unwrap();
        assert_eq!(result, "Hello, Cortex!");
    }

    #[test]
    fn test_simple_insert() {
        let mut patch = Patch::new();
        patch.retain(5);
        patch.insert("!");

        let result = patch.apply("Hello").unwrap();
        assert_eq!(result, "Hello!");
    }
}
