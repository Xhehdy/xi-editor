//! Cortex Protocol - IPC protocol for Core <-> UI communication
//!
//! Uses MessagePack for high-performance serialization over Unix sockets.

use cortex_events::{CoreEvent, InputEvent, ViewId};
use cortex_patch::Patch;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Protocol errors
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("IO error: {0}")]
    Io(String),
}

/// Messages sent from UI to Core
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiToCore {
    /// Initialize connection
    Hello { client_version: String },
    /// Create a new view/buffer
    NewView { path: Option<String> },
    /// Close a view
    CloseView { view_id: ViewId },
    /// Input event from user
    Input { view_id: ViewId, event: InputEvent },
    /// Request current buffer content
    GetContent { view_id: ViewId },
    /// Send a chat message
    Chat { message: String, context_files: Vec<String> },
    /// Request inline completion (ghost text)
    RequestCompletion { view_id: ViewId, position: usize },
    /// Index a file for comprehension
    IndexFile { path: String },
    /// Query symbols in file
    QuerySymbols { path: String },
    /// Search symbols by name
    SearchSymbols { query: String },
}

/// A highlighted region in the text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub highlight: String,
}

/// Messages sent from Core to UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreToUi {
    /// Acknowledge hello
    Welcome { core_version: String },
    /// A new view was created
    ViewCreated { view_id: ViewId, content: String },
    /// Apply a patch to the view
    ApplyPatch { view_id: ViewId, patch: Patch },
    /// Apply syntax highlighting
    ApplyHighlights { view_id: ViewId, spans: Vec<HighlightSpan> },
    /// Full content update (for debugging, avoid in production)
    SetContent { view_id: ViewId, content: String, spans: Vec<HighlightSpan> },
    /// Streaming chat response
    ChatToken { token: String, done: bool },
    /// Ghost text completion
    GhostText { view_id: ViewId, text: String },
    /// File indexed successfully
    FileIndexed { path: String, symbol_count: usize },
    /// Symbol query results
    Symbols { symbols: Vec<SymbolInfo> },
    /// Core event notification
    Event(CoreEvent),
    /// Error response
    Error { message: String },
}

/// Symbol information for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub signature: Option<String>,
}

/// Serializes a message to JSON bytes (default for Swift compatibility)
pub fn serialize<T: Serialize>(msg: &T) -> Result<Vec<u8>, ProtocolError> {
    serde_json::to_vec(msg).map_err(|e| ProtocolError::Serialization(e.to_string()))
}

/// Deserializes a message from JSON bytes
pub fn deserialize<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, ProtocolError> {
    serde_json::from_slice(bytes).map_err(|e| ProtocolError::Deserialization(e.to_string()))
}

/// Serializes a message to MessagePack bytes (for future optimization)
#[allow(dead_code)]
pub fn serialize_msgpack<T: Serialize>(msg: &T) -> Result<Vec<u8>, ProtocolError> {
    rmp_serde::to_vec(msg).map_err(|e| ProtocolError::Serialization(e.to_string()))
}

/// Deserializes a message from MessagePack bytes
#[allow(dead_code)]
pub fn deserialize_msgpack<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, ProtocolError> {
    rmp_serde::from_slice(bytes).map_err(|e| ProtocolError::Deserialization(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_json() {
        let msg = UiToCore::Hello {
            client_version: "0.1.0".to_string(),
        };
        let bytes = serialize(&msg).unwrap();
        
        // Verify it's valid JSON
        let json_str = std::str::from_utf8(&bytes).unwrap();
        assert!(json_str.contains("Hello"));
        
        let decoded: UiToCore = deserialize(&bytes).unwrap();
        
        if let UiToCore::Hello { client_version } = decoded {
            assert_eq!(client_version, "0.1.0");
        } else {
            panic!("Wrong message type");
        }
    }
}
