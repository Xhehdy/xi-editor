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
    /// Queue a file for debounced background indexing
    QueueIndexFile { path: String },
    /// Flush/cancel queued indexing work
    FlushIndexQueue,
    /// Read indexing queue metrics
    GetIndexQueueStats,
    /// Query symbols in file
    QuerySymbols { path: String },
    /// Search symbols by name
    SearchSymbols { query: String },
    /// Query graph nodes/edges for a specific file
    QueryGraphFile { path: String },
    /// Search graph nodes by name with optional pagination and filters
    SearchGraph {
        query: String,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        kind_filter: Option<String>,
        #[serde(default)]
        file_filter: Option<String>,
    },
    /// Build ranked graph context for chat/intent/wiki consumption
    QueryGraphContext {
        query: String,
        #[serde(default)]
        context_files: Option<Vec<String>>,
        #[serde(default)]
        limit: Option<usize>,
    },
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
    IndexQueueFull,
    GraphQueryFailed,
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
    /// File accepted into debounced index queue
    IndexQueued {
        path: String,
        queued_count: usize,
        debounce_ms: u64,
    },
    /// Index queue flushed
    IndexQueueFlushed { cancelled: usize },
    /// Index queue metrics
    IndexQueueStats { stats: IndexQueueStatsInfo },
    /// Symbol query results
    Symbols { symbols: Vec<SymbolInfo> },
    /// Graph query results
    GraphData {
        nodes: Vec<GraphNodeInfo>,
        edges: Vec<GraphEdgeInfo>,
    },
    /// Ranked graph context payload
    GraphContext {
        summary: String,
        items: Vec<GraphContextItem>,
    },
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

/// Graph node information for UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeInfo {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub file: Option<String>,
}

/// Graph edge information for UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgeInfo {
    pub from_id: i64,
    pub to_id: i64,
    pub kind: String,
}

/// Ranked graph context item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphContextItem {
    pub node: GraphNodeInfo,
    pub score: f64,
    pub incoming: usize,
    pub outgoing: usize,
    pub reasons: Vec<String>,
}

/// Background index queue metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexQueueStatsInfo {
    pub queued: usize,
    pub in_progress: usize,
    pub completed: u64,
    pub failed: u64,
    pub retried: u64,
    pub dropped: u64,
    pub max_pending: usize,
    pub debounce_ms: u64,
    pub max_retries: u8,
    pub last_error: Option<String>,
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

    #[test]
    fn test_roundtrip_graph_messages() {
        let file_query = UiToCore::QueryGraphFile {
            path: "src/lib.rs".to_string(),
        };
        let file_query_msgpack = serialize(&file_query).unwrap();
        let file_query_decoded: UiToCore = deserialize(&file_query_msgpack).unwrap();
        assert!(matches!(
            file_query_decoded,
            UiToCore::QueryGraphFile { path } if path == "src/lib.rs"
        ));
        let file_query_json = serialize_json(&file_query).unwrap();
        let file_query_json_decoded: UiToCore = deserialize_json(&file_query_json).unwrap();
        assert!(matches!(
            file_query_json_decoded,
            UiToCore::QueryGraphFile { path } if path == "src/lib.rs"
        ));

        let search_query = UiToCore::SearchGraph {
            query: "handler".to_string(),
            limit: Some(25),
            offset: Some(5),
            kind_filter: Some("function".to_string()),
            file_filter: Some("src/lib.rs".to_string()),
        };

        let search_query_msgpack = serialize(&search_query).unwrap();
        let search_query_decoded: UiToCore = deserialize(&search_query_msgpack).unwrap();
        assert!(matches!(
            search_query_decoded,
            UiToCore::SearchGraph {
                query,
                limit: Some(25),
                offset: Some(5),
                kind_filter: Some(kind_filter),
                file_filter: Some(file_filter),
            } if query == "handler"
                && kind_filter == "function"
                && file_filter == "src/lib.rs"
        ));
        let search_query_json = serialize_json(&search_query).unwrap();
        let search_query_json_decoded: UiToCore = deserialize_json(&search_query_json).unwrap();
        assert!(matches!(
            search_query_json_decoded,
            UiToCore::SearchGraph {
                query,
                limit: Some(25),
                offset: Some(5),
                kind_filter: Some(kind_filter),
                file_filter: Some(file_filter),
            } if query == "handler"
                && kind_filter == "function"
                && file_filter == "src/lib.rs"
        ));

        let legacy_search_json = br#"{"SearchGraph":{"query":"handler","limit":25}}"#;
        let legacy_search_decoded: UiToCore = deserialize_json(legacy_search_json).unwrap();
        assert!(matches!(
            legacy_search_decoded,
            UiToCore::SearchGraph {
                query,
                limit: Some(25),
                offset: None,
                kind_filter: None,
                file_filter: None,
            } if query == "handler"
        ));

        let context_query = UiToCore::QueryGraphContext {
            query: "refactor handler".to_string(),
            context_files: Some(vec!["src/lib.rs".to_string(), "src/router.rs".to_string()]),
            limit: Some(8),
        };
        let context_query_json = serialize_json(&context_query).unwrap();
        let context_query_json_decoded: UiToCore = deserialize_json(&context_query_json).unwrap();
        assert!(matches!(
            context_query_json_decoded,
            UiToCore::QueryGraphContext {
                query,
                context_files: Some(files),
                limit: Some(8),
            } if query == "refactor handler"
                && files.len() == 2
                && files[0] == "src/lib.rs"
                && files[1] == "src/router.rs"
        ));

        let legacy_context_query_json = br#"{"QueryGraphContext":{"query":"refactor handler"}}"#;
        let legacy_context_query_decoded: UiToCore =
            deserialize_json(legacy_context_query_json).unwrap();
        assert!(matches!(
            legacy_context_query_decoded,
            UiToCore::QueryGraphContext {
                query,
                context_files: None,
                limit: None,
            } if query == "refactor handler"
        ));

        let response = CoreToUi::GraphData {
            nodes: vec![GraphNodeInfo {
                id: 1,
                kind: "function".to_string(),
                name: "apply_edit".to_string(),
                file: Some("src/lib.rs".to_string()),
            }],
            edges: vec![GraphEdgeInfo {
                from_id: 1,
                to_id: 2,
                kind: "calls".to_string(),
            }],
        };

        let msgpack_bytes = serialize(&response).unwrap();
        let msgpack_decoded: CoreToUi = deserialize(&msgpack_bytes).unwrap();
        assert!(matches!(
            msgpack_decoded,
            CoreToUi::GraphData { nodes, edges }
                if nodes.len() == 1
                    && nodes[0].name == "apply_edit"
                    && edges.len() == 1
                    && edges[0].kind == "calls"
        ));

        let json_bytes = serialize_json(&response).unwrap();
        let json_decoded: CoreToUi = deserialize_json(&json_bytes).unwrap();
        assert!(matches!(
            json_decoded,
            CoreToUi::GraphData { nodes, edges }
                if nodes.len() == 1
                    && nodes[0].name == "apply_edit"
                    && edges.len() == 1
                    && edges[0].kind == "calls"
        ));

        let context_response = CoreToUi::GraphContext {
            summary: "ranked graph context".to_string(),
            items: vec![GraphContextItem {
                node: GraphNodeInfo {
                    id: 10,
                    kind: "function".to_string(),
                    name: "handler".to_string(),
                    file: Some("src/lib.rs".to_string()),
                },
                score: 12.5,
                incoming: 3,
                outgoing: 5,
                reasons: vec!["name_match".to_string(), "file_scope".to_string()],
            }],
        };
        let context_response_msgpack = serialize(&context_response).unwrap();
        let context_response_decoded: CoreToUi = deserialize(&context_response_msgpack).unwrap();
        assert!(matches!(
            context_response_decoded,
            CoreToUi::GraphContext { summary, items }
                if summary == "ranked graph context"
                    && items.len() == 1
                    && items[0].node.name == "handler"
                    && (items[0].score - 12.5).abs() < f64::EPSILON
                    && items[0].incoming == 3
                    && items[0].outgoing == 5
                    && items[0].reasons.len() == 2
        ));

        let context_response_json = serialize_json(&context_response).unwrap();
        let context_response_json_decoded: CoreToUi =
            deserialize_json(&context_response_json).unwrap();
        assert!(matches!(
            context_response_json_decoded,
            CoreToUi::GraphContext { summary, items }
                if summary == "ranked graph context"
                    && items.len() == 1
                    && items[0].node.name == "handler"
                    && (items[0].score - 12.5).abs() < f64::EPSILON
                    && items[0].incoming == 3
                    && items[0].outgoing == 5
                    && items[0].reasons.len() == 2
        ));
    }

    #[test]
    fn test_roundtrip_index_queue_messages() {
        let queued = UiToCore::QueueIndexFile {
            path: "src/lib.rs".to_string(),
        };
        let queued_msgpack = serialize(&queued).unwrap();
        let queued_decoded: UiToCore = deserialize(&queued_msgpack).unwrap();
        assert!(matches!(
            queued_decoded,
            UiToCore::QueueIndexFile { path } if path == "src/lib.rs"
        ));

        let stats_req = UiToCore::GetIndexQueueStats;
        let stats_req_json = serialize_json(&stats_req).unwrap();
        let stats_req_decoded: UiToCore = deserialize_json(&stats_req_json).unwrap();
        assert!(matches!(stats_req_decoded, UiToCore::GetIndexQueueStats));

        let stats_response = CoreToUi::IndexQueueStats {
            stats: IndexQueueStatsInfo {
                queued: 2,
                in_progress: 1,
                completed: 10,
                failed: 1,
                retried: 3,
                dropped: 0,
                max_pending: 256,
                debounce_ms: 250,
                max_retries: 2,
                last_error: Some("transient failure".to_string()),
            },
        };
        let stats_response_json = serialize_json(&stats_response).unwrap();
        let stats_response_decoded: CoreToUi = deserialize_json(&stats_response_json).unwrap();
        assert!(matches!(
            stats_response_decoded,
            CoreToUi::IndexQueueStats { stats }
                if stats.queued == 2
                    && stats.in_progress == 1
                    && stats.debounce_ms == 250
                    && stats.last_error.as_deref() == Some("transient failure")
        ));
    }
}
