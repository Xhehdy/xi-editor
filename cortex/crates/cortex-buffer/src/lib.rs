//! Cortex Buffer - Text buffer management for the Cortex IDE
//!
//! Manages text buffers with undo/redo support, cursor positions, and change tracking.

use cortex_rope::Rope;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Buffer-related errors
#[derive(Debug, Error)]
pub enum BufferError {
    #[error("Invalid position: {0}")]
    InvalidPosition(usize),
    #[error("Invalid range: {start}..{end}")]
    InvalidRange { start: usize, end: usize },
}

/// A unique identifier for a buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BufferId(pub u64);

/// A text buffer with editing capabilities
#[derive(Debug, Clone)]
pub struct Buffer {
    id: BufferId,
    rope: Rope,
    /// Whether the buffer has unsaved changes
    dirty: bool,
    /// Undo stack (simplified for now)
    undo_stack: Vec<String>,
    /// Redo stack
    redo_stack: Vec<String>,
}

impl Buffer {
    /// Creates a new buffer with the given ID and content
    pub fn new(id: BufferId, content: impl Into<Rope>) -> Self {
        Self {
            id,
            rope: content.into(),
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Returns the buffer ID
    pub fn id(&self) -> BufferId {
        self.id
    }

    /// Returns the current content as a string
    pub fn content(&self) -> String {
        self.rope.to_string()
    }

    /// Returns the length in bytes
    pub fn len(&self) -> usize {
        self.rope.len()
    }

    /// Returns true if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.rope.is_empty()
    }

    /// Returns true if the buffer has unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Edits the buffer, replacing the given range with new text
    pub fn edit(&mut self, range: std::ops::Range<usize>, new_text: &str) -> Result<(), BufferError> {
        if range.start > range.end || range.end > self.len() {
            return Err(BufferError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        // Save state for undo
        self.undo_stack.push(self.content());
        self.redo_stack.clear();

        // Apply edit
        self.rope.edit(range, new_text);
        self.dirty = true;

        Ok(())
    }

    /// Undoes the last edit
    pub fn undo(&mut self) -> bool {
        if let Some(prev_content) = self.undo_stack.pop() {
            self.redo_stack.push(self.content());
            self.rope = Rope::from(prev_content);
            true
        } else {
            false
        }
    }

    /// Redoes the last undone edit
    pub fn redo(&mut self) -> bool {
        if let Some(next_content) = self.redo_stack.pop() {
            self.undo_stack.push(self.content());
            self.rope = Rope::from(next_content);
            true
        } else {
            false
        }
    }

    /// Marks the buffer as saved (not dirty)
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_and_undo() {
        let mut buffer = Buffer::new(BufferId(1), "Hello");
        buffer.edit(5..5, ", World!").unwrap();
        assert_eq!(buffer.content(), "Hello, World!");
        assert!(buffer.is_dirty());

        buffer.undo();
        assert_eq!(buffer.content(), "Hello");
    }
}
