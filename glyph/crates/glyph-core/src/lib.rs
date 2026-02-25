//! Glyph Core - Main coordinator for the Glyph IDE
//!
//! This is the brain that ties everything together.

pub mod editor;

use crate::editor::EditorCore;
use glyph_buffer::{Buffer, BufferId};
use glyph_ce::ComprehensionEngine;
use glyph_events::{CoreEvent, InputEvent, ViewId};
use glyph_graph::CodeGraph;
use glyph_llm::LlmClient;
use glyph_patch::Patch;
use glyph_protocol::{CoreToUi, ErrorCode, SymbolInfo, UiToCore};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use glyph_syntax::{Language, SyntaxHighlighter};

/// The main Glyph core state
pub struct Glyph {
    /// Active buffers
    buffers: Arc<RwLock<HashMap<BufferId, Buffer>>>,
    /// View to buffer mapping
    views: Arc<RwLock<HashMap<ViewId, BufferId>>>,
    /// Buffer language mapping
    languages: Arc<RwLock<HashMap<BufferId, Language>>>,
    /// Monotonic content revision per buffer
    revisions: Arc<RwLock<HashMap<BufferId, u64>>>,
    /// Cursor position per view (byte offset)
    cursors: Arc<RwLock<HashMap<ViewId, usize>>>,
    /// Syntax highlighter
    highlighter: Arc<RwLock<SyntaxHighlighter>>,
    /// Comprehension Engine
    ce: Arc<RwLock<ComprehensionEngine>>,
    /// Code Knowledge Graph
    graph: Arc<Mutex<CodeGraph>>,
    /// LLM Client
    llm: Arc<LlmClient>,
    /// Next available ID
    next_id: AtomicU64,
    /// Core version
    pub version: String,
}

impl Glyph {
    const COMPLETION_PREFIX_BYTES: usize = 2048;
    const COMPLETION_SUFFIX_BYTES: usize = 512;
    const COMPLETION_MAX_BYTES: usize = 512;
    const COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);

    /// Creates a new Glyph instance
    pub fn new() -> Self {
        // Use in-memory graph for now
        let graph = CodeGraph::in_memory().expect("Failed to create code graph");

        // Initialize LLM client (pointing to sidecar)
        let llm = LlmClient::new("http://127.0.0.1:8000");

        Self {
            buffers: Arc::new(RwLock::new(HashMap::new())),
            views: Arc::new(RwLock::new(HashMap::new())),
            languages: Arc::new(RwLock::new(HashMap::new())),
            revisions: Arc::new(RwLock::new(HashMap::new())),
            cursors: Arc::new(RwLock::new(HashMap::new())),
            highlighter: Arc::new(RwLock::new(SyntaxHighlighter::new().unwrap_or_default())),
            ce: Arc::new(RwLock::new(ComprehensionEngine::new())),
            graph: Arc::new(Mutex::new(graph)),
            llm: Arc::new(llm),
            next_id: AtomicU64::new(1),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Generates a new unique ID
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Helper to highlight content
    async fn highlight(&self, content: &str, lang: Language) -> Vec<glyph_protocol::HighlightSpan> {
        let mut highlighter = self.highlighter.write().await;
        let spans = highlighter.highlight(content, lang).unwrap_or_default();

        // Convert from syntax span to protocol span
        spans
            .into_iter()
            .map(|s| glyph_protocol::HighlightSpan {
                start: s.start,
                end: s.end,
                highlight: s.highlight,
            })
            .collect()
    }

    fn advance_revision_if_changed(revision: &mut u64, patch: &Patch) -> u64 {
        if !patch.ops().is_empty() {
            *revision = revision.saturating_add(1);
        }
        *revision
    }

    fn protocol_error(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
        should_resync: bool,
    ) -> CoreToUi {
        CoreToUi::Error {
            message: message.into(),
            code,
            retryable,
            should_resync,
        }
    }

    fn completion_prompt() -> &'static str {
        "Complete code at the cursor.\n\
         Return only the exact text to insert at the cursor.\n\
         Do not repeat existing text, do not add markdown fences, and do not explain."
    }

    fn clamp_boundary_left(text: &str, mut index: usize) -> usize {
        index = index.min(text.len());
        while index > 0 && !text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }

    fn clamp_boundary_right(text: &str, mut index: usize) -> usize {
        index = index.min(text.len());
        while index < text.len() && !text.is_char_boundary(index) {
            index += 1;
        }
        index
    }

    fn build_completion_context(content: &str, position: usize) -> (usize, String) {
        let cursor = Self::clamp_boundary_left(content, position);
        let before_start = Self::clamp_boundary_left(
            content,
            cursor.saturating_sub(Self::COMPLETION_PREFIX_BYTES),
        );
        let after_end = Self::clamp_boundary_right(
            content,
            cursor.saturating_add(Self::COMPLETION_SUFFIX_BYTES),
        );

        let before = &content[before_start..cursor];
        let after = &content[cursor..after_end];

        (
            cursor,
            format!(
                "Cursor byte offset: {cursor}\n\
                 Text before cursor:\n<before>\n{before}\n</before>\n\
                 Text after cursor:\n<after>\n{after}\n</after>"
            ),
        )
    }

    fn truncate_utf8(text: &str, max_bytes: usize) -> String {
        if text.len() <= max_bytes {
            return text.to_string();
        }

        let mut end = max_bytes.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].to_string()
    }

    fn strip_markdown_fence(text: &str) -> String {
        let trimmed = text.trim();
        if !trimmed.starts_with("```") {
            return trimmed.to_string();
        }

        let mut lines = trimmed.lines();
        let Some(first) = lines.next() else {
            return String::new();
        };

        if !first.trim_start().starts_with("```") {
            return trimmed.to_string();
        }

        let mut body: Vec<&str> = lines.collect();
        if body
            .last()
            .map(|line| line.trim_start().starts_with("```"))
            .unwrap_or(false)
        {
            body.pop();
        }
        body.join("\n").trim().to_string()
    }

    fn sanitize_ghost_text(raw: &str) -> String {
        let normalized = raw.replace("\r\n", "\n");
        let mut text = Self::strip_markdown_fence(&normalized);

        if text.eq_ignore_ascii_case("none")
            || text.eq_ignore_ascii_case("no completion")
            || text.eq_ignore_ascii_case("no suggestion")
        {
            return String::new();
        }

        if text.starts_with("Completion:") {
            text = text
                .trim_start_matches("Completion:")
                .trim_start()
                .to_string();
        }

        let text = text.trim_start_matches('\n').to_string();
        Self::truncate_utf8(&text, Self::COMPLETION_MAX_BYTES)
    }

    /// Handles an incoming message from the UI
    pub async fn handle_message(&self, msg: UiToCore) -> CoreToUi {
        match msg {
            UiToCore::Hello { client_version, .. } => {
                info!("Client connected: {}", client_version);
                CoreToUi::Welcome {
                    core_version: self.version.clone(),
                }
            }

            UiToCore::NewView { path } => {
                let buffer_id = BufferId(self.next_id());
                let view_id = ViewId(self.next_id());

                // Detect language
                let lang = if let Some(ref p) = path {
                    let ext = std::path::Path::new(p)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    Language::from_extension(ext)
                } else {
                    Language::Plain
                };

                // Load content from path or create empty
                let content = if let Some(ref p) = path {
                    match tokio::fs::read_to_string(p).await {
                        Ok(content) => content,
                        Err(e) => {
                            return Self::protocol_error(
                                ErrorCode::FileReadFailed,
                                format!("Failed to open file '{}': {}", p, e),
                                false,
                                false,
                            );
                        }
                    }
                } else {
                    String::new()
                };

                let buffer = Buffer::new(buffer_id, content.clone());

                // Store buffer and view mapping
                {
                    let mut buffers = self.buffers.write().await;
                    buffers.insert(buffer_id, buffer);
                }
                {
                    let mut views = self.views.write().await;
                    views.insert(view_id, buffer_id);
                }
                {
                    let mut langs = self.languages.write().await;
                    langs.insert(buffer_id, lang);
                }
                {
                    let mut revisions = self.revisions.write().await;
                    revisions.insert(buffer_id, 0);
                }
                {
                    let mut cursors = self.cursors.write().await;
                    cursors.insert(view_id, content.len());
                }

                info!(
                    "Created view {:?} for buffer {:?} (Lang: {:?})",
                    view_id, buffer_id, lang
                );

                CoreToUi::ViewCreated {
                    view_id,
                    content,
                    revision: 0,
                }
            }

            UiToCore::CloseView { view_id } => {
                let mut views = self.views.write().await;
                if let Some(buffer_id) = views.remove(&view_id) {
                    // Check if any other views use this buffer
                    let still_used = views.values().any(|id| *id == buffer_id);
                    if !still_used {
                        let mut buffers = self.buffers.write().await;
                        buffers.remove(&buffer_id);
                        let mut langs = self.languages.write().await;
                        langs.remove(&buffer_id);
                        let mut revisions = self.revisions.write().await;
                        revisions.remove(&buffer_id);
                    }
                    let mut cursors = self.cursors.write().await;
                    cursors.remove(&view_id);
                    info!("Closed view {:?}", view_id);
                }
                CoreToUi::Event(CoreEvent::BufferChanged { view_id })
            }

            UiToCore::Input { view_id, event } => {
                // Get buffer ID for this view
                let buffer_id = {
                    let views = self.views.read().await;
                    views.get(&view_id).copied()
                };

                let Some(buffer_id) = buffer_id else {
                    return Self::protocol_error(
                        ErrorCode::ViewNotFound,
                        format!("View {:?} not found", view_id),
                        false,
                        false,
                    );
                };

                let mut buffers = self.buffers.write().await;
                let Some(buffer) = buffers.get_mut(&buffer_id) else {
                    return Self::protocol_error(
                        ErrorCode::BufferNotFound,
                        format!("Buffer for view {:?} not found", view_id),
                        false,
                        true,
                    );
                };

                let mut cursors = self.cursors.write().await;
                let cursor = cursors.entry(view_id).or_insert(buffer.len());
                *cursor = buffer.nearest_char_boundary((*cursor).min(buffer.len()));
                let mut revisions = self.revisions.write().await;
                let revision = revisions.entry(buffer_id).or_insert(0);

                let response = match event {
                    InputEvent::Insert { text } => {
                        let patch = match EditorCore::apply_insert(buffer, cursor, &text) {
                            Ok(patch) => patch,
                            Err(e) => {
                                return Self::protocol_error(
                                    ErrorCode::InvalidRange,
                                    format!("Insert failed: {}", e),
                                    false,
                                    true,
                                );
                            }
                        };
                        let revision = Self::advance_revision_if_changed(revision, &patch);
                        CoreToUi::ApplyPatch {
                            view_id,
                            patch,
                            revision,
                        }
                    }
                    InputEvent::Backspace => {
                        let patch = match EditorCore::apply_backspace(buffer, cursor) {
                            Ok(patch) => patch,
                            Err(e) => {
                                return Self::protocol_error(
                                    ErrorCode::InvalidRange,
                                    format!("Backspace failed: {}", e),
                                    false,
                                    true,
                                );
                            }
                        };
                        let revision = Self::advance_revision_if_changed(revision, &patch);
                        CoreToUi::ApplyPatch {
                            view_id,
                            patch,
                            revision,
                        }
                    }
                    InputEvent::Delete => {
                        let patch = match EditorCore::apply_delete(buffer, cursor) {
                            Ok(patch) => patch,
                            Err(e) => {
                                return Self::protocol_error(
                                    ErrorCode::InvalidRange,
                                    format!("Delete failed: {}", e),
                                    false,
                                    true,
                                );
                            }
                        };
                        let revision = Self::advance_revision_if_changed(revision, &patch);
                        CoreToUi::ApplyPatch {
                            view_id,
                            patch,
                            revision,
                        }
                    }
                    InputEvent::Move(movement) => {
                        *cursor = EditorCore::move_cursor(buffer, *cursor, movement);
                        CoreToUi::Event(CoreEvent::CursorMoved {
                            view_id,
                            position: *cursor,
                        })
                    }
                    InputEvent::Select { end, .. } => {
                        *cursor = buffer.nearest_char_boundary(end.min(buffer.len()));
                        CoreToUi::Event(CoreEvent::CursorMoved {
                            view_id,
                            position: *cursor,
                        })
                    }
                    InputEvent::Undo => {
                        let patch = match EditorCore::apply_undo(buffer, cursor) {
                            Ok(patch) => patch,
                            Err(e) => {
                                return Self::protocol_error(
                                    ErrorCode::InvalidRange,
                                    format!("Undo failed: {}", e),
                                    false,
                                    true,
                                );
                            }
                        };
                        let revision = Self::advance_revision_if_changed(revision, &patch);
                        CoreToUi::ApplyPatch {
                            view_id,
                            patch,
                            revision,
                        }
                    }
                    InputEvent::Redo => {
                        let patch = match EditorCore::apply_redo(buffer, cursor) {
                            Ok(patch) => patch,
                            Err(e) => {
                                return Self::protocol_error(
                                    ErrorCode::InvalidRange,
                                    format!("Redo failed: {}", e),
                                    false,
                                    true,
                                );
                            }
                        };
                        let revision = Self::advance_revision_if_changed(revision, &patch);
                        CoreToUi::ApplyPatch {
                            view_id,
                            patch,
                            revision,
                        }
                    }
                    InputEvent::Save => {
                        buffer.mark_saved();
                        CoreToUi::Event(CoreEvent::BufferChanged { view_id })
                    }
                    InputEvent::Find { .. } => CoreToUi::Event(CoreEvent::CursorMoved {
                        view_id,
                        position: *cursor,
                    }),
                };

                response
            }

            UiToCore::ApplyEdit {
                view_id,
                start,
                deleted_len,
                inserted_text,
                base_revision,
            } => {
                let buffer_id = {
                    let views = self.views.read().await;
                    views.get(&view_id).copied()
                };

                let Some(buffer_id) = buffer_id else {
                    return Self::protocol_error(
                        ErrorCode::ViewNotFound,
                        format!("View {:?} not found", view_id),
                        false,
                        false,
                    );
                };

                let end = match start.checked_add(deleted_len) {
                    Some(end) => end,
                    None => {
                        return Self::protocol_error(
                            ErrorCode::InvalidRequest,
                            format!(
                                "ApplyEdit failed: range overflow for start={} deleted_len={}",
                                start, deleted_len
                            ),
                            false,
                            false,
                        );
                    }
                };

                let mut buffers = self.buffers.write().await;
                let Some(buffer) = buffers.get_mut(&buffer_id) else {
                    return Self::protocol_error(
                        ErrorCode::BufferNotFound,
                        format!("Buffer for view {:?} not found", view_id),
                        false,
                        true,
                    );
                };

                let mut cursors = self.cursors.write().await;
                let mut revisions = self.revisions.write().await;
                let revision = revisions.entry(buffer_id).or_insert(0);
                if base_revision != *revision {
                    return Self::protocol_error(
                        ErrorCode::StaleRevision,
                        format!(
                            "ApplyEdit failed: stale revision base={} current={}",
                            base_revision, *revision
                        ),
                        true,
                        true,
                    );
                }

                let patch = match EditorCore::apply_range_edit(
                    buffer,
                    cursors.entry(view_id).or_insert(0),
                    start..end,
                    &inserted_text,
                ) {
                    Ok(patch) => patch,
                    Err(e) => {
                        return Self::protocol_error(
                            ErrorCode::InvalidRange,
                            format!("ApplyEdit failed: {}", e),
                            false,
                            true,
                        );
                    }
                };
                let revision = Self::advance_revision_if_changed(revision, &patch);
                CoreToUi::ApplyPatch {
                    view_id,
                    patch,
                    revision,
                }
            }

            UiToCore::GetContent { view_id } => {
                let (buffer_id, lang, cursor, revision) = {
                    let views = self.views.read().await;
                    if let Some(&bid) = views.get(&view_id) {
                        let langs = self.languages.read().await;
                        let cursors = self.cursors.read().await;
                        let revisions = self.revisions.read().await;
                        (
                            Some(bid),
                            langs.get(&bid).copied().unwrap_or(Language::Plain),
                            cursors.get(&view_id).copied().unwrap_or(0),
                            revisions.get(&bid).copied().unwrap_or(0),
                        )
                    } else {
                        (None, Language::Plain, 0, 0)
                    }
                };

                if let Some(buffer_id) = buffer_id {
                    let buffers = self.buffers.read().await;
                    if let Some(buffer) = buffers.get(&buffer_id) {
                        let content = buffer.content();
                        let cursor = buffer.nearest_char_boundary(cursor.min(buffer.len()));
                        drop(buffers);
                        let spans = self.highlight(&content, lang).await;

                        if let Ok(mut cursors) = self.cursors.try_write() {
                            cursors.insert(view_id, cursor);
                        }

                        return CoreToUi::SetContent {
                            view_id,
                            content,
                            spans,
                            revision,
                        };
                    }
                }

                Self::protocol_error(
                    ErrorCode::ViewNotFound,
                    format!("View {:?} not found", view_id),
                    false,
                    false,
                )
            }

            UiToCore::Chat {
                message,
                context_files,
            } => {
                // Build context (simplified for now)
                let context = if !context_files.is_empty() {
                    Some(format!(
                        "User is looking at specific files: {:?}",
                        context_files
                    ))
                } else {
                    None
                };

                match self.llm.complete(&message, context.as_deref()).await {
                    Ok(response) => CoreToUi::ChatToken {
                        token: response,
                        done: true,
                    },
                    Err(e) => Self::protocol_error(
                        ErrorCode::LlmFailure,
                        format!("AI Error: {}", e),
                        true,
                        false,
                    ),
                }
            }

            UiToCore::RequestCompletion { view_id, position } => {
                let buffer_id = {
                    let views = self.views.read().await;
                    views.get(&view_id).copied()
                };

                let Some(buffer_id) = buffer_id else {
                    return Self::protocol_error(
                        ErrorCode::ViewNotFound,
                        format!("View {:?} not found", view_id),
                        false,
                        false,
                    );
                };

                let (content, cursor) = {
                    let buffers = self.buffers.read().await;
                    let Some(buffer) = buffers.get(&buffer_id) else {
                        return Self::protocol_error(
                            ErrorCode::BufferNotFound,
                            format!("Buffer for view {:?} not found", view_id),
                            false,
                            true,
                        );
                    };
                    let cursor = buffer.nearest_char_boundary(position.min(buffer.len()));
                    (buffer.content(), cursor)
                };

                let (_, context) = Self::build_completion_context(&content, cursor);
                let completion = match tokio::time::timeout(
                    Self::COMPLETION_TIMEOUT,
                    self.llm.complete(Self::completion_prompt(), Some(&context)),
                )
                .await
                {
                    Ok(Ok(raw)) => Self::sanitize_ghost_text(&raw),
                    Ok(Err(err)) => {
                        warn!("completion request failed for view {:?}: {}", view_id, err);
                        return Self::protocol_error(
                            ErrorCode::LlmFailure,
                            format!("Completion failed: {}", err),
                            true,
                            false,
                        );
                    }
                    Err(_) => {
                        warn!("completion request timed out for view {:?}", view_id);
                        return Self::protocol_error(
                            ErrorCode::CompletionTimeout,
                            "Completion request timed out",
                            true,
                            false,
                        );
                    }
                };

                CoreToUi::GhostText {
                    view_id,
                    text: completion,
                }
            }

            UiToCore::IndexFile { path } => {
                // Read file and index it
                let content = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(e) => {
                        return Self::protocol_error(
                            ErrorCode::FileReadFailed,
                            format!("Failed to read file: {}", e),
                            false,
                            false,
                        );
                    }
                };

                // Index with CE
                let mut ce = self.ce.write().await;
                let path_buf = Path::new(&path);
                match ce.index_file(path_buf, &content) {
                    Ok(symbol_ids) => {
                        info!("Indexed {} symbols from {}", symbol_ids.len(), path);

                        // Also add to graph
                        let file_node = glyph_graph::Node::new_file(self.next_id() as i64, &path);
                        if let Ok(graph) = self.graph.lock() {
                            let _ = graph.add_node(&file_node);
                        } else {
                            error!("Failed to lock graph");
                        }

                        CoreToUi::FileIndexed {
                            path,
                            symbol_count: symbol_ids.len(),
                        }
                    }
                    Err(e) => Self::protocol_error(
                        ErrorCode::IndexingFailed,
                        format!("Indexing failed: {}", e),
                        false,
                        false,
                    ),
                }
            }

            UiToCore::QuerySymbols { path } => {
                let ce = self.ce.read().await;
                let path_buf = Path::new(&path);
                let symbols = ce.symbols_in_file(path_buf);

                let symbol_infos: Vec<SymbolInfo> = symbols
                    .into_iter()
                    .map(|s| SymbolInfo {
                        name: s.name.clone(),
                        kind: s.kind.as_str().to_string(),
                        file: s.file.to_string_lossy().to_string(),
                        line: s.start_line,
                        signature: s.signature.clone(),
                    })
                    .collect();

                CoreToUi::Symbols {
                    symbols: symbol_infos,
                }
            }

            UiToCore::SearchSymbols { query } => {
                let ce = self.ce.read().await;
                let symbols = ce.find_by_name(&query);

                let symbol_infos: Vec<SymbolInfo> = symbols
                    .into_iter()
                    .map(|s| SymbolInfo {
                        name: s.name.clone(),
                        kind: s.kind.as_str().to_string(),
                        file: s.file.to_string_lossy().to_string(),
                        line: s.start_line,
                        signature: s.signature.clone(),
                    })
                    .collect();

                CoreToUi::Symbols {
                    symbols: symbol_infos,
                }
            }
        }
    }
}

impl Default for Glyph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_events::Movement;

    async fn create_view(glyph: &Glyph) -> ViewId {
        match glyph.handle_message(UiToCore::NewView { path: None }).await {
            CoreToUi::ViewCreated { view_id, .. } => view_id,
            other => panic!("unexpected response: {:?}", other),
        }
    }

    async fn get_content(glyph: &Glyph, view_id: ViewId) -> String {
        match glyph.handle_message(UiToCore::GetContent { view_id }).await {
            CoreToUi::SetContent { content, .. } => content,
            other => panic!("unexpected response: {:?}", other),
        }
    }

    async fn get_content_with_revision(glyph: &Glyph, view_id: ViewId) -> (String, u64) {
        match glyph.handle_message(UiToCore::GetContent { view_id }).await {
            CoreToUi::SetContent {
                content, revision, ..
            } => (content, revision),
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_insert_respects_cursor_position() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let first_insert = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "ab".to_string(),
                },
            })
            .await;
        assert!(matches!(first_insert, CoreToUi::ApplyPatch { .. }));

        let _ = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Move(Movement::Left),
            })
            .await;

        let second_insert = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "X".to_string(),
                },
            })
            .await;
        assert!(matches!(second_insert, CoreToUi::ApplyPatch { .. }));

        let content = get_content(&glyph, view_id).await;

        assert_eq!(content, "aXb");
    }

    #[tokio::test]
    async fn test_apply_edit_single_rpc_returns_roundtrippable_patch() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let _ = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "abc".to_string(),
                },
            })
            .await;
        let before = get_content(&glyph, view_id).await;

        let response = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 1,
                deleted_len: 1,
                inserted_text: "XYZ".to_string(),
                base_revision: 1,
            })
            .await;

        let patch = match response {
            CoreToUi::ApplyPatch { patch, .. } => patch,
            other => panic!("expected apply patch response, got {:?}", other),
        };

        let after = get_content(&glyph, view_id).await;
        assert_eq!(patch.apply(&before).unwrap(), after);
        assert_eq!(after, "aXYZc");
    }

    #[tokio::test]
    async fn test_apply_edit_rejects_invalid_range() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let response = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 10,
                deleted_len: 1,
                inserted_text: String::new(),
                base_revision: 0,
            })
            .await;

        assert!(matches!(response, CoreToUi::Error { .. }));
    }

    #[tokio::test]
    async fn test_apply_edit_rejects_non_utf8_boundary_range() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let _ = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "é".to_string(),
                },
            })
            .await;

        let response = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 1,
                deleted_len: 1,
                inserted_text: String::new(),
                base_revision: 1,
            })
            .await;

        assert!(matches!(response, CoreToUi::Error { .. }));
    }

    #[tokio::test]
    async fn test_apply_edit_rejects_stale_revision() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let _ = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "abc".to_string(),
                },
            })
            .await;

        let response = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 0,
                deleted_len: 1,
                inserted_text: "z".to_string(),
                base_revision: 0,
            })
            .await;

        assert!(matches!(
            response,
            CoreToUi::Error {
                code: ErrorCode::StaleRevision,
                retryable: true,
                should_resync: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_apply_edit_replay_rejected_with_stale_revision() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let first = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 0,
                deleted_len: 0,
                inserted_text: "x".to_string(),
                base_revision: 0,
            })
            .await;
        assert!(matches!(first, CoreToUi::ApplyPatch { revision: 1, .. }));

        let replay = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 0,
                deleted_len: 0,
                inserted_text: "x".to_string(),
                base_revision: 0,
            })
            .await;
        assert!(matches!(
            replay,
            CoreToUi::Error {
                code: ErrorCode::StaleRevision,
                retryable: true,
                should_resync: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_out_of_order_apply_edit_requires_resync_then_succeeds() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let first = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 0,
                deleted_len: 0,
                inserted_text: "abc".to_string(),
                base_revision: 0,
            })
            .await;
        assert!(matches!(first, CoreToUi::ApplyPatch { revision: 1, .. }));

        let second = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 1,
                deleted_len: 1,
                inserted_text: "Z".to_string(),
                base_revision: 1,
            })
            .await;
        assert!(matches!(second, CoreToUi::ApplyPatch { revision: 2, .. }));

        let out_of_order = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 2,
                deleted_len: 1,
                inserted_text: "Y".to_string(),
                base_revision: 1,
            })
            .await;
        assert!(matches!(
            out_of_order,
            CoreToUi::Error {
                code: ErrorCode::StaleRevision,
                retryable: true,
                should_resync: true,
                ..
            }
        ));

        let (content, revision) = get_content_with_revision(&glyph, view_id).await;
        assert_eq!(content, "aZc");
        assert_eq!(revision, 2);

        let recovered = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 2,
                deleted_len: 1,
                inserted_text: "Y".to_string(),
                base_revision: revision,
            })
            .await;
        assert!(matches!(
            recovered,
            CoreToUi::ApplyPatch { revision: 3, .. }
        ));

        let final_content = get_content(&glyph, view_id).await;
        assert_eq!(final_content, "aZY");
    }

    #[tokio::test]
    async fn test_reconnect_hello_does_not_reset_view_revision_state() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let edit = glyph
            .handle_message(UiToCore::ApplyEdit {
                view_id,
                start: 0,
                deleted_len: 0,
                inserted_text: "state".to_string(),
                base_revision: 0,
            })
            .await;
        assert!(matches!(edit, CoreToUi::ApplyPatch { revision: 1, .. }));

        let hello = glyph
            .handle_message(UiToCore::Hello {
                client_version: "reconnect-test".to_string(),
                capabilities: None,
            })
            .await;
        assert!(matches!(hello, CoreToUi::Welcome { .. }));

        let (_, revision) = get_content_with_revision(&glyph, view_id).await;
        assert_eq!(revision, 1);
    }

    #[tokio::test]
    async fn test_mutating_input_returns_apply_patch_with_roundtrip_result() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;
        let before = get_content(&glyph, view_id).await;

        let response = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "hello".to_string(),
                },
            })
            .await;

        let patch = match response {
            CoreToUi::ApplyPatch { patch, .. } => patch,
            other => panic!("expected apply patch response, got {:?}", other),
        };

        let patched = patch.apply(&before).unwrap();
        let after = get_content(&glyph, view_id).await;

        assert_eq!(patched, after);
        assert_eq!(after, "hello");
    }

    #[tokio::test]
    async fn test_undo_redo_return_roundtrippable_patches() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let _ = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "abc".to_string(),
                },
            })
            .await;

        let before_undo = get_content(&glyph, view_id).await;
        let undo_response = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Undo,
            })
            .await;
        let undo_patch = match undo_response {
            CoreToUi::ApplyPatch { patch, .. } => patch,
            other => panic!("expected apply patch response, got {:?}", other),
        };
        let after_undo = get_content(&glyph, view_id).await;
        assert_eq!(undo_patch.apply(&before_undo).unwrap(), after_undo);
        assert_eq!(after_undo, "");

        let before_redo = after_undo;
        let redo_response = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Redo,
            })
            .await;
        let redo_patch = match redo_response {
            CoreToUi::ApplyPatch { patch, .. } => patch,
            other => panic!("expected apply patch response, got {:?}", other),
        };
        let after_redo = get_content(&glyph, view_id).await;
        assert_eq!(redo_patch.apply(&before_redo).unwrap(), after_redo);
        assert_eq!(after_redo, "abc");
    }

    #[tokio::test]
    async fn test_noop_backspace_returns_empty_patch() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;

        let response = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Backspace,
            })
            .await;

        match response {
            CoreToUi::ApplyPatch { patch, .. } => assert!(patch.ops().is_empty()),
            other => panic!("expected apply patch response, got {:?}", other),
        }
    }

    #[test]
    fn test_build_completion_context_clamps_to_utf8_boundary() {
        let content = "abc🙂def";
        let (cursor, context) = Glyph::build_completion_context(content, 5);

        assert_eq!(cursor, 3);
        assert!(context.contains("<before>\nabc\n</before>"));
        assert!(context.contains("<after>\n🙂def\n</after>"));
    }

    #[test]
    fn test_sanitize_ghost_text_strips_markdown_fence() {
        let raw = "```rust\nlet value = 42;\n```";
        let sanitized = Glyph::sanitize_ghost_text(raw);
        assert_eq!(sanitized, "let value = 42;");
    }

    #[test]
    fn test_sanitize_ghost_text_truncates_utf8_safely() {
        let raw = "🙂".repeat(300);
        let sanitized = Glyph::sanitize_ghost_text(&raw);
        assert!(sanitized.len() <= Glyph::COMPLETION_MAX_BYTES);
        assert!(sanitized.is_char_boundary(sanitized.len()));
    }

    #[tokio::test]
    async fn test_non_mutating_move_keeps_cursor_event() {
        let glyph = Glyph::new();
        let view_id = create_view(&glyph).await;
        let _ = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Insert {
                    text: "ab".to_string(),
                },
            })
            .await;

        let response = glyph
            .handle_message(UiToCore::Input {
                view_id,
                event: InputEvent::Move(Movement::Left),
            })
            .await;

        assert!(matches!(
            response,
            CoreToUi::Event(CoreEvent::CursorMoved { .. })
        ));
    }

    #[tokio::test]
    async fn test_request_completion_unknown_view_returns_error() {
        let glyph = Glyph::new();
        let response = glyph
            .handle_message(UiToCore::RequestCompletion {
                view_id: ViewId(999),
                position: 0,
            })
            .await;

        assert!(matches!(response, CoreToUi::Error { .. }));
    }

    #[tokio::test]
    async fn test_new_view_with_missing_file_returns_error() {
        let glyph = Glyph::new();
        let response = glyph
            .handle_message(UiToCore::NewView {
                path: Some("/this/path/does/not/exist.txt".to_string()),
            })
            .await;

        assert!(matches!(response, CoreToUi::Error { .. }));
    }
}
