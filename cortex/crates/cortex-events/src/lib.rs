//! Cortex Events - Intent and editor event types
//!
//! Defines the event types that flow through the Cortex IDE system.

use serde::{Deserialize, Serialize};

/// Unique identifier for a view/editor tab
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewId(pub u64);

/// User input events from the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    /// Text was inserted at cursor
    Insert { text: String },
    /// Backspace key pressed
    Backspace,
    /// Delete key pressed
    Delete,
    /// Arrow key navigation
    Move(Movement),
    /// Selection changed
    Select { start: usize, end: usize },
    /// Undo requested
    Undo,
    /// Redo requested
    Redo,
    /// Save requested
    Save,
    /// Find text
    Find { query: String },
}

/// Cursor movement directions
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    WordLeft,
    WordRight,
}

/// Intent types for AI-related operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Intent {
    /// Generate code based on context
    Generate { prompt: String },
    /// Refactor selected code
    Refactor { selection: std::ops::Range<usize>, instruction: String },
    /// Explain selected code
    Explain { selection: std::ops::Range<usize> },
    /// Debug/fix an error
    Debug { error: String },
    /// Plan a larger change
    Plan { description: String },
}

/// Confidence level for AI suggestions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// An AI-generated suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: u64,
    pub intent: Intent,
    pub confidence: Confidence,
    pub preview: String,
    /// The actual change to apply (as a patch)
    pub change: Option<String>,
}

/// Core events that the system produces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreEvent {
    /// Buffer content changed
    BufferChanged { view_id: ViewId },
    /// Cursor position changed
    CursorMoved { view_id: ViewId, position: usize },
    /// AI suggestion available
    SuggestionReady(Suggestion),
    /// Error occurred
    Error { message: String },
}
