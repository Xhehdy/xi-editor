//! Glyph Protocol - IPC protocol for Core <-> UI communication
//!
//! Uses MessagePack for high-performance serialization over Unix sockets.

use glyph_events::{CoreEvent, InputEvent, ViewId};
use glyph_patch::Patch;
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

/// Client feature flags sent during connection handshake.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// When true, core may stream `ApplyPatch` responses for mutating edits.
    #[serde(default)]
    pub patch_streaming: bool,
}

/// Messages sent from UI to Core
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiToCore {
    /// Initialize connection
    Hello {
        client_version: String,
        #[serde(default)]
        capabilities: Option<ClientCapabilities>,
    },
    /// Create a new view/buffer
    NewView { path: Option<String> },
    /// Close a view
    CloseView { view_id: ViewId },
    /// Input event from user
    Input { view_id: ViewId, event: InputEvent },
    /// Atomic contiguous edit in UTF-8 byte offsets.
    ApplyEdit {
        view_id: ViewId,
        start: usize,
        deleted_len: usize,
        inserted_text: String,
        /// Expected buffer revision before this edit is applied.
        #[serde(default)]
        base_revision: u64,
    },
    /// Request current buffer content
    GetContent { view_id: ViewId },
    /// Send a chat message
    Chat {
        message: String,
        context_files: Vec<String>,
    },
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

/// Stable protocol-level error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    #[default]
    Unknown,
    InvalidRequest,
    ViewNotFound,
    BufferNotFound,
    StaleRevision,
    InvalidRange,
    InvalidUtf8Boundary,
    DeserializeFailed,
    FrameTooLarge,
    FileReadFailed,
    LlmFailure,
    CompletionTimeout,
    IndexingFailed,
}

/// Messages sent from Core to UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreToUi {
    /// Acknowledge hello
    Welcome { core_version: String },
    /// A new view was created
    ViewCreated {
        view_id: ViewId,
        content: String,
        revision: u64,
    },
    /// Apply a patch to the view
    ApplyPatch {
        view_id: ViewId,
        patch: Patch,
        revision: u64,
    },
    /// Apply syntax highlighting
    ApplyHighlights {
        view_id: ViewId,
        spans: Vec<HighlightSpan>,
    },
    /// Full content update (for debugging, avoid in production)
    SetContent {
        view_id: ViewId,
        content: String,
        spans: Vec<HighlightSpan>,
        revision: u64,
    },
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
    Error {
        message: String,
        #[serde(default)]
        code: ErrorCode,
        #[serde(default)]
        retryable: bool,
        #[serde(default)]
        should_resync: bool,
    },
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

/// Serializes a message to MessagePack bytes.
pub fn serialize<T: Serialize>(msg: &T) -> Result<Vec<u8>, ProtocolError> {
    rmp_serde::to_vec(msg).map_err(|e| ProtocolError::Serialization(e.to_string()))
}

/// Deserializes a message from MessagePack bytes.
pub fn deserialize<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, ProtocolError> {
    rmp_serde::from_slice(bytes).map_err(|e| ProtocolError::Deserialization(e.to_string()))
}

/// Serializes a message to JSON bytes for compatibility/debugging.
pub fn serialize_json<T: Serialize>(msg: &T) -> Result<Vec<u8>, ProtocolError> {
    serde_json::to_vec(msg).map_err(|e| ProtocolError::Serialization(e.to_string()))
}

/// Deserializes a message from JSON bytes.
pub fn deserialize_json<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, ProtocolError> {
    serde_json::from_slice(bytes).map_err(|e| ProtocolError::Deserialization(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_msgpack_and_json_helpers() {
        let msg = UiToCore::Hello {
            client_version: "0.1.0".to_string(),
            capabilities: Some(ClientCapabilities {
                patch_streaming: true,
            }),
        };

        // MessagePack is the default wire format.
        let bytes = serialize(&msg).unwrap();
        let decoded: UiToCore = deserialize(&bytes).unwrap();
        assert!(matches!(
            decoded,
            UiToCore::Hello {
                client_version,
                capabilities: Some(ClientCapabilities { patch_streaming: true })
            } if client_version == "0.1.0"
        ));

        // JSON helpers remain available for compatibility.
        let json_bytes = serialize_json(&msg).unwrap();
        let json_decoded: UiToCore = deserialize_json(&json_bytes).unwrap();
        assert!(matches!(
            json_decoded,
            UiToCore::Hello {
                client_version,
                capabilities: Some(ClientCapabilities { patch_streaming: true })
            } if client_version == "0.1.0"
        ));
    }

    #[test]
    fn test_legacy_hello_defaults_capabilities_to_none() {
        let json = br#"{"Hello":{"client_version":"0.1.0"}}"#;
        let decoded: UiToCore = deserialize_json(json).unwrap();

        assert!(matches!(
            &decoded,
            UiToCore::Hello {
                client_version,
                capabilities: None
            } if client_version == "0.1.0"
        ));

        let patch_streaming = match decoded {
            UiToCore::Hello { capabilities, .. } => capabilities
                .map(|caps| caps.patch_streaming)
                .unwrap_or(false),
            _ => unreachable!("test decodes hello variant"),
        };
        assert!(!patch_streaming);
    }

    #[test]
    fn test_roundtrip_apply_edit_helpers() {
        let msg = UiToCore::ApplyEdit {
            view_id: ViewId(42),
            start: 3,
            deleted_len: 5,
            inserted_text: "hello".to_string(),
            base_revision: 7,
        };

        let bytes = serialize(&msg).unwrap();
        let decoded: UiToCore = deserialize(&bytes).unwrap();
        assert!(matches!(
            decoded,
            UiToCore::ApplyEdit {
                view_id: ViewId(42),
                start: 3,
                deleted_len: 5,
                inserted_text,
                base_revision: 7,
            } if inserted_text == "hello"
        ));

        let json_bytes = serialize_json(&msg).unwrap();
        let json_decoded: UiToCore = deserialize_json(&json_bytes).unwrap();
        assert!(matches!(
            json_decoded,
            UiToCore::ApplyEdit {
                view_id: ViewId(42),
                start: 3,
                deleted_len: 5,
                inserted_text,
                base_revision: 7,
            } if inserted_text == "hello"
        ));
    }

    #[test]
    fn test_error_roundtrip_with_metadata() {
        let msg = CoreToUi::Error {
            message: "stale revision".to_string(),
            code: ErrorCode::StaleRevision,
            retryable: true,
            should_resync: true,
        };

        let bytes = serialize(&msg).unwrap();
        let decoded: CoreToUi = deserialize(&bytes).unwrap();
        assert!(matches!(
            decoded,
            CoreToUi::Error {
                message,
                code: ErrorCode::StaleRevision,
                retryable: true,
                should_resync: true,
            } if message == "stale revision"
        ));

        let json_bytes = serialize_json(&msg).unwrap();
        let json_decoded: CoreToUi = deserialize_json(&json_bytes).unwrap();
        assert!(matches!(
            json_decoded,
            CoreToUi::Error {
                message,
                code: ErrorCode::StaleRevision,
                retryable: true,
                should_resync: true,
            } if message == "stale revision"
        ));
    }

    #[test]
    fn test_legacy_error_defaults_metadata() {
        let json = br#"{"Error":{"message":"legacy error"}}"#;
        let decoded: CoreToUi = deserialize_json(json).unwrap();
        assert!(matches!(
            decoded,
            CoreToUi::Error {
                message,
                code: ErrorCode::Unknown,
                retryable: false,
                should_resync: false,
            } if message == "legacy error"
        ));
    }
}
