//! Glyph Buffer - Text buffer management for the Glyph IDE
//!
//! Manages text buffers with undo/redo support, cursor positions, and change tracking.

use glyph_rope::{LinesMetric, Rope};
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

/// A deterministic edit operation in byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditLog {
    start: usize,
    deleted: String,
    inserted: String,
}

impl EditLog {
    fn new(start: usize, deleted: String, inserted: String) -> Self {
        Self {
            start,
            deleted,
            inserted,
        }
    }

    /// Returns the byte start offset of this edit.
    pub fn start(&self) -> usize {
        self.start
    }

    /// Returns deleted text for this edit.
    pub fn deleted(&self) -> &str {
        &self.deleted
    }

    /// Returns inserted text for this edit.
    pub fn inserted(&self) -> &str {
        &self.inserted
    }

    /// Returns deleted byte length.
    pub fn deleted_len(&self) -> usize {
        self.deleted.len()
    }

    /// Returns true if this edit does not change content.
    pub fn is_noop(&self) -> bool {
        self.deleted == self.inserted
    }

    /// Returns the inverse operation for undo/redo stacks.
    pub fn inverse(&self) -> Self {
        Self {
            start: self.start,
            deleted: self.inserted.clone(),
            inserted: self.deleted.clone(),
        }
    }
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
    /// Undo stack contains inverse edits to apply.
    undo_stack: Vec<EditLog>,
    /// Redo stack
    redo_stack: Vec<EditLog>,
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

    /// Returns the number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.rope.measure::<LinesMetric>() + 1
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

    /// Returns the closest previous valid UTF-8 boundary at or before `position`.
    pub fn nearest_char_boundary(&self, position: usize) -> usize {
        if position >= self.len() {
            return self.len();
        }
        if self.rope.is_codepoint_boundary(position) {
            return position;
        }
        self.rope
            .at_or_prev_codepoint_boundary(position)
            .unwrap_or(0)
    }

    /// Returns the previous UTF-8 boundary before `position`.
    pub fn prev_char_boundary(&self, position: usize) -> usize {
        let position = self.nearest_char_boundary(position);
        self.rope.prev_codepoint_offset(position).unwrap_or(0)
    }

    /// Returns the next UTF-8 boundary after `position`.
    pub fn next_char_boundary(&self, position: usize) -> usize {
        let position = self.nearest_char_boundary(position);
        self.rope
            .next_codepoint_offset(position)
            .unwrap_or_else(|| self.len())
    }

    /// Returns the line number for a byte offset.
    pub fn line_of_offset(&self, position: usize) -> usize {
        let position = self.nearest_char_boundary(position.min(self.len()));
        self.rope.line_of_offset(position)
    }

    /// Returns the start byte offset of the line containing `position`.
    pub fn line_start(&self, position: usize) -> usize {
        let line = self.line_of_offset(position);
        self.rope.offset_of_line(line)
    }

    /// Returns the end byte offset (excluding trailing newline) for a line.
    pub fn line_end_for_line(&self, line: usize) -> usize {
        let last_line = self.line_count().saturating_sub(1);
        let line = line.min(last_line);
        let next_start = self.rope.offset_of_line(line + 1);
        if next_start > 0 && self.rope.byte_at(next_start - 1) == b'\n' {
            next_start - 1
        } else {
            next_start
        }
    }

    /// Returns the end byte offset (excluding trailing newline) of the current line.
    pub fn line_end(&self, position: usize) -> usize {
        let line = self.line_of_offset(position);
        self.line_end_for_line(line)
    }

    /// Returns visual column (in Unicode scalar values) from line start to `position`.
    pub fn column_of_offset(&self, position: usize) -> usize {
        let position = self.nearest_char_boundary(position.min(self.len()));
        let line_start = self.line_start(position);
        self.rope
            .iter_chunks(line_start..position)
            .map(|chunk| chunk.chars().count())
            .sum()
    }

    /// Returns byte offset for `column` within `line`, clamped to line end.
    pub fn offset_for_line_column(&self, line: usize, column: usize) -> usize {
        let last_line = self.line_count().saturating_sub(1);
        let line = line.min(last_line);
        let line_start = self.rope.offset_of_line(line);
        let line_end = self.line_end_for_line(line);

        let mut remaining = column;
        let mut offset = line_start;
        for chunk in self.rope.iter_chunks(line_start..line_end) {
            let chars_in_chunk = chunk.chars().count();
            if remaining >= chars_in_chunk {
                remaining -= chars_in_chunk;
                offset += chunk.len();
                continue;
            }

            let byte_in_chunk = chunk
                .char_indices()
                .nth(remaining)
                .map(|(idx, _)| idx)
                .unwrap_or(chunk.len());
            return offset + byte_in_chunk;
        }

        line_end
    }

    fn validate_range(&self, range: &std::ops::Range<usize>) -> Result<(), BufferError> {
        if range.start > range.end || range.end > self.len() {
            return Err(BufferError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        if !self.rope.is_codepoint_boundary(range.start)
            || !self.rope.is_codepoint_boundary(range.end)
        {
            return Err(BufferError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        Ok(())
    }

    fn apply_edit_log(&mut self, edit: &EditLog) -> Result<(), BufferError> {
        if edit.is_noop() {
            return Ok(());
        }

        let end = edit
            .start
            .checked_add(edit.deleted.len())
            .ok_or(BufferError::InvalidRange {
                start: edit.start,
                end: usize::MAX,
            })?;
        let range = edit.start..end;
        self.validate_range(&range)?;
        self.rope.edit(range, &edit.inserted);
        self.dirty = true;
        Ok(())
    }

    /// Edits the buffer, replacing the given range with new text.
    /// Returns an edit log that can be converted to a patch without diffing.
    pub fn edit(
        &mut self,
        range: std::ops::Range<usize>,
        new_text: &str,
    ) -> Result<EditLog, BufferError> {
        self.validate_range(&range)?;

        let deleted = String::from(self.rope.slice(range.clone()));
        let edit = EditLog::new(range.start, deleted, new_text.to_string());

        if edit.is_noop() {
            return Ok(edit);
        }

        self.apply_edit_log(&edit)?;
        self.undo_stack.push(edit.inverse());
        self.redo_stack.clear();
        Ok(edit)
    }

    /// Undoes the last edit and returns the applied edit log if available.
    pub fn undo(&mut self) -> Result<Option<EditLog>, BufferError> {
        let Some(edit) = self.undo_stack.pop() else {
            return Ok(None);
        };
        self.apply_edit_log(&edit)?;
        self.redo_stack.push(edit.inverse());
        Ok(Some(edit))
    }

    /// Redoes the last undone edit and returns the applied edit log if available.
    pub fn redo(&mut self) -> Result<Option<EditLog>, BufferError> {
        let Some(edit) = self.redo_stack.pop() else {
            return Ok(None);
        };
        self.apply_edit_log(&edit)?;
        self.undo_stack.push(edit.inverse());
        Ok(Some(edit))
    }

    /// Marks the buffer as saved (not dirty)
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Replaces the entire buffer content with disk-authoritative text.
    ///
    /// This clears undo/redo history because the new content may not be
    /// derivable from the prior edit stream.
    pub fn replace_all(&mut self, new_content: &str) {
        self.rope = Rope::from(new_content);
        self.dirty = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_and_undo() {
        let mut buffer = Buffer::new(BufferId(1), "Hello");
        let edit = buffer.edit(5..5, ", World!").unwrap();
        assert_eq!(edit.start(), 5);
        assert_eq!(edit.deleted(), "");
        assert_eq!(edit.inserted(), ", World!");
        assert_eq!(buffer.content(), "Hello, World!");
        assert!(buffer.is_dirty());

        let undo = buffer.undo().unwrap().unwrap();
        assert_eq!(undo.start(), 5);
        assert_eq!(undo.deleted(), ", World!");
        assert_eq!(undo.inserted(), "");
        assert_eq!(buffer.content(), "Hello");

        let redo = buffer.redo().unwrap().unwrap();
        assert_eq!(redo.start(), 5);
        assert_eq!(redo.deleted(), "");
        assert_eq!(redo.inserted(), ", World!");
        assert_eq!(buffer.content(), "Hello, World!");
    }

    #[test]
    fn test_edit_rejects_non_boundary_ranges() {
        let mut buffer = Buffer::new(BufferId(2), "a👋");
        let err = buffer.edit(2..2, "x").unwrap_err();
        assert!(matches!(err, BufferError::InvalidRange { .. }));
    }

    #[test]
    fn test_rope_native_boundaries_and_line_metrics() {
        let buffer = Buffer::new(BufferId(3), "a👋\nβeta\n");

        assert_eq!(buffer.nearest_char_boundary(2), 1);
        assert_eq!(buffer.prev_char_boundary(5), 1);
        assert_eq!(buffer.next_char_boundary(1), 5);

        assert_eq!(buffer.line_count(), 3);
        assert_eq!(buffer.line_start(7), 6);
        assert_eq!(buffer.line_end(7), 11);
        assert_eq!(buffer.column_of_offset(8), 1);
        assert_eq!(buffer.offset_for_line_column(1, 2), 9);
    }

    #[test]
    fn test_undo_redo_roundtrip_with_unicode() {
        let mut buffer = Buffer::new(BufferId(4), "hi 👋\n");
        let _ = buffer.edit(3..7, "🌍").unwrap();
        assert_eq!(buffer.content(), "hi 🌍\n");

        let _ = buffer.undo().unwrap();
        assert_eq!(buffer.content(), "hi 👋\n");

        let _ = buffer.redo().unwrap();
        assert_eq!(buffer.content(), "hi 🌍\n");
    }

    #[test]
    fn test_replace_all_resets_dirty_and_history() {
        let mut buffer = Buffer::new(BufferId(5), "draft");
        let _ = buffer.edit(5..5, " change").unwrap();
        assert!(buffer.is_dirty());

        buffer.replace_all("disk");

        assert_eq!(buffer.content(), "disk");
        assert!(!buffer.is_dirty());
        assert!(buffer.undo().unwrap().is_none());
        assert!(buffer.redo().unwrap().is_none());
    }
}
