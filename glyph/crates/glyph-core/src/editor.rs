//! Editor module - deterministic editor operations for core input handling.
//!
//! Keeps cursor movement and mutation semantics in one place so the core
//! message handler stays focused on protocol orchestration.

use glyph_buffer::{Buffer, BufferError, EditLog};
use glyph_events::Movement;
use glyph_patch::Patch;
use std::ops::Range;

/// Stateless editor helpers used by `glyph-core`.
pub struct EditorCore;

impl EditorCore {
    /// Converts a buffer edit log into a byte-offset patch.
    pub fn patch_from_edit(edit: &EditLog) -> Patch {
        if edit.is_noop() {
            return Patch::new();
        }

        let mut patch = Patch::new();
        patch.retain(edit.start());
        patch.delete(edit.deleted_len());
        patch.insert(edit.inserted());
        patch
    }

    /// Applies an insert at cursor and returns the resulting patch.
    pub fn apply_insert(
        buffer: &mut Buffer,
        cursor: &mut usize,
        text: &str,
    ) -> Result<Patch, BufferError> {
        let edit = buffer.edit(*cursor..*cursor, text)?;
        *cursor = buffer.nearest_char_boundary((*cursor).saturating_add(text.len()));
        Ok(Self::patch_from_edit(&edit))
    }

    /// Applies backspace at cursor and returns the resulting patch.
    pub fn apply_backspace(buffer: &mut Buffer, cursor: &mut usize) -> Result<Patch, BufferError> {
        if *cursor == 0 {
            return Ok(Patch::new());
        }

        let start = buffer.prev_char_boundary(*cursor);
        let edit = buffer.edit(start..*cursor, "")?;
        *cursor = start;
        Ok(Self::patch_from_edit(&edit))
    }

    /// Applies delete at cursor and returns the resulting patch.
    pub fn apply_delete(buffer: &mut Buffer, cursor: &mut usize) -> Result<Patch, BufferError> {
        if *cursor >= buffer.len() {
            return Ok(Patch::new());
        }

        let end = buffer.next_char_boundary(*cursor);
        let edit = buffer.edit(*cursor..end, "")?;
        Ok(Self::patch_from_edit(&edit))
    }

    /// Applies undo and returns patch for the resulting change.
    pub fn apply_undo(buffer: &mut Buffer, cursor: &mut usize) -> Result<Patch, BufferError> {
        let patch = match buffer.undo()? {
            Some(edit) => Self::patch_from_edit(&edit),
            None => Patch::new(),
        };
        *cursor = buffer.nearest_char_boundary((*cursor).min(buffer.len()));
        Ok(patch)
    }

    /// Applies redo and returns patch for the resulting change.
    pub fn apply_redo(buffer: &mut Buffer, cursor: &mut usize) -> Result<Patch, BufferError> {
        let patch = match buffer.redo()? {
            Some(edit) => Self::patch_from_edit(&edit),
            None => Patch::new(),
        };
        *cursor = buffer.nearest_char_boundary((*cursor).min(buffer.len()));
        Ok(patch)
    }

    /// Applies an arbitrary range replacement and updates cursor to insertion end.
    pub fn apply_range_edit(
        buffer: &mut Buffer,
        cursor: &mut usize,
        range: Range<usize>,
        inserted_text: &str,
    ) -> Result<Patch, BufferError> {
        let start = range.start;
        let edit = buffer.edit(range, inserted_text)?;
        *cursor = buffer.nearest_char_boundary(start.saturating_add(inserted_text.len()));
        Ok(Self::patch_from_edit(&edit))
    }

    /// Resolves the next cursor position for a movement request.
    pub fn move_cursor(buffer: &Buffer, cursor: usize, movement: Movement) -> usize {
        let len = buffer.len();
        let current = buffer.nearest_char_boundary(cursor.min(len));

        match movement {
            Movement::Left | Movement::WordLeft => buffer.prev_char_boundary(current),
            Movement::Right | Movement::WordRight => buffer.next_char_boundary(current),
            Movement::DocumentStart => 0,
            Movement::DocumentEnd => len,
            Movement::LineStart => buffer.line_start(current),
            Movement::LineEnd => buffer.line_end(current),
            Movement::Up => {
                let current_line = buffer.line_of_offset(current);
                if current_line == 0 {
                    return 0;
                }
                let column = buffer.column_of_offset(current);
                buffer.offset_for_line_column(current_line - 1, column)
            }
            Movement::Down => {
                let current_line = buffer.line_of_offset(current);
                let last_line = buffer.line_count().saturating_sub(1);
                if current_line >= last_line {
                    return len;
                }
                let column = buffer.column_of_offset(current);
                buffer.offset_for_line_column(current_line + 1, column)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EditorCore;
    use glyph_buffer::{Buffer, BufferId};
    use glyph_events::Movement;

    #[test]
    fn test_insert_and_backspace_emit_roundtrippable_patches() {
        let mut buffer = Buffer::new(BufferId(1), "Hello");
        let mut cursor = buffer.len();

        let old = buffer.content();
        let insert_patch = EditorCore::apply_insert(&mut buffer, &mut cursor, ", Glyph").unwrap();
        assert_eq!(insert_patch.apply(&old).unwrap(), buffer.content());
        assert_eq!(buffer.content(), "Hello, Glyph");

        let old = buffer.content();
        let backspace_patch = EditorCore::apply_backspace(&mut buffer, &mut cursor).unwrap();
        assert_eq!(backspace_patch.apply(&old).unwrap(), buffer.content());
        assert_eq!(buffer.content(), "Hello, Glyp");
    }

    #[test]
    fn test_delete_handles_multibyte_codepoint() {
        let mut buffer = Buffer::new(BufferId(2), "A🙂B");
        let mut cursor = 1;
        let old = buffer.content();

        let patch = EditorCore::apply_delete(&mut buffer, &mut cursor).unwrap();
        assert_eq!(patch.apply(&old).unwrap(), buffer.content());
        assert_eq!(buffer.content(), "AB");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn test_undo_redo_emit_roundtrippable_patches() {
        let mut buffer = Buffer::new(BufferId(3), "abc");
        let mut cursor = buffer.len();

        let _ = EditorCore::apply_insert(&mut buffer, &mut cursor, "d").unwrap();
        let old = buffer.content();
        let undo_patch = EditorCore::apply_undo(&mut buffer, &mut cursor).unwrap();
        assert_eq!(undo_patch.apply(&old).unwrap(), buffer.content());
        assert_eq!(buffer.content(), "abc");

        let old = buffer.content();
        let redo_patch = EditorCore::apply_redo(&mut buffer, &mut cursor).unwrap();
        assert_eq!(redo_patch.apply(&old).unwrap(), buffer.content());
        assert_eq!(buffer.content(), "abcd");
    }

    #[test]
    fn test_apply_range_edit_updates_cursor_to_insert_end() {
        let mut buffer = Buffer::new(BufferId(4), "abcdef");
        let mut cursor = 0;
        let old = buffer.content();

        let patch = EditorCore::apply_range_edit(&mut buffer, &mut cursor, 2..4, "ZZ").unwrap();
        assert_eq!(patch.apply(&old).unwrap(), buffer.content());
        assert_eq!(buffer.content(), "abZZef");
        assert_eq!(cursor, 4);
    }

    #[test]
    fn test_move_cursor_line_navigation() {
        let buffer = Buffer::new(BufferId(5), "abc\nx\n");

        assert_eq!(EditorCore::move_cursor(&buffer, 2, Movement::LineStart), 0);
        assert_eq!(EditorCore::move_cursor(&buffer, 2, Movement::LineEnd), 3);
        assert_eq!(EditorCore::move_cursor(&buffer, 0, Movement::Down), 4);
        assert_eq!(EditorCore::move_cursor(&buffer, 4, Movement::Up), 0);
        assert_eq!(
            EditorCore::move_cursor(&buffer, 2, Movement::DocumentStart),
            0
        );
        assert_eq!(
            EditorCore::move_cursor(&buffer, 2, Movement::DocumentEnd),
            buffer.len()
        );
    }
}
