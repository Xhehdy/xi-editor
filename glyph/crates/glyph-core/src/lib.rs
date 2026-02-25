//! Glyph Core - Main coordinator for the Glyph IDE
//!
//! This is the brain that ties everything together.

pub mod editor;

use crate::editor::EditorCore;
use glyph_buffer::{Buffer, BufferId};
use glyph_ce::{ComprehensionEngine, SymbolKind};
use glyph_events::{CoreEvent, InputEvent, ViewId};
use glyph_graph::{CodeGraph, Edge, EdgeKind, Node, NodeId, NodeKind};
use glyph_llm::LlmClient;
use glyph_patch::Patch;
use glyph_protocol::{
    CoreToUi, ErrorCode, GraphContextItem, GraphEdgeInfo, GraphNodeInfo, IndexQueueStatsInfo,
    SymbolInfo, UiToCore,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use tree_sitter::{Node as TsNode, Parser};

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
    /// Debounced background indexing queue state
    index_queue: Arc<Mutex<IndexQueueState>>,
    /// LLM Client
    llm: Arc<LlmClient>,
    /// Next available ID
    next_id: AtomicU64,
    /// Core version
    pub version: String,
}

#[derive(Debug, Clone)]
struct IndexedGraphSymbol {
    name: String,
    kind: SymbolKind,
    line: usize,
    signature: Option<String>,
}

#[derive(Debug, Default)]
struct SemanticExtraction {
    imports: Vec<String>,
    implements: Vec<(String, String)>,
    calls: Vec<(String, String)>,
    functions: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct IndexQueueConfig {
    debounce_ms: u64,
    retry_backoff_ms: u64,
    max_retries: u8,
    max_pending: usize,
}

struct IndexTaskEntry {
    generation: u64,
    handle: tokio::task::JoinHandle<()>,
}

struct IndexQueueState {
    tasks: HashMap<String, IndexTaskEntry>,
    next_generation: u64,
    in_progress: usize,
    completed: u64,
    failed: u64,
    retried: u64,
    dropped: u64,
    last_error: Option<String>,
}

impl IndexQueueState {
    fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            next_generation: 0,
            in_progress: 0,
            completed: 0,
            failed: 0,
            retried: 0,
            dropped: 0,
            last_error: None,
        }
    }
}

impl Glyph {
    const COMPLETION_PREFIX_BYTES: usize = 2048;
    const COMPLETION_SUFFIX_BYTES: usize = 512;
    const COMPLETION_MAX_BYTES: usize = 512;
    const COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);
    const GRAPH_SEARCH_DEFAULT_LIMIT: usize = 100;
    const GRAPH_SEARCH_MAX_LIMIT: usize = 1000;
    const GRAPH_CONTEXT_DEFAULT_LIMIT: usize = 12;
    const GRAPH_CONTEXT_MAX_LIMIT: usize = 64;
    const CKG_EDGE_CONFIDENCE_CONTAINS: f64 = 0.92;
    const CKG_EDGE_CONFIDENCE_AST: f64 = 0.97;
    const CKG_EDGE_CONFIDENCE_HEURISTIC: f64 = 0.68;
    const CKG_EXTRACTOR_VERSION: &'static str = concat!("glyph-core@", env!("CARGO_PKG_VERSION"));
    const INDEX_QUEUE_DEBOUNCE_MS: u64 = 250;
    const INDEX_QUEUE_RETRY_BACKOFF_MS: u64 = 150;
    const INDEX_QUEUE_MAX_RETRIES: u8 = 2;
    const INDEX_QUEUE_MAX_PENDING: usize = 256;

    /// Creates a new Glyph instance
    pub fn new() -> Self {
        Self::new_with_graph_path(None)
    }

    fn new_with_graph_path(path_override: Option<&Path>) -> Self {
        let graph = Self::init_graph(path_override);

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
            index_queue: Arc::new(Mutex::new(IndexQueueState::new())),
            llm: Arc::new(llm),
            next_id: AtomicU64::new(1),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    fn resolve_graph_path(path_override: Option<&Path>) -> PathBuf {
        if let Some(path) = path_override {
            return path.to_path_buf();
        }

        if let Ok(path) = std::env::var("GLYPH_CKG_PATH") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }

        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".glyph")
            .join("ckg.sqlite")
    }

    fn init_graph(path_override: Option<&Path>) -> CodeGraph {
        let graph_path = Self::resolve_graph_path(path_override);

        if let Some(parent) = graph_path.parent() {
            if parent.as_os_str().is_empty() {
                return match CodeGraph::open(&graph_path) {
                    Ok(graph) => graph,
                    Err(err) => {
                        warn!(
                            "Failed to open CKG at '{}': {}; falling back to in-memory graph",
                            graph_path.display(),
                            err
                        );
                        CodeGraph::in_memory().expect("Failed to create in-memory code graph")
                    }
                };
            }
            if let Err(err) = std::fs::create_dir_all(parent) {
                warn!(
                    "Failed to create CKG directory '{}': {}; falling back to in-memory graph",
                    parent.display(),
                    err
                );
                return CodeGraph::in_memory().expect("Failed to create in-memory code graph");
            }
        }

        match CodeGraph::open(&graph_path) {
            Ok(graph) => graph,
            Err(err) => {
                warn!(
                    "Failed to open CKG at '{}': {}; falling back to in-memory graph",
                    graph_path.display(),
                    err
                );
                CodeGraph::in_memory().expect("Failed to create in-memory code graph")
            }
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

    fn stable_hash_64(namespace: &str, key: &str) -> u64 {
        // FNV-1a 64-bit hash for deterministic, process-independent IDs.
        let mut hash = 0xcbf29ce484222325u64;
        for byte in namespace
            .as_bytes()
            .iter()
            .chain(std::iter::once(&b'|'))
            .chain(key.as_bytes().iter())
        {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn stable_node_id(namespace: &str, key: &str) -> NodeId {
        let raw = Self::stable_hash_64(namespace, key);
        // Keep IDs positive and non-zero for compatibility with SQL PK usage.
        let positive = (raw & 0x7fff_ffff_ffff_ffff) as i64;
        NodeId(if positive == 0 { 1 } else { positive })
    }

    fn stable_file_node_id(path: &str) -> NodeId {
        Self::stable_node_id("file", path)
    }

    fn stable_symbol_node_id(path: &str, symbol: &IndexedGraphSymbol) -> NodeId {
        let signature = symbol.signature.as_deref().unwrap_or("");
        let key = format!(
            "{}|{}|{}|{}|{}",
            path,
            symbol.kind.as_str(),
            symbol.line,
            symbol.name,
            signature
        );
        Self::stable_node_id("symbol", &key)
    }

    fn graph_node_kind_from_symbol(kind: SymbolKind) -> NodeKind {
        match kind {
            SymbolKind::Function => NodeKind::Function,
            SymbolKind::Struct => NodeKind::Struct,
            SymbolKind::Enum => NodeKind::Enum,
            SymbolKind::Trait => NodeKind::Trait,
            SymbolKind::Module => NodeKind::Module,
            SymbolKind::Constant => NodeKind::Constant,
            SymbolKind::Type => NodeKind::Type,
            SymbolKind::Variable | SymbolKind::Macro | SymbolKind::Import => NodeKind::Concept,
        }
    }

    fn graph_node_to_info(node: &Node) -> GraphNodeInfo {
        GraphNodeInfo {
            id: node.id.0,
            kind: node.kind.as_str().to_string(),
            name: node.name.clone(),
            file: node.file.clone(),
        }
    }

    fn sort_graph_node_infos(nodes: &mut [GraphNodeInfo]) {
        nodes.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.file.cmp(&right.file))
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    fn graph_edge_to_info(edge: &Edge) -> GraphEdgeInfo {
        GraphEdgeInfo {
            from_id: edge.from.0,
            to_id: edge.to.0,
            kind: edge.kind.as_str().to_string(),
        }
    }

    fn sort_graph_edge_infos(edges: &mut [GraphEdgeInfo]) {
        edges.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.from_id.cmp(&right.from_id))
                .then_with(|| left.to_id.cmp(&right.to_id))
        });
    }

    fn collect_indexed_symbols(ce: &ComprehensionEngine, path: &Path) -> Vec<IndexedGraphSymbol> {
        ce.symbols_in_file(path)
            .into_iter()
            .map(|symbol| IndexedGraphSymbol {
                name: symbol.name.clone(),
                kind: symbol.kind,
                line: symbol.start_line,
                signature: symbol.signature.clone(),
            })
            .collect()
    }

    fn is_ident_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_'
    }

    fn clean_identifier(candidate: &str) -> Option<String> {
        let mut trimmed = candidate.trim();
        if let Some(prefix) = trimmed.strip_prefix("mut ") {
            trimmed = prefix.trim();
        }
        if let Some(prefix) = trimmed.strip_prefix('&') {
            trimmed = prefix.trim_start_matches("mut ").trim();
        }
        if let Some((head, _)) = trimmed.split_once('<') {
            trimmed = head;
        }
        if let Some((head, _)) = trimmed.split_once(':') {
            trimmed = head;
        }
        if let Some((head, _)) = trimmed.split_once('(') {
            trimmed = head;
        }

        let ident: String = trimmed
            .chars()
            .take_while(|ch| Self::is_ident_char(*ch))
            .collect();
        if ident.is_empty() {
            None
        } else {
            Some(ident)
        }
    }

    fn extract_path_tail_name(expr: &str) -> Option<String> {
        let mut cleaned = expr.trim();
        cleaned = cleaned.split('{').next().unwrap_or(cleaned).trim();
        cleaned = cleaned.split(" where ").next().unwrap_or(cleaned).trim();
        cleaned = cleaned.trim_end_matches(',');

        if cleaned.starts_with('<') {
            let mut depth = 0usize;
            let mut byte_after = None;
            for (idx, ch) in cleaned.char_indices() {
                match ch {
                    '<' => depth += 1,
                    '>' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        if depth == 0 {
                            byte_after = Some(idx + ch.len_utf8());
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(byte_after) = byte_after {
                cleaned = cleaned[byte_after..].trim_start();
            }
        }

        while let Some(rest) = cleaned.strip_prefix('&') {
            cleaned = rest.trim_start();
        }
        if let Some(rest) = cleaned.strip_prefix("mut ") {
            cleaned = rest.trim_start();
        }
        if let Some(rest) = cleaned.strip_prefix("dyn ") {
            cleaned = rest.trim_start();
        }

        let token = cleaned
            .split('+')
            .next()
            .unwrap_or(cleaned)
            .split_whitespace()
            .last()
            .unwrap_or(cleaned)
            .trim()
            .split('<')
            .next()
            .unwrap_or(cleaned)
            .trim()
            .split("::")
            .last()
            .unwrap_or(cleaned)
            .trim();

        Self::clean_identifier(token)
    }

    fn node_text<'a>(content: &'a str, node: TsNode<'_>) -> Option<&'a str> {
        content.get(node.byte_range())
    }

    fn rust_function_name(node: TsNode<'_>, content: &str) -> Option<String> {
        if node.kind() != "function_item" {
            return None;
        }

        if let Some(name_node) = node.child_by_field_name("name") {
            return Self::node_text(content, name_node).and_then(Self::clean_identifier);
        }

        let ident_node = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|child| child.kind() == "identifier");
            found
        };
        ident_node
            .and_then(|child| Self::node_text(content, child))
            .and_then(Self::clean_identifier)
    }

    fn rust_callee_name_from_node(node: TsNode<'_>, content: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" | "type_identifier" => {
                Self::node_text(content, node).and_then(Self::clean_identifier)
            }
            "scoped_identifier" | "scoped_type_identifier" => Self::node_text(content, node)
                .and_then(|text| {
                    text.split("::")
                        .last()
                        .and_then(Self::clean_identifier)
                        .or_else(|| Self::clean_identifier(text))
                }),
            "generic_function" => node
                .child_by_field_name("function")
                .or_else(|| node.named_child(0))
                .and_then(|inner| Self::rust_callee_name_from_node(inner, content)),
            "field_expression" => {
                let fallback_field = {
                    let mut cursor = node.walk();
                    let found = node
                        .children(&mut cursor)
                        .find(|child| child.kind() == "field_identifier");
                    found
                };
                node.child_by_field_name("field")
                    .or(fallback_field)
                    .and_then(|field| Self::node_text(content, field))
                    .and_then(Self::clean_identifier)
            }
            _ => Self::node_text(content, node).and_then(Self::extract_path_tail_name),
        }
    }

    fn enclosing_rust_function_name(node: TsNode<'_>, content: &str) -> Option<String> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == "function_item" {
                return Self::rust_function_name(parent, content);
            }
            current = parent.parent();
        }
        None
    }

    fn extract_rust_ast_semantics(content: &str) -> Option<SemanticExtraction> {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(content, None)?;
        let root = tree.root_node();

        let mut imports = Vec::new();
        let mut imports_seen: HashSet<String> = HashSet::new();
        let mut implements = Vec::new();
        let mut implements_seen: HashSet<(String, String)> = HashSet::new();
        let mut calls = Vec::new();
        let mut calls_seen: HashSet<(String, String)> = HashSet::new();
        let mut functions = Vec::new();
        let mut functions_seen: HashSet<String> = HashSet::new();

        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "use_declaration" => {
                    if let Some(text) = Self::node_text(content, node) {
                        let trimmed = text.trim();
                        if let Some(rest) = trimmed.strip_prefix("use ") {
                            let target = rest.trim_end_matches(';').trim().to_string();
                            if !target.is_empty() && imports_seen.insert(target.clone()) {
                                imports.push(target);
                            }
                        }
                    }
                }
                "impl_item" => {
                    if let Some(text) = Self::node_text(content, node) {
                        let header = text.split('{').next().unwrap_or(text).trim();
                        let header = header.strip_prefix("unsafe ").unwrap_or(header).trim();
                        if let Some(after_impl) = header.strip_prefix("impl ") {
                            if let Some((trait_part, type_part)) = after_impl.split_once(" for ") {
                                if let (Some(trait_name), Some(type_name)) = (
                                    Self::extract_path_tail_name(trait_part),
                                    Self::extract_path_tail_name(type_part),
                                ) {
                                    let relation = (type_name, trait_name);
                                    if implements_seen.insert(relation.clone()) {
                                        implements.push(relation);
                                    }
                                }
                            }
                        }
                    }
                }
                "function_item" => {
                    if let Some(name) = Self::rust_function_name(node, content) {
                        if functions_seen.insert(name.clone()) {
                            functions.push(name);
                        }
                    }
                }
                "call_expression" => {
                    let caller = Self::enclosing_rust_function_name(node, content);
                    let callee = node
                        .child_by_field_name("function")
                        .or_else(|| node.named_child(0))
                        .and_then(|function_node| {
                            Self::rust_callee_name_from_node(function_node, content)
                        });
                    if let (Some(caller), Some(callee)) = (caller, callee) {
                        let relation = (caller, callee);
                        if calls_seen.insert(relation.clone()) {
                            calls.push(relation);
                        }
                    }
                }
                "method_call_expression" => {
                    let caller = Self::enclosing_rust_function_name(node, content);
                    let fallback_method = {
                        let mut cursor = node.walk();
                        let found = node.children(&mut cursor).find(|child| {
                            child.kind() == "field_identifier" || child.kind() == "identifier"
                        });
                        found
                    };
                    let callee = node
                        .child_by_field_name("method")
                        .or_else(|| node.child_by_field_name("name"))
                        .or(fallback_method)
                        .and_then(|name_node| Self::node_text(content, name_node))
                        .and_then(Self::clean_identifier);
                    if let (Some(caller), Some(callee)) = (caller, callee) {
                        let relation = (caller, callee);
                        if calls_seen.insert(relation.clone()) {
                            calls.push(relation);
                        }
                    }
                }
                _ => {}
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }

        Some(SemanticExtraction {
            imports,
            implements,
            calls,
            functions,
        })
    }

    fn string_literal_contents(raw: &str) -> Option<String> {
        let trimmed = raw.trim().trim_end_matches(';').trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                trimmed
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .or_else(|| {
                trimmed
                    .strip_prefix('`')
                    .and_then(|value| value.strip_suffix('`'))
            });
        let value = unquoted.unwrap_or(trimmed).trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    fn javascript_variable_function_name(node: TsNode<'_>, content: &str) -> Option<String> {
        if node.kind() != "variable_declarator" {
            return None;
        }
        let value = node.child_by_field_name("value")?;
        if !matches!(value.kind(), "arrow_function" | "function_expression") {
            return None;
        }
        node.child_by_field_name("name")
            .and_then(|name_node| Self::node_text(content, name_node))
            .and_then(Self::clean_identifier)
    }

    fn javascript_function_name(node: TsNode<'_>, content: &str) -> Option<String> {
        match node.kind() {
            "function_declaration" | "method_definition" => node
                .child_by_field_name("name")
                .and_then(|name_node| Self::node_text(content, name_node))
                .and_then(Self::clean_identifier),
            "function_expression" | "arrow_function" => {
                let parent = node.parent()?;
                if let Some(name) = Self::javascript_variable_function_name(parent, content) {
                    return Some(name);
                }
                if parent.kind() == "assignment_expression" {
                    return parent
                        .child_by_field_name("left")
                        .and_then(|left| Self::javascript_callee_name_from_node(left, content));
                }
                None
            }
            "variable_declarator" => Self::javascript_variable_function_name(node, content),
            _ => None,
        }
    }

    fn javascript_callee_name_from_node(node: TsNode<'_>, content: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "property_identifier" => {
                Self::node_text(content, node).and_then(Self::clean_identifier)
            }
            "private_property_identifier" => Self::node_text(content, node)
                .and_then(|text| Self::clean_identifier(text.trim_start_matches('#'))),
            "member_expression" => node
                .child_by_field_name("property")
                .and_then(|property| Self::javascript_callee_name_from_node(property, content)),
            "subscript_expression" => node
                .child_by_field_name("object")
                .and_then(|object| Self::javascript_callee_name_from_node(object, content)),
            "call_expression" => node
                .child_by_field_name("function")
                .and_then(|function| Self::javascript_callee_name_from_node(function, content)),
            "new_expression" => node
                .child_by_field_name("constructor")
                .and_then(|constructor| {
                    Self::javascript_callee_name_from_node(constructor, content)
                }),
            "parenthesized_expression" => node
                .named_child(0)
                .and_then(|inner| Self::javascript_callee_name_from_node(inner, content)),
            _ => Self::node_text(content, node).and_then(Self::extract_path_tail_name),
        }
    }

    fn enclosing_javascript_function_name(node: TsNode<'_>, content: &str) -> Option<String> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if let Some(name) = Self::javascript_function_name(parent, content) {
                return Some(name);
            }
            current = parent.parent();
        }
        None
    }

    fn extract_javascript_ast_semantics(content: &str) -> Option<SemanticExtraction> {
        let mut parser = Parser::new();
        let language = tree_sitter_javascript::LANGUAGE.into();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(content, None)?;
        let root = tree.root_node();

        let mut imports = Vec::new();
        let mut imports_seen: HashSet<String> = HashSet::new();
        let mut implements = Vec::new();
        let mut implements_seen: HashSet<(String, String)> = HashSet::new();
        let mut calls = Vec::new();
        let mut calls_seen: HashSet<(String, String)> = HashSet::new();
        let mut functions = Vec::new();
        let mut functions_seen: HashSet<String> = HashSet::new();

        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "import_statement" => {
                    let target = node
                        .child_by_field_name("source")
                        .and_then(|source| Self::node_text(content, source))
                        .and_then(Self::string_literal_contents);
                    if let Some(target) = target {
                        if imports_seen.insert(target.clone()) {
                            imports.push(target);
                        }
                    }
                }
                "class_declaration" => {
                    let class_name = node
                        .child_by_field_name("name")
                        .and_then(|name_node| Self::node_text(content, name_node))
                        .and_then(Self::clean_identifier);
                    if let Some(class_name) = class_name {
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.kind() != "class_heritage" {
                                continue;
                            }
                            let heritage_name = child.named_child(0).and_then(|base| {
                                Self::javascript_callee_name_from_node(base, content)
                            });
                            if let Some(heritage_name) = heritage_name {
                                let relation = (class_name.clone(), heritage_name);
                                if implements_seen.insert(relation.clone()) {
                                    implements.push(relation);
                                }
                            }
                        }
                    }
                }
                "function_declaration" | "method_definition" | "variable_declarator" => {
                    if let Some(name) = Self::javascript_function_name(node, content) {
                        if functions_seen.insert(name.clone()) {
                            functions.push(name);
                        }
                    }
                }
                "call_expression" => {
                    let caller = Self::enclosing_javascript_function_name(node, content);
                    let callee = node
                        .child_by_field_name("function")
                        .and_then(|function_node| {
                            Self::javascript_callee_name_from_node(function_node, content)
                        });
                    if let (Some(caller), Some(callee)) = (caller, callee) {
                        let relation = (caller, callee);
                        if calls_seen.insert(relation.clone()) {
                            calls.push(relation);
                        }
                    }
                }
                _ => {}
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }

        Some(SemanticExtraction {
            imports,
            implements,
            calls,
            functions,
        })
    }

    fn python_name_from_import_node(node: TsNode<'_>, content: &str) -> Option<String> {
        match node.kind() {
            "dotted_name" | "relative_import" => {
                let value = Self::node_text(content, node)?.trim();
                if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            }
            "aliased_import" => node
                .child_by_field_name("name")
                .and_then(|name_node| Self::python_name_from_import_node(name_node, content)),
            _ => None,
        }
    }

    fn python_callee_name_from_node(node: TsNode<'_>, content: &str) -> Option<String> {
        match node.kind() {
            "identifier" => Self::node_text(content, node).and_then(Self::clean_identifier),
            "attribute" => node
                .child_by_field_name("attribute")
                .and_then(|attr| Self::node_text(content, attr))
                .and_then(Self::clean_identifier),
            "dotted_name" => Self::node_text(content, node).and_then(|text| {
                text.rsplit('.')
                    .next()
                    .and_then(Self::clean_identifier)
                    .or_else(|| Self::clean_identifier(text))
            }),
            "subscript" => node
                .child_by_field_name("value")
                .and_then(|value| Self::python_callee_name_from_node(value, content)),
            "call" => node
                .child_by_field_name("function")
                .and_then(|function_node| {
                    Self::python_callee_name_from_node(function_node, content)
                }),
            "parenthesized_expression" => node
                .named_child(0)
                .and_then(|inner| Self::python_callee_name_from_node(inner, content)),
            _ => Self::node_text(content, node).and_then(Self::extract_path_tail_name),
        }
    }

    fn enclosing_python_function_name(node: TsNode<'_>, content: &str) -> Option<String> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == "function_definition" {
                return parent
                    .child_by_field_name("name")
                    .and_then(|name_node| Self::node_text(content, name_node))
                    .and_then(Self::clean_identifier);
            }
            current = parent.parent();
        }
        None
    }

    fn extract_python_ast_semantics(content: &str) -> Option<SemanticExtraction> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE.into();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(content, None)?;
        let root = tree.root_node();

        let mut imports = Vec::new();
        let mut imports_seen: HashSet<String> = HashSet::new();
        let mut implements = Vec::new();
        let mut implements_seen: HashSet<(String, String)> = HashSet::new();
        let mut calls = Vec::new();
        let mut calls_seen: HashSet<(String, String)> = HashSet::new();
        let mut functions = Vec::new();
        let mut functions_seen: HashSet<String> = HashSet::new();

        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "import_statement" => {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if !child.is_named() {
                            continue;
                        }
                        if let Some(target) = Self::python_name_from_import_node(child, content) {
                            if imports_seen.insert(target.clone()) {
                                imports.push(target);
                            }
                        }
                    }
                }
                "import_from_statement" => {
                    let target = node
                        .child_by_field_name("module_name")
                        .and_then(|module| Self::python_name_from_import_node(module, content));
                    if let Some(target) = target {
                        if imports_seen.insert(target.clone()) {
                            imports.push(target);
                        }
                    }
                }
                "class_definition" => {
                    let class_name = node
                        .child_by_field_name("name")
                        .and_then(|name_node| Self::node_text(content, name_node))
                        .and_then(Self::clean_identifier);
                    if let Some(class_name) = class_name {
                        if let Some(superclasses) = node.child_by_field_name("superclasses") {
                            let mut cursor = superclasses.walk();
                            for child in superclasses.children(&mut cursor) {
                                if !child.is_named() {
                                    continue;
                                }
                                if let Some(base_name) =
                                    Self::python_callee_name_from_node(child, content)
                                {
                                    let relation = (class_name.clone(), base_name);
                                    if implements_seen.insert(relation.clone()) {
                                        implements.push(relation);
                                    }
                                }
                            }
                        }
                    }
                }
                "function_definition" => {
                    let name = node
                        .child_by_field_name("name")
                        .and_then(|name_node| Self::node_text(content, name_node))
                        .and_then(Self::clean_identifier);
                    if let Some(name) = name {
                        if functions_seen.insert(name.clone()) {
                            functions.push(name);
                        }
                    }
                }
                "call" => {
                    let caller = Self::enclosing_python_function_name(node, content);
                    let callee = node
                        .child_by_field_name("function")
                        .and_then(|function_node| {
                            Self::python_callee_name_from_node(function_node, content)
                        });
                    if let (Some(caller), Some(callee)) = (caller, callee) {
                        let relation = (caller, callee);
                        if calls_seen.insert(relation.clone()) {
                            calls.push(relation);
                        }
                    }
                }
                _ => {}
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }

        Some(SemanticExtraction {
            imports,
            implements,
            calls,
            functions,
        })
    }

    fn extract_import_targets(content: &str) -> Vec<String> {
        let mut targets = Vec::new();
        let mut seen = HashSet::new();

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                continue;
            }

            let candidate = if let Some(rest) = line.strip_prefix("use ") {
                Some(rest.trim_end_matches(';').trim().to_string())
            } else if line.starts_with("import ") {
                if let Some((_, from_part)) = line.rsplit_once(" from ") {
                    let value = from_part
                        .trim()
                        .trim_end_matches(';')
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    Some(value)
                } else {
                    let rest = line
                        .strip_prefix("import ")
                        .unwrap_or_default()
                        .trim_end_matches(';')
                        .trim()
                        .to_string();
                    if rest.is_empty() {
                        None
                    } else {
                        Some(rest)
                    }
                }
            } else if let Some(rest) = line.strip_prefix("from ") {
                let module = rest
                    .split(" import ")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if module.is_empty() {
                    None
                } else {
                    Some(module)
                }
            } else if let Some(rest) = line.strip_prefix("#include ") {
                let header = rest
                    .trim()
                    .trim_matches('<')
                    .trim_matches('>')
                    .trim_matches('"')
                    .to_string();
                if header.is_empty() {
                    None
                } else {
                    Some(header)
                }
            } else {
                None
            };

            if let Some(target) = candidate {
                if seen.insert(target.clone()) {
                    targets.push(target);
                }
            }
        }

        targets
    }

    fn extract_implements_relations(content: &str) -> Vec<(String, String)> {
        let mut relations = Vec::new();
        let mut seen = HashSet::new();

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            if let Some(after_impl) = line.strip_prefix("impl ") {
                if let Some((trait_part, type_part)) = after_impl.split_once(" for ") {
                    if let (Some(trait_name), Some(type_name)) = (
                        Self::clean_identifier(trait_part),
                        Self::clean_identifier(type_part),
                    ) {
                        let relation = (type_name, trait_name);
                        if seen.insert(relation.clone()) {
                            relations.push(relation);
                        }
                    }
                }
                continue;
            }

            if let Some(after_class) = line.strip_prefix("class ") {
                if let Some((class_decl, impl_part)) = after_class.split_once(" implements ") {
                    let class_name = Self::clean_identifier(class_decl);
                    let impl_list = impl_part
                        .split('{')
                        .next()
                        .unwrap_or_default()
                        .split(',')
                        .filter_map(Self::clean_identifier);
                    if let Some(class_name) = class_name {
                        for trait_name in impl_list {
                            let relation = (class_name.clone(), trait_name);
                            if seen.insert(relation.clone()) {
                                relations.push(relation);
                            }
                        }
                    }
                }
            }
        }

        relations
    }

    fn extract_rust_function_bodies(content: &str) -> Vec<(String, String)> {
        let mut functions = Vec::new();
        let mut search_from = 0usize;

        while let Some(relative_idx) = content[search_from..].find("fn ") {
            let fn_idx = search_from + relative_idx;
            if fn_idx > 0 {
                let before = content[..fn_idx].chars().next_back().unwrap_or(' ');
                if Self::is_ident_char(before) {
                    search_from = fn_idx + 3;
                    continue;
                }
            }

            let mut cursor = fn_idx + 3;
            while cursor < content.len()
                && content[cursor..]
                    .chars()
                    .next()
                    .map(|ch| ch.is_whitespace())
                    .unwrap_or(false)
            {
                cursor += content[cursor..]
                    .chars()
                    .next()
                    .map(|ch| ch.len_utf8())
                    .unwrap_or(1);
            }

            let mut name = String::new();
            let mut name_cursor = cursor;
            while name_cursor < content.len() {
                let ch = content[name_cursor..].chars().next().unwrap_or('\0');
                if !Self::is_ident_char(ch) {
                    break;
                }
                name.push(ch);
                name_cursor += ch.len_utf8();
            }
            if name.is_empty() {
                search_from = fn_idx + 3;
                continue;
            }

            let after_name = &content[name_cursor..];
            let Some(brace_rel_idx) = after_name.find('{') else {
                search_from = name_cursor;
                continue;
            };
            let brace_idx = name_cursor + brace_rel_idx;

            let mut depth = 0usize;
            let mut end_idx = None;
            for (offset, ch) in content[brace_idx..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        if depth == 0 {
                            end_idx = Some(brace_idx + offset);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(end_idx) = end_idx {
                let body = content[(brace_idx + 1)..end_idx].to_string();
                functions.push((name, body));
                search_from = end_idx + 1;
            } else {
                search_from = brace_idx + 1;
            }
        }

        functions
    }

    fn body_contains_call(body: &str, callee: &str) -> bool {
        if callee.is_empty() {
            return false;
        }
        let pattern = format!("{}(", callee);
        let mut search_from = 0usize;
        while let Some(relative_idx) = body[search_from..].find(&pattern) {
            let idx = search_from + relative_idx;
            let before_ok = if idx == 0 {
                true
            } else {
                let prev = body[..idx].chars().next_back().unwrap_or(' ');
                !Self::is_ident_char(prev)
            };
            if before_ok {
                return true;
            }
            search_from = idx + pattern.len();
        }
        false
    }

    fn upsert_external_node(
        graph: &CodeGraph,
        id: NodeId,
        kind: NodeKind,
        name: &str,
        source: &str,
    ) -> Result<(), String> {
        let node = Node {
            id,
            kind,
            name: name.to_string(),
            file: None,
            data: serde_json::json!({ "source": source }),
        };
        graph
            .add_node(&node)
            .map_err(|err| format!("failed upserting external node '{}': {}", name, err))
            .map(|_| ())
    }

    fn add_edge_dedup(
        graph: &CodeGraph,
        seen: &mut HashSet<(NodeId, NodeId, EdgeKind)>,
        edge: Edge,
    ) -> Result<(), String> {
        let key = (edge.from, edge.to, edge.kind);
        if !seen.insert(key) {
            return Ok(());
        }
        graph.add_edge(&edge).map_err(|err| {
            format!(
                "failed inserting edge {:?}->{:?}: {}",
                edge.from, edge.to, err
            )
        })
    }

    fn graph_language_label(path: &str) -> &'static str {
        let ext = Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        match Language::from_extension(ext) {
            Language::Rust => "rust",
            Language::JavaScript => "javascript",
            Language::Python => "python",
            Language::Json => "json",
            Language::Plain => "plain",
        }
    }

    fn workspace_root() -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    fn normalize_path_lexical(path: &Path) -> PathBuf {
        use std::path::Component;

        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normalized.pop() {
                        normalized.push(component.as_os_str());
                    }
                }
                _ => normalized.push(component.as_os_str()),
            }
        }
        normalized
    }

    fn add_existing_file_candidate(paths: &mut BTreeSet<String>, candidate: &Path) {
        if !candidate.is_file() {
            return;
        }
        let normalized = Self::normalize_path_lexical(candidate);
        if let Some(path) = normalized.to_str() {
            paths.insert(path.to_string());
        }
    }

    fn sanitize_import_target(import_target: &str) -> String {
        let trimmed = import_target.trim();
        let without_alias = trimmed.split(" as ").next().unwrap_or(trimmed).trim();
        let without_group = without_alias
            .split('{')
            .next()
            .unwrap_or(without_alias)
            .trim();
        let without_semicolon = without_group.trim_end_matches(';').trim();
        let without_query = without_semicolon
            .split('?')
            .next()
            .unwrap_or(without_semicolon)
            .trim();
        let without_fragment = without_query
            .split('#')
            .next()
            .unwrap_or(without_query)
            .trim();
        without_fragment
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string()
    }

    fn find_crate_root(source_file: &Path) -> Option<PathBuf> {
        let mut current = source_file.parent();
        while let Some(dir) = current {
            if dir.join("Cargo.toml").is_file() {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
        None
    }

    fn resolve_javascript_import_files(
        source_file: &Path,
        import_target: &str,
    ) -> BTreeSet<String> {
        let mut resolved = BTreeSet::new();
        let target = Self::sanitize_import_target(import_target);
        if target.is_empty() {
            return resolved;
        }
        if !(target.starts_with("./") || target.starts_with("../") || target.starts_with('/')) {
            return resolved;
        }

        let source_dir = source_file.parent().unwrap_or_else(|| Path::new("."));
        let base = if target.starts_with('/') {
            PathBuf::from(&target)
        } else {
            source_dir.join(&target)
        };

        Self::add_existing_file_candidate(&mut resolved, &base);
        if base.extension().is_none() {
            let exts = ["js", "jsx", "ts", "tsx", "mjs", "cjs"];
            for ext in exts {
                Self::add_existing_file_candidate(&mut resolved, &base.with_extension(ext));
                Self::add_existing_file_candidate(
                    &mut resolved,
                    &base.join(format!("index.{}", ext)),
                );
            }
        }

        resolved
    }

    fn resolve_python_import_files(source_file: &Path, import_target: &str) -> BTreeSet<String> {
        let mut resolved = BTreeSet::new();
        let target = Self::sanitize_import_target(import_target);
        if target.is_empty() {
            return resolved;
        }

        let source_dir = source_file.parent().unwrap_or_else(|| Path::new("."));
        let leading_dots = target.chars().take_while(|ch| *ch == '.').count();
        let module = target.trim_start_matches('.');
        let module_path = module.replace('.', "/");
        if module_path.is_empty() {
            return resolved;
        }

        let mut roots = Vec::new();
        if leading_dots > 0 {
            let mut root = source_dir.to_path_buf();
            for _ in 1..leading_dots {
                if let Some(parent) = root.parent() {
                    root = parent.to_path_buf();
                }
            }
            roots.push(root);
        } else {
            roots.push(source_dir.to_path_buf());
            roots.push(Self::workspace_root());
        }

        for root in roots {
            let base = root.join(&module_path);
            Self::add_existing_file_candidate(&mut resolved, &base.with_extension("py"));
            Self::add_existing_file_candidate(&mut resolved, &base.join("__init__.py"));
        }

        resolved
    }

    fn resolve_rust_import_files(source_file: &Path, import_target: &str) -> BTreeSet<String> {
        let mut resolved = BTreeSet::new();
        let target = Self::sanitize_import_target(import_target);
        if target.is_empty() {
            return resolved;
        }

        let source_dir = source_file.parent().unwrap_or_else(|| Path::new("."));
        let (root, path_tail) = if let Some(rest) = target.strip_prefix("crate::") {
            let crate_root =
                Self::find_crate_root(source_file).unwrap_or_else(Self::workspace_root);
            let src_root = crate_root.join("src");
            let root = if src_root.is_dir() {
                src_root
            } else {
                crate_root
            };
            (root, rest)
        } else if let Some(rest) = target.strip_prefix("self::") {
            (source_dir.to_path_buf(), rest)
        } else if let Some(rest) = target.strip_prefix("super::") {
            let parent = source_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| source_dir.to_path_buf());
            (parent, rest)
        } else {
            return resolved;
        };

        let segments: Vec<&str> = path_tail
            .split("::")
            .filter(|segment| !segment.trim().is_empty())
            .collect();
        if segments.is_empty() {
            return resolved;
        }

        let mut depths = vec![segments.len()];
        if segments.len() > 1 {
            depths.push(segments.len() - 1);
        }

        for depth in depths {
            let rel = segments[..depth]
                .iter()
                .fold(PathBuf::new(), |mut acc, segment| {
                    acc.push(segment);
                    acc
                });
            Self::add_existing_file_candidate(&mut resolved, &root.join(&rel).with_extension("rs"));
            Self::add_existing_file_candidate(&mut resolved, &root.join(&rel).join("mod.rs"));
        }

        resolved
    }

    fn resolve_imported_files(
        source_path: &str,
        import_target: &str,
        language: Language,
    ) -> Vec<String> {
        let source_file = Path::new(source_path);
        let resolved = match language {
            Language::JavaScript => {
                Self::resolve_javascript_import_files(source_file, import_target)
            }
            Language::Python => Self::resolve_python_import_files(source_file, import_target),
            Language::Rust => Self::resolve_rust_import_files(source_file, import_target),
            Language::Json | Language::Plain => BTreeSet::new(),
        };
        resolved.into_iter().collect()
    }

    fn resolved_import_file_ref_node_id(path: &str) -> NodeId {
        Self::stable_node_id("resolved-import-file", path)
    }

    fn upsert_resolved_import_file_node(
        graph: &CodeGraph,
        resolved_path: &str,
        source: &str,
    ) -> Result<NodeId, String> {
        let existing_file_node = graph
            .get_nodes_in_file(resolved_path)
            .map_err(|err| format!("failed loading resolved import file node: {}", err))?
            .into_iter()
            .find(|node| node.kind == NodeKind::File);
        if let Some(node) = existing_file_node {
            return Ok(node.id);
        }

        let id = Self::resolved_import_file_ref_node_id(resolved_path);
        let node = Node {
            id,
            kind: NodeKind::File,
            name: resolved_path.to_string(),
            file: None,
            data: serde_json::json!({
                "source": source,
                "resolved_path": resolved_path,
            }),
        };
        graph
            .add_node(&node)
            .map_err(|err| format!("failed upserting resolved import file node: {}", err))?;
        Ok(id)
    }

    fn find_global_function_targets(
        graph: &CodeGraph,
        function_name: &str,
        exclude_file: Option<&str>,
        max_targets: usize,
    ) -> Result<Vec<NodeId>, String> {
        if function_name.trim().is_empty() || max_targets == 0 {
            return Ok(Vec::new());
        }

        let mut nodes: Vec<Node> = graph
            .find_by_name(function_name)
            .map_err(|err| format!("failed searching global function targets: {}", err))?
            .into_iter()
            .filter(|node| node.kind == NodeKind::Function)
            .filter(|node| node.name == function_name)
            .filter(|node| {
                exclude_file
                    .map(|file| node.file.as_deref() != Some(file))
                    .unwrap_or(true)
            })
            .collect();
        Self::sort_nodes_for_search(&mut nodes);

        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        for node in nodes {
            if seen.insert(node.id) {
                ids.push(node.id);
                if ids.len() >= max_targets {
                    break;
                }
            }
        }
        Ok(ids)
    }

    fn current_unix_timestamp_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    }

    fn edge_with_provenance(
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        source: &str,
        language: &str,
        confidence: f64,
    ) -> Edge {
        Edge::new(from, to, kind).with_data(serde_json::json!({
            "source": source,
            "language": language,
            "timestamp_unix": Self::current_unix_timestamp_secs(),
            "confidence": confidence,
            "extractor_version": Self::CKG_EXTRACTOR_VERSION,
        }))
    }

    fn rebuild_file_graph(
        graph: &CodeGraph,
        path: &str,
        content: &str,
        symbols: &[IndexedGraphSymbol],
    ) -> Result<(), String> {
        graph
            .delete_file(path)
            .map_err(|err| format!("failed deleting stale graph data: {}", err))?;

        let file_node_id = Self::stable_file_node_id(path);
        let file_node = Node::new_file(file_node_id.0, path);
        graph
            .add_node(&file_node)
            .map_err(|err| format!("failed inserting file node: {}", err))?;

        let mut function_ids_by_name: HashMap<String, NodeId> = HashMap::new();
        let mut type_ids_by_name: HashMap<String, NodeId> = HashMap::new();
        let mut trait_ids_by_name: HashMap<String, NodeId> = HashMap::new();
        let mut seen_edges: HashSet<(NodeId, NodeId, EdgeKind)> = HashSet::new();
        let language = Self::graph_language_label(path);

        for symbol in symbols {
            let symbol_node_id = Self::stable_symbol_node_id(path, symbol);
            let node_kind = Self::graph_node_kind_from_symbol(symbol.kind);
            let symbol_node = Node {
                id: symbol_node_id,
                kind: node_kind,
                name: symbol.name.clone(),
                file: Some(path.to_string()),
                data: serde_json::json!({
                    "line": symbol.line,
                    "signature": symbol.signature,
                    "symbol_kind": symbol.kind.as_str(),
                }),
            };

            graph
                .add_node(&symbol_node)
                .map_err(|err| format!("failed inserting symbol node: {}", err))?;

            Self::add_edge_dedup(
                graph,
                &mut seen_edges,
                Self::edge_with_provenance(
                    file_node_id,
                    symbol_node_id,
                    EdgeKind::Contains,
                    "ce_contains",
                    language,
                    Self::CKG_EDGE_CONFIDENCE_CONTAINS,
                ),
            )?;

            match node_kind {
                NodeKind::Function => {
                    function_ids_by_name
                        .entry(symbol.name.clone())
                        .or_insert(symbol_node_id);
                }
                NodeKind::Struct | NodeKind::Enum | NodeKind::Type | NodeKind::Module => {
                    type_ids_by_name
                        .entry(symbol.name.clone())
                        .or_insert(symbol_node_id);
                }
                NodeKind::Trait => {
                    trait_ids_by_name
                        .entry(symbol.name.clone())
                        .or_insert(symbol_node_id);
                }
                _ => {}
            }
        }

        let language_kind = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Plain);
        let ast_semantics = match language_kind {
            Language::Rust => Self::extract_rust_ast_semantics(content),
            Language::JavaScript => Self::extract_javascript_ast_semantics(content),
            Language::Python => Self::extract_python_ast_semantics(content),
            Language::Json | Language::Plain => None,
        };

        let import_targets = ast_semantics
            .as_ref()
            .map(|semantic| semantic.imports.clone())
            .unwrap_or_else(|| Self::extract_import_targets(content));
        for import_target in import_targets {
            let source = if ast_semantics.is_some() {
                "ast_import"
            } else {
                "heuristic_import"
            };
            let confidence = if ast_semantics.is_some() {
                Self::CKG_EDGE_CONFIDENCE_AST
            } else {
                Self::CKG_EDGE_CONFIDENCE_HEURISTIC
            };
            let import_node_id = Self::stable_node_id("import-target", &import_target);
            Self::upsert_external_node(
                graph,
                import_node_id,
                NodeKind::Module,
                &import_target,
                source,
            )?;
            Self::add_edge_dedup(
                graph,
                &mut seen_edges,
                Self::edge_with_provenance(
                    file_node_id,
                    import_node_id,
                    EdgeKind::Imports,
                    source,
                    language,
                    confidence,
                ),
            )?;

            let resolved_import_paths =
                Self::resolve_imported_files(path, &import_target, language_kind);
            let resolved_source = if ast_semantics.is_some() {
                "ast_import_resolved"
            } else {
                "heuristic_import_resolved"
            };
            for resolved_path in resolved_import_paths {
                let resolved_file_node_id =
                    Self::upsert_resolved_import_file_node(graph, &resolved_path, resolved_source)?;
                Self::add_edge_dedup(
                    graph,
                    &mut seen_edges,
                    Self::edge_with_provenance(
                        file_node_id,
                        resolved_file_node_id,
                        EdgeKind::Imports,
                        resolved_source,
                        language,
                        confidence,
                    ),
                )?;
            }
        }

        let implement_relations = ast_semantics
            .as_ref()
            .map(|semantic| semantic.implements.clone())
            .unwrap_or_else(|| Self::extract_implements_relations(content));
        for (implementer, trait_name) in implement_relations {
            let source = if ast_semantics.is_some() {
                "ast_impl"
            } else {
                "heuristic_impl"
            };
            let confidence = if ast_semantics.is_some() {
                Self::CKG_EDGE_CONFIDENCE_AST
            } else {
                Self::CKG_EDGE_CONFIDENCE_HEURISTIC
            };
            let implementer_id = if let Some(id) = type_ids_by_name.get(&implementer).copied() {
                id
            } else if ast_semantics.is_some() {
                let id =
                    Self::stable_node_id("semantic-type", &format!("{}|{}", path, implementer));
                let implementer_node = Node {
                    id,
                    kind: NodeKind::Type,
                    name: implementer.clone(),
                    file: Some(path.to_string()),
                    data: serde_json::json!({ "source": source }),
                };
                graph
                    .add_node(&implementer_node)
                    .map_err(|err| format!("failed upserting semantic type node: {}", err))?;
                Self::add_edge_dedup(
                    graph,
                    &mut seen_edges,
                    Self::edge_with_provenance(
                        file_node_id,
                        id,
                        EdgeKind::Contains,
                        "ast_type_contains",
                        language,
                        Self::CKG_EDGE_CONFIDENCE_AST,
                    ),
                )?;
                type_ids_by_name.entry(implementer.clone()).or_insert(id);
                id
            } else {
                let id = Self::stable_node_id("implementer", &implementer);
                Self::upsert_external_node(graph, id, NodeKind::Type, &implementer, source)?;
                id
            };

            let trait_id = if let Some(id) = trait_ids_by_name.get(&trait_name).copied() {
                id
            } else {
                let id = Self::stable_node_id("trait-target", &trait_name);
                Self::upsert_external_node(graph, id, NodeKind::Trait, &trait_name, source)?;
                id
            };

            Self::add_edge_dedup(
                graph,
                &mut seen_edges,
                Self::edge_with_provenance(
                    implementer_id,
                    trait_id,
                    EdgeKind::Implements,
                    source,
                    language,
                    confidence,
                ),
            )?;
        }

        let parsed_functions = if ast_semantics.is_some() {
            Vec::new()
        } else {
            Self::extract_rust_function_bodies(content)
        };
        let fallback_function_names: Vec<String> = parsed_functions
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let function_names = ast_semantics
            .as_ref()
            .map(|semantic| semantic.functions.clone())
            .unwrap_or(fallback_function_names);
        let declared_function_names: HashSet<String> = function_names.iter().cloned().collect();
        for function_name in &function_names {
            if function_ids_by_name.contains_key(function_name) {
                continue;
            }
            let function_id =
                Self::stable_node_id("heuristic-function", &format!("{}|{}", path, function_name));
            let function_node = Node {
                id: function_id,
                kind: NodeKind::Function,
                name: function_name.clone(),
                file: Some(path.to_string()),
                data: serde_json::json!({
                    "source": if ast_semantics.is_some() {
                        "ast_function"
                    } else {
                        "heuristic_call"
                    }
                }),
            };
            graph
                .add_node(&function_node)
                .map_err(|err| format!("failed upserting heuristic function node: {}", err))?;
            Self::add_edge_dedup(
                graph,
                &mut seen_edges,
                Self::edge_with_provenance(
                    file_node_id,
                    function_id,
                    EdgeKind::Contains,
                    if ast_semantics.is_some() {
                        "ast_function_contains"
                    } else {
                        "heuristic_function_contains"
                    },
                    language,
                    if ast_semantics.is_some() {
                        Self::CKG_EDGE_CONFIDENCE_AST
                    } else {
                        Self::CKG_EDGE_CONFIDENCE_HEURISTIC
                    },
                ),
            )?;
            function_ids_by_name
                .entry(function_name.clone())
                .or_insert(function_id);
        }

        let ast_call_relations = ast_semantics
            .as_ref()
            .map(|semantic| semantic.calls.clone())
            .unwrap_or_default();
        if !ast_call_relations.is_empty() {
            for (caller_name, callee_name) in ast_call_relations {
                let Some(caller_id) = function_ids_by_name.get(&caller_name).copied() else {
                    continue;
                };

                let (callee_ids, source, confidence) = if declared_function_names
                    .contains(&callee_name)
                {
                    if let Some(id) = function_ids_by_name.get(&callee_name).copied() {
                        (vec![id], "ast_call", Self::CKG_EDGE_CONFIDENCE_AST)
                    } else {
                        let global_targets =
                            Self::find_global_function_targets(graph, &callee_name, Some(path), 4)?;
                        if !global_targets.is_empty() {
                            (
                                global_targets,
                                "ast_call_cross_file",
                                Self::CKG_EDGE_CONFIDENCE_AST * 0.9,
                            )
                        } else {
                            let id = Self::stable_node_id("callee-function", &callee_name);
                            Self::upsert_external_node(
                                graph,
                                id,
                                NodeKind::Function,
                                &callee_name,
                                "ast_call",
                            )?;
                            (vec![id], "ast_call", Self::CKG_EDGE_CONFIDENCE_AST)
                        }
                    }
                } else {
                    let global_targets =
                        Self::find_global_function_targets(graph, &callee_name, Some(path), 4)?;
                    if !global_targets.is_empty() {
                        (
                            global_targets,
                            "ast_call_cross_file",
                            Self::CKG_EDGE_CONFIDENCE_AST * 0.9,
                        )
                    } else {
                        let id = Self::stable_node_id("callee-function", &callee_name);
                        Self::upsert_external_node(
                            graph,
                            id,
                            NodeKind::Function,
                            &callee_name,
                            "ast_call",
                        )?;
                        (vec![id], "ast_call", Self::CKG_EDGE_CONFIDENCE_AST)
                    }
                };

                for callee_id in callee_ids {
                    Self::add_edge_dedup(
                        graph,
                        &mut seen_edges,
                        Self::edge_with_provenance(
                            caller_id,
                            callee_id,
                            EdgeKind::Calls,
                            source,
                            language,
                            confidence,
                        ),
                    )?;
                }
            }
        } else {
            for (caller_name, body) in parsed_functions {
                let Some(caller_id) = function_ids_by_name.get(&caller_name).copied() else {
                    continue;
                };

                for (callee_name, callee_id) in &function_ids_by_name {
                    if callee_name == &caller_name {
                        continue;
                    }
                    if !Self::body_contains_call(&body, callee_name) {
                        continue;
                    }
                    Self::add_edge_dedup(
                        graph,
                        &mut seen_edges,
                        Self::edge_with_provenance(
                            caller_id,
                            *callee_id,
                            EdgeKind::Calls,
                            "heuristic_call",
                            language,
                            Self::CKG_EDGE_CONFIDENCE_HEURISTIC,
                        ),
                    )?;
                }
            }
        }

        Ok(())
    }

    fn query_graph_file_data(
        graph: &CodeGraph,
        path: &str,
    ) -> Result<(Vec<GraphNodeInfo>, Vec<GraphEdgeInfo>), String> {
        let initial_nodes = graph
            .get_nodes_in_file(path)
            .map_err(|err| format!("failed loading file nodes: {}", err))?;

        if initial_nodes.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let source_ids: Vec<NodeId> = initial_nodes.iter().map(|node| node.id).collect();
        let mut nodes_by_id: HashMap<i64, Node> = initial_nodes
            .into_iter()
            .map(|node| (node.id.0, node))
            .collect();

        let mut edge_infos = Vec::new();
        let mut seen_edges: HashSet<(i64, i64, String)> = HashSet::new();

        for source_id in source_ids {
            let outgoing = graph
                .get_outgoing(source_id)
                .map_err(|err| format!("failed loading outgoing edges: {}", err))?;

            for edge in outgoing {
                let edge_key = (edge.from.0, edge.to.0, edge.kind.as_str().to_string());
                if seen_edges.insert(edge_key) {
                    edge_infos.push(Self::graph_edge_to_info(&edge));
                }

                if !nodes_by_id.contains_key(&edge.to.0) {
                    if let Some(target_node) = graph
                        .get_node(edge.to)
                        .map_err(|err| format!("failed loading edge target node: {}", err))?
                    {
                        nodes_by_id.insert(target_node.id.0, target_node);
                    }
                }
            }
        }

        let mut node_infos: Vec<GraphNodeInfo> = nodes_by_id
            .into_values()
            .map(|node| Self::graph_node_to_info(&node))
            .collect();
        Self::sort_graph_node_infos(&mut node_infos);
        Self::sort_graph_edge_infos(&mut edge_infos);

        Ok((node_infos, edge_infos))
    }

    fn search_kind_matches_filter(node: &Node, filter: Option<&str>) -> bool {
        let Some(filter) = filter else {
            return true;
        };
        let normalized = filter.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return true;
        }
        node.kind.as_str() == normalized
    }

    fn search_file_matches_filter(node: &Node, filter: Option<&str>) -> bool {
        let Some(filter) = filter else {
            return true;
        };
        let normalized = filter.trim();
        if normalized.is_empty() {
            return true;
        }
        node.file.as_deref() == Some(normalized)
    }

    fn sort_nodes_for_search(nodes: &mut [Node]) {
        nodes.sort_by(|left, right| {
            left.kind
                .as_str()
                .cmp(right.kind.as_str())
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.file.cmp(&right.file))
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
    }

    fn search_graph_data(
        graph: &CodeGraph,
        query: &str,
        limit: Option<usize>,
        offset: Option<usize>,
        kind_filter: Option<&str>,
        file_filter: Option<&str>,
    ) -> Result<(Vec<GraphNodeInfo>, Vec<GraphEdgeInfo>), String> {
        let capped_limit = limit
            .unwrap_or(Self::GRAPH_SEARCH_DEFAULT_LIMIT)
            .min(Self::GRAPH_SEARCH_MAX_LIMIT);
        if capped_limit == 0 {
            return Ok((Vec::new(), Vec::new()));
        }
        let offset = offset.unwrap_or(0);

        let mut nodes = graph
            .find_by_name(query)
            .map_err(|err| format!("failed searching graph by name: {}", err))?;
        nodes.retain(|node| {
            Self::search_kind_matches_filter(node, kind_filter)
                && Self::search_file_matches_filter(node, file_filter)
        });
        Self::sort_nodes_for_search(&mut nodes);

        let limited_nodes: Vec<Node> = nodes.into_iter().skip(offset).take(capped_limit).collect();
        if limited_nodes.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let node_id_set: HashSet<i64> = limited_nodes.iter().map(|node| node.id.0).collect();
        let mut edge_infos = Vec::new();
        let mut seen_edges: HashSet<(i64, i64, String)> = HashSet::new();
        for node in &limited_nodes {
            let outgoing = graph
                .get_outgoing(node.id)
                .map_err(|err| format!("failed loading outgoing edges: {}", err))?;
            for edge in outgoing {
                if !node_id_set.contains(&edge.to.0) {
                    continue;
                }
                let edge_key = (edge.from.0, edge.to.0, edge.kind.as_str().to_string());
                if seen_edges.insert(edge_key) {
                    edge_infos.push(Self::graph_edge_to_info(&edge));
                }
            }
        }

        let mut node_infos: Vec<GraphNodeInfo> =
            limited_nodes.iter().map(Self::graph_node_to_info).collect();
        Self::sort_graph_node_infos(&mut node_infos);
        Self::sort_graph_edge_infos(&mut edge_infos);

        Ok((node_infos, edge_infos))
    }

    fn normalize_graph_context_scope(context_files: Option<Vec<String>>) -> BTreeSet<String> {
        let mut scope = BTreeSet::new();
        if let Some(files) = context_files {
            for file in files {
                let trimmed = file.trim();
                if !trimmed.is_empty() {
                    scope.insert(trimmed.to_string());
                }
            }
        }
        scope
    }

    fn tokenize_graph_query(query: &str) -> Vec<String> {
        let mut terms = BTreeSet::new();
        for token in query.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')) {
            let normalized = token.trim().to_ascii_lowercase();
            if normalized.len() >= 2 {
                terms.insert(normalized);
                if terms.len() >= 8 {
                    break;
                }
            }
        }
        terms.into_iter().collect()
    }

    fn score_graph_context_candidate(
        node: &Node,
        query_lower: &str,
        query_terms: &[String],
        scoped_files: &BTreeSet<String>,
        incoming: usize,
        outgoing: usize,
    ) -> (f64, Vec<String>) {
        let mut score = 0.0f64;
        let mut reasons = Vec::new();
        let name_lower = node.name.to_ascii_lowercase();

        if !query_lower.is_empty() {
            if name_lower == query_lower {
                score += 16.0;
                reasons.push("exact_name".to_string());
            }
            if name_lower.contains(query_lower) {
                score += 8.0;
                reasons.push("query_substring".to_string());
            }
            let term_matches = query_terms
                .iter()
                .filter(|term| name_lower.contains(term.as_str()))
                .count();
            if term_matches > 0 {
                score += (term_matches as f64) * 3.0;
                reasons.push(format!("term_matches:{}", term_matches));
            }
        }

        match node.kind {
            NodeKind::Function => {
                score += 2.0;
                reasons.push("function_kind".to_string());
            }
            NodeKind::Struct | NodeKind::Enum | NodeKind::Trait | NodeKind::Type => {
                score += 1.5;
                reasons.push("type_kind".to_string());
            }
            NodeKind::Module => {
                score += 1.2;
                reasons.push("module_kind".to_string());
            }
            NodeKind::File => {
                score -= 1.0;
            }
            NodeKind::Constant | NodeKind::Concept => {}
        }

        if let Some(file) = node.file.as_deref() {
            if scoped_files.contains(file) {
                score += 4.0;
                reasons.push("context_file".to_string());
            }
        }

        let degree = incoming.saturating_add(outgoing);
        if degree > 0 {
            score += (degree.min(24) as f64) * 0.2;
            reasons.push(format!("degree:{}", degree));
        }

        if reasons.is_empty() {
            reasons.push("name_match".to_string());
        }

        (score.max(0.0), reasons)
    }

    fn query_graph_context_data(
        graph: &CodeGraph,
        query: &str,
        context_files: Option<Vec<String>>,
        limit: Option<usize>,
    ) -> Result<(String, Vec<GraphContextItem>), String> {
        let capped_limit = limit
            .unwrap_or(Self::GRAPH_CONTEXT_DEFAULT_LIMIT)
            .min(Self::GRAPH_CONTEXT_MAX_LIMIT);
        if capped_limit == 0 {
            return Ok((
                "graph context requested with limit=0; returning no results".to_string(),
                Vec::new(),
            ));
        }

        let query_trimmed = query.trim();
        let query_lower = query_trimmed.to_ascii_lowercase();
        let query_terms = Self::tokenize_graph_query(query_trimmed);
        let scoped_files = Self::normalize_graph_context_scope(context_files);
        let mut candidates: HashMap<i64, Node> = HashMap::new();

        if !query_trimmed.is_empty() {
            for node in graph
                .find_by_name(query_trimmed)
                .map_err(|err| format!("failed searching graph context by query: {}", err))?
            {
                candidates.entry(node.id.0).or_insert(node);
            }

            for term in &query_terms {
                if term == &query_lower {
                    continue;
                }
                for node in graph
                    .find_by_name(term)
                    .map_err(|err| format!("failed searching graph context by term: {}", err))?
                {
                    candidates.entry(node.id.0).or_insert(node);
                }
            }
        }

        for file in &scoped_files {
            for node in graph
                .get_nodes_in_file(file)
                .map_err(|err| format!("failed loading scoped file nodes: {}", err))?
            {
                candidates.entry(node.id.0).or_insert(node);
            }
        }

        if candidates.is_empty() {
            let summary = if query_trimmed.is_empty() && scoped_files.is_empty() {
                "graph context has no candidates (empty query and empty scope)".to_string()
            } else {
                format!(
                    "graph context has no candidates for query='{}' scoped_files={}",
                    query_trimmed,
                    scoped_files.len()
                )
            };
            return Ok((summary, Vec::new()));
        }

        let mut items = Vec::new();
        for node in candidates.into_values() {
            let incoming = graph
                .get_incoming(node.id)
                .map_err(|err| format!("failed loading incoming edges: {}", err))?
                .len();
            let outgoing = graph
                .get_outgoing(node.id)
                .map_err(|err| format!("failed loading outgoing edges: {}", err))?
                .len();
            let (score, reasons) = Self::score_graph_context_candidate(
                &node,
                &query_lower,
                &query_terms,
                &scoped_files,
                incoming,
                outgoing,
            );
            if score <= 0.0 {
                continue;
            }
            items.push(GraphContextItem {
                node: Self::graph_node_to_info(&node),
                score,
                incoming,
                outgoing,
                reasons,
            });
        }

        if items.is_empty() {
            return Ok((
                "graph context candidates found but all scores were zero".to_string(),
                Vec::new(),
            ));
        }

        items.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.node.kind.cmp(&right.node.kind))
                .then_with(|| left.node.name.cmp(&right.node.name))
                .then_with(|| left.node.file.cmp(&right.node.file))
                .then_with(|| left.node.id.cmp(&right.node.id))
        });

        let total_ranked = items.len();
        if items.len() > capped_limit {
            items.truncate(capped_limit);
        }

        let summary = format!(
            "graph context returned {} of {} candidates (query_terms={}, scoped_files={})",
            items.len(),
            total_ranked,
            query_terms.len(),
            scoped_files.len()
        );
        Ok((summary, items))
    }

    fn format_chat_graph_context(summary: &str, items: &[GraphContextItem]) -> String {
        let mut lines = Vec::with_capacity(items.len().saturating_add(2));
        lines.push(format!("CKG summary: {}", summary));
        lines.push("Top graph context candidates:".to_string());
        for item in items {
            let file = item
                .node
                .file
                .clone()
                .unwrap_or_else(|| "<external>".to_string());
            let reasons = if item.reasons.is_empty() {
                "none".to_string()
            } else {
                item.reasons.join(", ")
            };
            lines.push(format!(
                "- score={:.2} {} {} file={} in={} out={} reasons=[{}]",
                item.score,
                item.node.kind,
                item.node.name,
                file,
                item.incoming,
                item.outgoing,
                reasons
            ));
        }
        lines.join("\n")
    }

    fn index_queue_config() -> IndexQueueConfig {
        IndexQueueConfig {
            debounce_ms: Self::INDEX_QUEUE_DEBOUNCE_MS,
            retry_backoff_ms: Self::INDEX_QUEUE_RETRY_BACKOFF_MS,
            max_retries: Self::INDEX_QUEUE_MAX_RETRIES,
            max_pending: Self::INDEX_QUEUE_MAX_PENDING,
        }
    }

    fn index_queue_stats_response(&self) -> CoreToUi {
        let config = Self::index_queue_config();
        match self.index_queue.lock() {
            Ok(state) => CoreToUi::IndexQueueStats {
                stats: IndexQueueStatsInfo {
                    queued: state.tasks.len(),
                    in_progress: state.in_progress,
                    completed: state.completed,
                    failed: state.failed,
                    retried: state.retried,
                    dropped: state.dropped,
                    max_pending: config.max_pending,
                    debounce_ms: config.debounce_ms,
                    max_retries: config.max_retries,
                    last_error: state.last_error.clone(),
                },
            },
            Err(err) => Self::protocol_error(
                ErrorCode::IndexingFailed,
                format!("Index queue stats unavailable: {}", err),
                true,
                false,
            ),
        }
    }

    fn flush_index_queue(&self) -> CoreToUi {
        match self.index_queue.lock() {
            Ok(mut state) => {
                let cancelled = state.tasks.len();
                for (_, task) in state.tasks.drain() {
                    task.handle.abort();
                }
                state.dropped = state.dropped.saturating_add(cancelled as u64);
                CoreToUi::IndexQueueFlushed { cancelled }
            }
            Err(err) => Self::protocol_error(
                ErrorCode::IndexingFailed,
                format!("Failed to flush index queue: {}", err),
                true,
                false,
            ),
        }
    }

    fn cancel_queued_path(&self, path: &str) {
        if let Ok(mut state) = self.index_queue.lock() {
            if let Some(task) = state.tasks.remove(path) {
                task.handle.abort();
                state.dropped = state.dropped.saturating_add(1);
            }
        }
    }

    fn queue_index_file(&self, path: String) -> CoreToUi {
        let config = Self::index_queue_config();
        let mut state = match self.index_queue.lock() {
            Ok(state) => state,
            Err(err) => {
                return Self::protocol_error(
                    ErrorCode::IndexingFailed,
                    format!("Failed to lock index queue: {}", err),
                    true,
                    false,
                );
            }
        };

        if !state.tasks.contains_key(&path) && state.tasks.len() >= config.max_pending {
            state.dropped = state.dropped.saturating_add(1);
            return Self::protocol_error(
                ErrorCode::IndexQueueFull,
                format!("Index queue full (max pending {})", config.max_pending),
                true,
                false,
            );
        }

        if let Some(existing) = state.tasks.remove(&path) {
            existing.handle.abort();
            state.dropped = state.dropped.saturating_add(1);
        }

        state.next_generation = state.next_generation.wrapping_add(1);
        let generation = state.next_generation;

        let path_for_task = path.clone();
        let ce = self.ce.clone();
        let graph = self.graph.clone();
        let queue_state = self.index_queue.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(config.debounce_ms)).await;

            if let Ok(mut state) = queue_state.lock() {
                state.in_progress = state.in_progress.saturating_add(1);
            }

            for attempt in 0..=config.max_retries {
                match Glyph::index_file_with_components(path_for_task.as_str(), &ce, &graph).await {
                    Ok(_) => {
                        if let Ok(mut state) = queue_state.lock() {
                            state.completed = state.completed.saturating_add(1);
                            state.last_error = None;
                        }
                        break;
                    }
                    Err(err) => {
                        if let Ok(mut state) = queue_state.lock() {
                            state.last_error = Some(format!("{}: {}", path_for_task, err));
                        }

                        if attempt < config.max_retries {
                            if let Ok(mut state) = queue_state.lock() {
                                state.retried = state.retried.saturating_add(1);
                            }
                            let delay = config
                                .retry_backoff_ms
                                .saturating_mul((attempt as u64).saturating_add(1));
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                            continue;
                        }

                        if let Ok(mut state) = queue_state.lock() {
                            state.failed = state.failed.saturating_add(1);
                        }
                        break;
                    }
                }
            }

            if let Ok(mut state) = queue_state.lock() {
                state.in_progress = state.in_progress.saturating_sub(1);
                if state
                    .tasks
                    .get(&path_for_task)
                    .map(|entry| entry.generation)
                    == Some(generation)
                {
                    state.tasks.remove(&path_for_task);
                }
            }
        });

        state
            .tasks
            .insert(path.clone(), IndexTaskEntry { generation, handle });
        CoreToUi::IndexQueued {
            path,
            queued_count: state.tasks.len(),
            debounce_ms: config.debounce_ms,
        }
    }

    async fn index_file_with_components(
        path: &str,
        ce: &Arc<RwLock<ComprehensionEngine>>,
        graph: &Arc<Mutex<CodeGraph>>,
    ) -> Result<usize, String> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|err| format!("Failed to read file '{}': {}", path, err))?;

        let mut ce = ce.write().await;
        let path_buf = Path::new(path);
        let symbol_ids = ce
            .index_file(path_buf, &content)
            .map_err(|err| format!("Indexing failed for '{}': {}", path, err))?;
        let indexed_symbols = Self::collect_indexed_symbols(&ce, path_buf);
        drop(ce);

        let graph = graph
            .lock()
            .map_err(|err| format!("Failed to lock graph while indexing '{}': {}", path, err))?;
        Self::rebuild_file_graph(&graph, path, &content, &indexed_symbols)
            .map_err(|err| format!("Graph update failed for '{}': {}", path, err))?;

        Ok(symbol_ids.len())
    }

    fn graph_query_failed(message: impl Into<String>) -> CoreToUi {
        Self::protocol_error(ErrorCode::GraphQueryFailed, message, true, false)
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
                let mut context_sections = Vec::new();
                if !context_files.is_empty() {
                    let focused_files = context_files
                        .iter()
                        .map(|path| format!("- {}", path))
                        .collect::<Vec<_>>()
                        .join("\n");
                    context_sections.push(format!("Focused files:\n{}", focused_files));
                }

                match self.graph.lock() {
                    Ok(graph) => {
                        let context_scope = if context_files.is_empty() {
                            None
                        } else {
                            Some(context_files.clone())
                        };
                        match Self::query_graph_context_data(
                            &graph,
                            &message,
                            context_scope,
                            Some(Self::GRAPH_CONTEXT_DEFAULT_LIMIT),
                        ) {
                            Ok((summary, items)) => {
                                if !items.is_empty() {
                                    context_sections
                                        .push(Self::format_chat_graph_context(&summary, &items));
                                }
                            }
                            Err(err) => {
                                warn!("failed building graph context for chat: {}", err);
                            }
                        }
                    }
                    Err(err) => {
                        warn!("failed locking graph for chat context: {}", err);
                    }
                }

                let context = if context_sections.is_empty() {
                    None
                } else {
                    Some(context_sections.join("\n\n"))
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

            UiToCore::QueueIndexFile { path } => self.queue_index_file(path),

            UiToCore::FlushIndexQueue => self.flush_index_queue(),

            UiToCore::GetIndexQueueStats => self.index_queue_stats_response(),

            UiToCore::IndexFile { path } => {
                self.cancel_queued_path(&path);
                match Self::index_file_with_components(&path, &self.ce, &self.graph).await {
                    Ok(symbol_count) => {
                        info!("Indexed {} symbols from {}", symbol_count, path);
                        CoreToUi::FileIndexed { path, symbol_count }
                    }
                    Err(err) => {
                        error!("IndexFile failed for '{}': {}", path, err);
                        let code = if err.contains("Failed to read file") {
                            ErrorCode::FileReadFailed
                        } else {
                            ErrorCode::IndexingFailed
                        };
                        Self::protocol_error(code, err, true, false)
                    }
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

            UiToCore::QueryGraphFile { path } => {
                let graph = match self.graph.lock() {
                    Ok(graph) => graph,
                    Err(err) => {
                        error!(
                            "Failed to lock graph for QueryGraphFile '{}': {}",
                            path, err
                        );
                        return Self::graph_query_failed(format!(
                            "Graph lock failure for file query '{}'",
                            path
                        ));
                    }
                };

                match Self::query_graph_file_data(&graph, &path) {
                    Ok((nodes, edges)) => CoreToUi::GraphData { nodes, edges },
                    Err(err) => {
                        error!("Graph file query failed for '{}': {}", path, err);
                        Self::graph_query_failed(format!(
                            "Graph file query failed for '{}': {}",
                            path, err
                        ))
                    }
                }
            }

            UiToCore::SearchGraph {
                query,
                limit,
                offset,
                kind_filter,
                file_filter,
            } => {
                let graph = match self.graph.lock() {
                    Ok(graph) => graph,
                    Err(err) => {
                        error!("Failed to lock graph for SearchGraph '{}': {}", query, err);
                        return Self::graph_query_failed(format!(
                            "Graph lock failure for search '{}'",
                            query
                        ));
                    }
                };

                match Self::search_graph_data(
                    &graph,
                    &query,
                    limit,
                    offset,
                    kind_filter.as_deref(),
                    file_filter.as_deref(),
                ) {
                    Ok((nodes, edges)) => CoreToUi::GraphData { nodes, edges },
                    Err(err) => {
                        error!("Graph search failed for '{}': {}", query, err);
                        Self::graph_query_failed(format!(
                            "Graph search failed for '{}': {}",
                            query, err
                        ))
                    }
                }
            }

            UiToCore::QueryGraphContext {
                query,
                context_files,
                limit,
            } => {
                let graph = match self.graph.lock() {
                    Ok(graph) => graph,
                    Err(err) => {
                        error!(
                            "Failed to lock graph for QueryGraphContext '{}': {}",
                            query, err
                        );
                        return Self::graph_query_failed(format!(
                            "Graph lock failure for context query '{}'",
                            query
                        ));
                    }
                };

                match Self::query_graph_context_data(&graph, &query, context_files, limit) {
                    Ok((summary, items)) => CoreToUi::GraphContext { summary, items },
                    Err(err) => {
                        error!("Graph context query failed for '{}': {}", query, err);
                        Self::graph_query_failed(format!(
                            "Graph context query failed for '{}': {}",
                            query, err
                        ))
                    }
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "glyph-core-{}-{}-{}",
            test_name,
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&root).expect("failed creating temp test directory");
        root
    }

    fn expect_graph_data(response: CoreToUi) -> (Vec<GraphNodeInfo>, Vec<GraphEdgeInfo>) {
        match response {
            CoreToUi::GraphData { nodes, edges } => (nodes, edges),
            other => panic!("expected graph data response, got {:?}", other),
        }
    }

    fn expect_graph_context(response: CoreToUi) -> (String, Vec<GraphContextItem>) {
        match response {
            CoreToUi::GraphContext { summary, items } => (summary, items),
            other => panic!("expected graph context response, got {:?}", other),
        }
    }

    async fn index_queue_stats(glyph: &Glyph) -> IndexQueueStatsInfo {
        match glyph.handle_message(UiToCore::GetIndexQueueStats).await {
            CoreToUi::IndexQueueStats { stats } => stats,
            other => panic!("expected index queue stats response, got {:?}", other),
        }
    }

    fn assert_edge_provenance(
        edge: &Edge,
        expected_source: &str,
        expected_language: &str,
        min_confidence: f64,
    ) {
        let source = edge
            .data
            .get("source")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert_eq!(source, expected_source);

        let language = edge
            .data
            .get("language")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert_eq!(language, expected_language);

        let timestamp = edge
            .data
            .get("timestamp_unix")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        assert!(timestamp > 0);

        let confidence = edge
            .data
            .get("confidence")
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        assert!(confidence >= min_confidence);

        let extractor = edge
            .data
            .get("extractor_version")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert_eq!(extractor, Glyph::CKG_EXTRACTOR_VERSION);
    }

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

    #[tokio::test]
    async fn test_index_file_rebuilds_file_graph_nodes_and_contains_edges() {
        let root = unique_temp_dir("index_file_graph");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("sample.rs");
        std::fs::write(&source_path, "fn alpha() {}\n").expect("failed to write source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile {
                path: source_path.to_string_lossy().to_string(),
            })
            .await;
        let symbol_count = match index_response {
            CoreToUi::FileIndexed { symbol_count, .. } => symbol_count,
            other => panic!("expected FileIndexed response, got {:?}", other),
        };
        assert!(symbol_count >= 1);

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile {
                    path: source_path.to_string_lossy().to_string(),
                })
                .await,
        );
        assert!(nodes.iter().any(|node| {
            node.kind == NodeKind::File.as_str() && node.name == source_path.to_string_lossy()
        }));
        assert!(
            nodes
                .iter()
                .filter(|node| node.kind != NodeKind::File.as_str())
                .count()
                >= symbol_count
        );
        assert!(edges.iter().filter(|edge| edge.kind == "contains").count() >= symbol_count);
    }

    #[tokio::test]
    async fn test_queue_index_file_processes_and_updates_stats() {
        let root = unique_temp_dir("queue_index_file");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("queued.rs");
        let path = source_path.to_string_lossy().to_string();
        std::fs::write(&source_path, "fn queued_fn() {}\n").expect("failed to write queued source");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let queued_response = glyph
            .handle_message(UiToCore::QueueIndexFile { path: path.clone() })
            .await;
        match queued_response {
            CoreToUi::IndexQueued {
                path: queued_path,
                queued_count,
                debounce_ms,
            } => {
                assert_eq!(queued_path, path);
                assert!(queued_count >= 1);
                assert_eq!(debounce_ms, Glyph::INDEX_QUEUE_DEBOUNCE_MS);
            }
            other => panic!("expected index queued response, got {:?}", other),
        }

        let mut completed = false;
        for _ in 0..80 {
            let stats = index_queue_stats(&glyph).await;
            if stats.completed >= 1 {
                completed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(completed, "queued indexing did not complete within timeout");

        let final_stats = index_queue_stats(&glyph).await;
        assert!(final_stats.completed >= 1);
        assert_eq!(final_stats.failed, 0);

        let (nodes, _) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path: path.clone() })
                .await,
        );
        assert!(nodes.iter().any(|node| node.name == "queued_fn"));
    }

    #[tokio::test]
    async fn test_flush_index_queue_cancels_pending_tasks() {
        let root = unique_temp_dir("flush_queue");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("flush.rs");
        let path = source_path.to_string_lossy().to_string();
        std::fs::write(&source_path, "fn flush_fn() {}\n").expect("failed to write flush source");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let queued_response = glyph
            .handle_message(UiToCore::QueueIndexFile { path: path.clone() })
            .await;
        assert!(matches!(queued_response, CoreToUi::IndexQueued { .. }));

        let flush_response = glyph.handle_message(UiToCore::FlushIndexQueue).await;
        let cancelled = match flush_response {
            CoreToUi::IndexQueueFlushed { cancelled } => cancelled,
            other => panic!("expected index queue flushed response, got {:?}", other),
        };
        assert!(cancelled >= 1);

        let stats = index_queue_stats(&glyph).await;
        assert_eq!(stats.queued, 0);
    }

    #[tokio::test]
    async fn test_index_file_adds_rust_semantic_edges() {
        let root = unique_temp_dir("semantic_edges");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("semantic.rs");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
use std::fmt::Debug;

trait Runner {}
struct Worker;
impl Runner for Worker {}

fn helper() {}
fn caller() { helper(); }
"#;
        std::fs::write(&source_path, source).expect("failed to write semantic source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path })
                .await,
        );
        let mut ids_by_name: HashMap<String, Vec<i64>> = HashMap::new();
        for node in &nodes {
            ids_by_name
                .entry(node.name.clone())
                .or_default()
                .push(node.id);
        }

        assert!(edges.iter().any(|edge| edge.kind == "imports"));
        assert!(edges.iter().any(|edge| edge.kind == "implements"));
        assert!(edges.iter().any(|edge| edge.kind == "calls"));

        let caller_ids = ids_by_name.get("caller").cloned().unwrap_or_default();
        let helper_ids = ids_by_name.get("helper").cloned().unwrap_or_default();
        assert!(!caller_ids.is_empty());
        assert!(!helper_ids.is_empty());
        assert!(edges.iter().any(|edge| {
            edge.kind == "calls"
                && caller_ids.contains(&edge.from_id)
                && helper_ids.contains(&edge.to_id)
        }));

        let worker_ids = ids_by_name.get("Worker").cloned().unwrap_or_default();
        let runner_ids = ids_by_name.get("Runner").cloned().unwrap_or_default();
        assert!(!worker_ids.is_empty());
        assert!(!runner_ids.is_empty());
        assert!(edges.iter().any(|edge| {
            edge.kind == "implements"
                && worker_ids.contains(&edge.from_id)
                && runner_ids.contains(&edge.to_id)
        }));
    }

    #[tokio::test]
    async fn test_index_file_adds_javascript_semantic_edges() {
        let root = unique_temp_dir("semantic_edges_js");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("semantic.js");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
import lib from "./dep";

class Worker extends Runner {}

function helper() {}
function caller() { helper(); }
"#;
        std::fs::write(&source_path, source)
            .expect("failed to write javascript semantic source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path })
                .await,
        );
        let mut ids_by_name: HashMap<String, Vec<i64>> = HashMap::new();
        for node in &nodes {
            ids_by_name
                .entry(node.name.clone())
                .or_default()
                .push(node.id);
        }

        assert!(nodes.iter().any(|node| node.name == "./dep"));
        assert!(edges.iter().any(|edge| edge.kind == "imports"));
        assert!(edges.iter().any(|edge| edge.kind == "implements"));
        assert!(edges.iter().any(|edge| edge.kind == "calls"));

        let caller_ids = ids_by_name.get("caller").cloned().unwrap_or_default();
        let helper_ids = ids_by_name.get("helper").cloned().unwrap_or_default();
        assert!(!caller_ids.is_empty());
        assert!(!helper_ids.is_empty());
        assert!(edges.iter().any(|edge| {
            edge.kind == "calls"
                && caller_ids.contains(&edge.from_id)
                && helper_ids.contains(&edge.to_id)
        }));

        let worker_ids = ids_by_name.get("Worker").cloned().unwrap_or_default();
        let runner_ids = ids_by_name.get("Runner").cloned().unwrap_or_default();
        assert!(!worker_ids.is_empty());
        assert!(!runner_ids.is_empty());
        assert!(edges.iter().any(|edge| {
            edge.kind == "implements"
                && worker_ids.contains(&edge.from_id)
                && runner_ids.contains(&edge.to_id)
        }));
    }

    #[tokio::test]
    async fn test_index_file_adds_python_semantic_edges() {
        let root = unique_temp_dir("semantic_edges_py");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("semantic.py");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
import os
from pkg.mod import thing

class Worker(Runner):
    pass

def helper():
    return 1

def caller():
    helper()
"#;
        std::fs::write(&source_path, source).expect("failed to write python semantic source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path })
                .await,
        );
        let mut ids_by_name: HashMap<String, Vec<i64>> = HashMap::new();
        for node in &nodes {
            ids_by_name
                .entry(node.name.clone())
                .or_default()
                .push(node.id);
        }

        assert!(nodes.iter().any(|node| node.name == "os"));
        assert!(nodes.iter().any(|node| node.name == "pkg.mod"));
        assert!(edges.iter().any(|edge| edge.kind == "imports"));
        assert!(edges.iter().any(|edge| edge.kind == "implements"));
        assert!(edges.iter().any(|edge| edge.kind == "calls"));

        let caller_ids = ids_by_name.get("caller").cloned().unwrap_or_default();
        let helper_ids = ids_by_name.get("helper").cloned().unwrap_or_default();
        assert!(!caller_ids.is_empty());
        assert!(!helper_ids.is_empty());
        assert!(edges.iter().any(|edge| {
            edge.kind == "calls"
                && caller_ids.contains(&edge.from_id)
                && helper_ids.contains(&edge.to_id)
        }));

        let worker_ids = ids_by_name.get("Worker").cloned().unwrap_or_default();
        let runner_ids = ids_by_name.get("Runner").cloned().unwrap_or_default();
        assert!(!worker_ids.is_empty());
        assert!(!runner_ids.is_empty());
        assert!(edges.iter().any(|edge| {
            edge.kind == "implements"
                && worker_ids.contains(&edge.from_id)
                && runner_ids.contains(&edge.to_id)
        }));
    }

    #[tokio::test]
    async fn test_index_file_resolves_javascript_relative_import_to_file_node() {
        let root = unique_temp_dir("resolved_js_import");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("entry.js");
        let dep_path = root.join("dep.js");
        std::fs::write(
            &source_path,
            "import dep from \"./dep\";\nfunction boot() { dep(); }\n",
        )
        .expect("failed to write source file");
        std::fs::write(&dep_path, "export function dep() { return 1; }\n")
            .expect("failed to write dep file");

        let source = source_path.to_string_lossy().to_string();
        let dep = dep_path.to_string_lossy().to_string();
        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile {
                path: source.clone(),
            })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile {
                    path: source.clone(),
                })
                .await,
        );

        let source_file_id = nodes
            .iter()
            .find(|node| node.kind == NodeKind::File.as_str() && node.name == source)
            .map(|node| node.id)
            .expect("missing source file node");
        let dep_file_ids: Vec<i64> = nodes
            .iter()
            .filter(|node| node.kind == NodeKind::File.as_str() && node.name == dep)
            .map(|node| node.id)
            .collect();
        assert!(
            !dep_file_ids.is_empty(),
            "missing resolved dependency file node"
        );
        assert!(edges.iter().any(|edge| {
            edge.kind == "imports"
                && edge.from_id == source_file_id
                && dep_file_ids.contains(&edge.to_id)
        }));
    }

    #[tokio::test]
    async fn test_index_file_links_ast_calls_to_existing_cross_file_functions() {
        let root = unique_temp_dir("cross_file_calls");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let helper_path = root.join("helper.rs");
        let caller_path = root.join("caller.rs");
        std::fs::write(&helper_path, "pub fn helper() {}\n").expect("failed to write helper file");
        std::fs::write(&caller_path, "fn caller() { helper(); }\n")
            .expect("failed to write caller file");

        let helper = helper_path.to_string_lossy().to_string();
        let caller = caller_path.to_string_lossy().to_string();
        let glyph = Glyph::new_with_graph_path(Some(&graph_path));

        let helper_index = glyph
            .handle_message(UiToCore::IndexFile {
                path: helper.clone(),
            })
            .await;
        assert!(matches!(helper_index, CoreToUi::FileIndexed { .. }));

        let caller_index = glyph
            .handle_message(UiToCore::IndexFile {
                path: caller.clone(),
            })
            .await;
        assert!(matches!(caller_index, CoreToUi::FileIndexed { .. }));

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile {
                    path: caller.clone(),
                })
                .await,
        );

        let caller_ids: Vec<i64> = nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Function.as_str() && node.name == "caller")
            .map(|node| node.id)
            .collect();
        assert!(!caller_ids.is_empty(), "missing caller function node");

        let helper_ids: Vec<i64> = nodes
            .iter()
            .filter(|node| {
                node.kind == NodeKind::Function.as_str()
                    && node.name == "helper"
                    && node.file.as_deref() == Some(helper.as_str())
            })
            .map(|node| node.id)
            .collect();
        assert!(
            !helper_ids.is_empty(),
            "missing cross-file helper function target"
        );

        assert!(edges.iter().any(|edge| {
            edge.kind == "calls"
                && caller_ids.contains(&edge.from_id)
                && helper_ids.contains(&edge.to_id)
        }));
        assert!(
            !nodes
                .iter()
                .any(|node| node.kind == NodeKind::Function.as_str()
                    && node.name == "helper"
                    && node.file.is_none()),
            "expected cross-file resolution to avoid external helper fallback"
        );
    }

    #[tokio::test]
    async fn test_ast_edges_include_provenance_metadata() {
        let root = unique_temp_dir("ast_edge_provenance");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("ast_meta.rs");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
use std::fmt::Debug;

trait Runner {}
struct Worker;
impl Runner for Worker {}

fn helper() {}
fn caller() { helper(); }
"#;
        std::fs::write(&source_path, source).expect("failed to write ast metadata source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let graph = glyph.graph.lock().expect("failed to lock graph");
        let file_node = graph
            .get_nodes_in_file(&path)
            .expect("failed reading file nodes")
            .into_iter()
            .find(|node| node.kind == NodeKind::File)
            .expect("missing file node");
        let file_outgoing = graph
            .get_outgoing(file_node.id)
            .expect("failed reading file outgoing edges");
        let import_edge = file_outgoing
            .iter()
            .find(|edge| edge.kind == EdgeKind::Imports)
            .expect("missing import edge");
        assert_edge_provenance(
            import_edge,
            "ast_import",
            "rust",
            Glyph::CKG_EDGE_CONFIDENCE_AST - 0.001,
        );

        let mut impl_edge = None;
        for node in graph.find_by_name("").expect("failed searching all nodes") {
            let outgoing = graph
                .get_outgoing(node.id)
                .expect("failed reading node outgoing edges");
            if let Some(edge) = outgoing
                .into_iter()
                .find(|edge| edge.kind == EdgeKind::Implements)
            {
                impl_edge = Some(edge);
                break;
            }
        }
        let impl_edge = impl_edge.expect("missing implements edge");
        assert_edge_provenance(
            &impl_edge,
            "ast_impl",
            "rust",
            Glyph::CKG_EDGE_CONFIDENCE_AST - 0.001,
        );

        let caller_ids: Vec<NodeId> = graph
            .find_by_name("caller")
            .expect("failed searching caller")
            .into_iter()
            .filter(|node| node.name == "caller")
            .map(|node| node.id)
            .collect();
        assert!(!caller_ids.is_empty(), "missing caller node");
        let helper_ids: HashSet<NodeId> = graph
            .find_by_name("helper")
            .expect("failed searching helper")
            .into_iter()
            .filter(|node| node.name == "helper")
            .map(|node| node.id)
            .collect();
        assert!(!helper_ids.is_empty(), "missing helper node");

        let mut call_edge: Option<Edge> = None;
        for caller_id in caller_ids {
            let caller_outgoing = graph
                .get_outgoing(caller_id)
                .expect("failed reading caller outgoing edges");
            if let Some(edge) = caller_outgoing
                .iter()
                .find(|edge| edge.kind == EdgeKind::Calls && helper_ids.contains(&edge.to))
            {
                call_edge = Some(edge.clone());
                break;
            }
        }
        let call_edge = call_edge.expect("missing call edge");
        assert_edge_provenance(
            &call_edge,
            "ast_call",
            "rust",
            Glyph::CKG_EDGE_CONFIDENCE_AST - 0.001,
        );
    }

    #[tokio::test]
    async fn test_python_ast_edges_include_provenance_metadata() {
        let root = unique_temp_dir("ast_edge_provenance_python");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("ast_meta.py");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
import requests

class Worker(Runner):
    pass

def helper():
    return 1

def caller():
    helper()
"#;
        std::fs::write(&source_path, source).expect("failed to write python metadata source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let graph = glyph.graph.lock().expect("failed to lock graph");
        let file_node = graph
            .get_nodes_in_file(&path)
            .expect("failed reading file nodes")
            .into_iter()
            .find(|node| node.kind == NodeKind::File)
            .expect("missing file node");
        let file_outgoing = graph
            .get_outgoing(file_node.id)
            .expect("failed reading file outgoing edges");
        let import_edge = file_outgoing
            .iter()
            .find(|edge| edge.kind == EdgeKind::Imports)
            .expect("missing import edge");
        assert_edge_provenance(
            import_edge,
            "ast_import",
            "python",
            Glyph::CKG_EDGE_CONFIDENCE_AST - 0.001,
        );

        let mut impl_edge = None;
        for node in graph.find_by_name("").expect("failed searching all nodes") {
            let outgoing = graph
                .get_outgoing(node.id)
                .expect("failed reading node outgoing edges");
            if let Some(edge) = outgoing
                .into_iter()
                .find(|edge| edge.kind == EdgeKind::Implements)
            {
                impl_edge = Some(edge);
                break;
            }
        }
        let impl_edge = impl_edge.expect("missing implements edge");
        assert_edge_provenance(
            &impl_edge,
            "ast_impl",
            "python",
            Glyph::CKG_EDGE_CONFIDENCE_AST - 0.001,
        );

        let caller_ids: Vec<NodeId> = graph
            .find_by_name("caller")
            .expect("failed searching caller")
            .into_iter()
            .filter(|node| node.name == "caller")
            .map(|node| node.id)
            .collect();
        assert!(!caller_ids.is_empty(), "missing caller node");
        let helper_ids: HashSet<NodeId> = graph
            .find_by_name("helper")
            .expect("failed searching helper")
            .into_iter()
            .filter(|node| node.name == "helper")
            .map(|node| node.id)
            .collect();
        assert!(!helper_ids.is_empty(), "missing helper node");

        let mut call_edge: Option<Edge> = None;
        for caller_id in caller_ids {
            let caller_outgoing = graph
                .get_outgoing(caller_id)
                .expect("failed reading caller outgoing edges");
            if let Some(edge) = caller_outgoing
                .iter()
                .find(|edge| edge.kind == EdgeKind::Calls && helper_ids.contains(&edge.to))
            {
                call_edge = Some(edge.clone());
                break;
            }
        }
        let call_edge = call_edge.expect("missing call edge");
        assert_edge_provenance(
            &call_edge,
            "ast_call",
            "python",
            Glyph::CKG_EDGE_CONFIDENCE_AST - 0.001,
        );
    }

    #[tokio::test]
    async fn test_heuristic_edges_include_provenance_metadata() {
        let root = unique_temp_dir("heuristic_edge_provenance");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("heuristic_meta.txt");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
import requests
"#;
        std::fs::write(&source_path, source)
            .expect("failed to write heuristic metadata source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let graph = glyph.graph.lock().expect("failed to lock graph");
        let file_node = graph
            .get_nodes_in_file(&path)
            .expect("failed reading file nodes")
            .into_iter()
            .find(|node| node.kind == NodeKind::File)
            .expect("missing file node");
        let file_outgoing = graph
            .get_outgoing(file_node.id)
            .expect("failed reading file outgoing edges");
        let import_edge = file_outgoing
            .iter()
            .find(|edge| edge.kind == EdgeKind::Imports)
            .expect("missing import edge");
        assert_edge_provenance(
            import_edge,
            "heuristic_import",
            "plain",
            Glyph::CKG_EDGE_CONFIDENCE_HEURISTIC - 0.001,
        );
    }

    #[tokio::test]
    async fn test_ast_calls_avoid_prefix_false_positives() {
        let root = unique_temp_dir("ast_call_precision");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("calls.rs");
        let path = source_path.to_string_lossy().to_string();
        let source = r#"
fn help() {}
fn helper() {}
fn caller() { helper(); }
"#;
        std::fs::write(&source_path, source).expect("failed to write call source file");

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));
        let index_response = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(index_response, CoreToUi::FileIndexed { .. }));

        let (nodes, edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path })
                .await,
        );
        let mut ids_by_name: HashMap<String, Vec<i64>> = HashMap::new();
        for node in &nodes {
            ids_by_name
                .entry(node.name.clone())
                .or_default()
                .push(node.id);
        }

        let caller_ids = ids_by_name.get("caller").cloned().unwrap_or_default();
        let helper_ids = ids_by_name.get("helper").cloned().unwrap_or_default();
        let help_ids = ids_by_name.get("help").cloned().unwrap_or_default();
        assert!(!caller_ids.is_empty());
        assert!(!helper_ids.is_empty());
        assert!(!help_ids.is_empty());

        assert!(edges.iter().any(|edge| {
            edge.kind == "calls"
                && caller_ids.contains(&edge.from_id)
                && helper_ids.contains(&edge.to_id)
        }));
        assert!(!edges.iter().any(|edge| {
            edge.kind == "calls"
                && caller_ids.contains(&edge.from_id)
                && help_ids.contains(&edge.to_id)
        }));
    }

    #[tokio::test]
    async fn test_reindex_removes_stale_symbol_nodes() {
        let root = unique_temp_dir("reindex_stale");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("reindex.rs");
        let path = source_path.to_string_lossy().to_string();

        let glyph = Glyph::new_with_graph_path(Some(&graph_path));

        std::fs::write(&source_path, "fn alpha() {}\n").expect("failed to write initial source");
        let first = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(first, CoreToUi::FileIndexed { .. }));

        let (first_nodes, _) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path: path.clone() })
                .await,
        );
        assert!(first_nodes.iter().any(|node| node.name == "alpha"));

        std::fs::write(&source_path, "fn beta() {}\n").expect("failed to write updated source");
        let second = glyph
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(second, CoreToUi::FileIndexed { .. }));

        let (second_nodes, _) = expect_graph_data(
            glyph
                .handle_message(UiToCore::QueryGraphFile { path })
                .await,
        );
        assert!(second_nodes.iter().any(|node| node.name == "beta"));
        assert!(!second_nodes.iter().any(|node| node.name == "alpha"));
    }

    #[tokio::test]
    async fn test_graph_data_persists_across_core_restart() {
        let root = unique_temp_dir("graph_persistence");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let source_path = root.join("persist.rs");
        let path = source_path.to_string_lossy().to_string();
        std::fs::write(&source_path, "fn persisted_fn() {}\n")
            .expect("failed to write source file");

        let glyph_a = Glyph::new_with_graph_path(Some(&graph_path));
        let indexed = glyph_a
            .handle_message(UiToCore::IndexFile { path: path.clone() })
            .await;
        assert!(matches!(indexed, CoreToUi::FileIndexed { .. }));
        drop(glyph_a);

        let glyph_b = Glyph::new_with_graph_path(Some(&graph_path));
        let (nodes, _) = expect_graph_data(
            glyph_b
                .handle_message(UiToCore::QueryGraphFile { path })
                .await,
        );
        assert!(nodes.iter().any(|node| node.name == "persisted_fn"));
    }

    #[tokio::test]
    async fn test_search_graph_respects_default_and_explicit_limits() {
        let root = unique_temp_dir("graph_search_limits");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let glyph = Glyph::new_with_graph_path(Some(&graph_path));

        {
            let graph = glyph.graph.lock().expect("failed to lock graph");
            for idx in 0..150 {
                let node = Node {
                    id: NodeId(10_000 + idx),
                    kind: NodeKind::Concept,
                    name: format!("limit_target_{}", idx),
                    file: None,
                    data: serde_json::Value::Null,
                };
                graph.add_node(&node).expect("failed to insert node");
            }
        }

        let (default_nodes, _) = expect_graph_data(
            glyph
                .handle_message(UiToCore::SearchGraph {
                    query: "limit_target_".to_string(),
                    limit: None,
                    offset: None,
                    kind_filter: None,
                    file_filter: None,
                })
                .await,
        );
        assert_eq!(default_nodes.len(), Glyph::GRAPH_SEARCH_DEFAULT_LIMIT);

        let explicit_limit = 7usize;
        let (limited_nodes, _) = expect_graph_data(
            glyph
                .handle_message(UiToCore::SearchGraph {
                    query: "limit_target_".to_string(),
                    limit: Some(explicit_limit),
                    offset: None,
                    kind_filter: None,
                    file_filter: None,
                })
                .await,
        );
        assert_eq!(limited_nodes.len(), explicit_limit);
    }

    #[tokio::test]
    async fn test_search_graph_respects_offset_filters_and_deterministic_order() {
        let root = unique_temp_dir("graph_search_filters");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let glyph = Glyph::new_with_graph_path(Some(&graph_path));

        {
            let graph = glyph.graph.lock().expect("failed to lock graph");
            let nodes = vec![
                Node {
                    id: NodeId(20_003),
                    kind: NodeKind::Function,
                    name: "target_d".to_string(),
                    file: Some("src/b.rs".to_string()),
                    data: serde_json::Value::Null,
                },
                Node {
                    id: NodeId(20_001),
                    kind: NodeKind::Concept,
                    name: "target_b".to_string(),
                    file: None,
                    data: serde_json::Value::Null,
                },
                Node {
                    id: NodeId(20_002),
                    kind: NodeKind::Function,
                    name: "target_c".to_string(),
                    file: Some("src/a.rs".to_string()),
                    data: serde_json::Value::Null,
                },
                Node {
                    id: NodeId(20_000),
                    kind: NodeKind::Function,
                    name: "target_a".to_string(),
                    file: Some("src/a.rs".to_string()),
                    data: serde_json::Value::Null,
                },
            ];
            for node in nodes {
                graph.add_node(&node).expect("failed to insert node");
            }
            graph
                .add_edge(&Edge::calls(NodeId(20_002), NodeId(20_000)))
                .expect("failed to insert edge");
            graph
                .add_edge(&Edge::calls(NodeId(20_003), NodeId(20_002)))
                .expect("failed to insert edge");
        }

        let (filtered_nodes, _) = expect_graph_data(
            glyph
                .handle_message(UiToCore::SearchGraph {
                    query: "target_".to_string(),
                    limit: Some(2),
                    offset: Some(1),
                    kind_filter: Some("function".to_string()),
                    file_filter: Some("src/a.rs".to_string()),
                })
                .await,
        );
        assert_eq!(filtered_nodes.len(), 1);
        assert_eq!(filtered_nodes[0].name, "target_c");

        let (first_nodes, first_edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::SearchGraph {
                    query: "target_".to_string(),
                    limit: Some(10),
                    offset: Some(0),
                    kind_filter: None,
                    file_filter: None,
                })
                .await,
        );
        let (second_nodes, second_edges) = expect_graph_data(
            glyph
                .handle_message(UiToCore::SearchGraph {
                    query: "target_".to_string(),
                    limit: Some(10),
                    offset: Some(0),
                    kind_filter: None,
                    file_filter: None,
                })
                .await,
        );

        let first_node_order: Vec<(String, String)> = first_nodes
            .iter()
            .map(|node| (node.kind.clone(), node.name.clone()))
            .collect();
        assert_eq!(
            first_node_order,
            vec![
                ("concept".to_string(), "target_b".to_string()),
                ("function".to_string(), "target_a".to_string()),
                ("function".to_string(), "target_c".to_string()),
                ("function".to_string(), "target_d".to_string()),
            ]
        );

        let second_node_order: Vec<(String, String)> = second_nodes
            .iter()
            .map(|node| (node.kind.clone(), node.name.clone()))
            .collect();
        assert_eq!(first_node_order, second_node_order);

        let first_edge_order: Vec<(String, i64, i64)> = first_edges
            .iter()
            .map(|edge| (edge.kind.clone(), edge.from_id, edge.to_id))
            .collect();
        let second_edge_order: Vec<(String, i64, i64)> = second_edges
            .iter()
            .map(|edge| (edge.kind.clone(), edge.from_id, edge.to_id))
            .collect();
        assert_eq!(first_edge_order, second_edge_order);
    }

    #[tokio::test]
    async fn test_query_graph_context_ranks_and_respects_limit() {
        let root = unique_temp_dir("graph_context_rank");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let glyph = Glyph::new_with_graph_path(Some(&graph_path));

        {
            let graph = glyph.graph.lock().expect("failed to lock graph");
            let nodes = vec![
                Node {
                    id: NodeId(30_000),
                    kind: NodeKind::Function,
                    name: "handler".to_string(),
                    file: Some("src/lib.rs".to_string()),
                    data: serde_json::Value::Null,
                },
                Node {
                    id: NodeId(30_001),
                    kind: NodeKind::Function,
                    name: "helper_handler".to_string(),
                    file: Some("src/other.rs".to_string()),
                    data: serde_json::Value::Null,
                },
                Node {
                    id: NodeId(30_002),
                    kind: NodeKind::Function,
                    name: "router".to_string(),
                    file: Some("src/lib.rs".to_string()),
                    data: serde_json::Value::Null,
                },
            ];
            for node in nodes {
                graph.add_node(&node).expect("failed to insert node");
            }
            graph
                .add_edge(&Edge::calls(NodeId(30_002), NodeId(30_000)))
                .expect("failed to insert incoming edge");
            graph
                .add_edge(&Edge::calls(NodeId(30_000), NodeId(30_002)))
                .expect("failed to insert outgoing edge");
        }

        let (summary, items) = expect_graph_context(
            glyph
                .handle_message(UiToCore::QueryGraphContext {
                    query: "handler".to_string(),
                    context_files: Some(vec!["src/lib.rs".to_string()]),
                    limit: Some(1),
                })
                .await,
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].node.name, "handler");
        assert!(items[0].score > 0.0);
        assert!(items[0].incoming >= 1);
        assert!(items[0].outgoing >= 1);
        assert!(summary.contains("returned 1 of"));
    }

    #[tokio::test]
    async fn test_query_graph_context_deterministic_order_with_file_scope() {
        let root = unique_temp_dir("graph_context_scope_order");
        let graph_path = root.join(".glyph").join("ckg.sqlite");
        let glyph = Glyph::new_with_graph_path(Some(&graph_path));

        {
            let graph = glyph.graph.lock().expect("failed to lock graph");
            let nodes = vec![
                Node {
                    id: NodeId(31_000),
                    kind: NodeKind::Function,
                    name: "target_zeta".to_string(),
                    file: Some("src/scope.rs".to_string()),
                    data: serde_json::Value::Null,
                },
                Node {
                    id: NodeId(31_001),
                    kind: NodeKind::Function,
                    name: "target_alpha".to_string(),
                    file: Some("src/scope.rs".to_string()),
                    data: serde_json::Value::Null,
                },
            ];
            for node in nodes {
                graph.add_node(&node).expect("failed to insert node");
            }
        }

        let first = expect_graph_context(
            glyph
                .handle_message(UiToCore::QueryGraphContext {
                    query: "".to_string(),
                    context_files: Some(vec!["src/scope.rs".to_string()]),
                    limit: None,
                })
                .await,
        )
        .1;
        let second = expect_graph_context(
            glyph
                .handle_message(UiToCore::QueryGraphContext {
                    query: "".to_string(),
                    context_files: Some(vec!["src/scope.rs".to_string()]),
                    limit: None,
                })
                .await,
        )
        .1;

        let first_order: Vec<(String, i64)> = first
            .iter()
            .map(|item| (item.node.name.clone(), item.node.id))
            .collect();
        let second_order: Vec<(String, i64)> = second
            .iter()
            .map(|item| (item.node.name.clone(), item.node.id))
            .collect();
        assert_eq!(first_order, second_order);
        assert_eq!(
            first_order,
            vec![
                ("target_alpha".to_string(), 31_001),
                ("target_zeta".to_string(), 31_000),
            ]
        );
    }

    #[tokio::test]
    async fn test_poisoned_graph_lock_maps_to_graph_query_failed() {
        let glyph = Glyph::new();
        let graph = glyph.graph.clone();
        let _ = std::thread::spawn(move || {
            let _guard = graph.lock().expect("failed to lock graph");
            panic!("poison graph lock");
        })
        .join();

        let response = glyph
            .handle_message(UiToCore::SearchGraph {
                query: "anything".to_string(),
                limit: None,
                offset: None,
                kind_filter: None,
                file_filter: None,
            })
            .await;
        assert!(matches!(
            response,
            CoreToUi::Error {
                code: ErrorCode::GraphQueryFailed,
                retryable: true,
                should_resync: false,
                ..
            }
        ));

        let context_response = glyph
            .handle_message(UiToCore::QueryGraphContext {
                query: "anything".to_string(),
                context_files: None,
                limit: None,
            })
            .await;
        assert!(matches!(
            context_response,
            CoreToUi::Error {
                code: ErrorCode::GraphQueryFailed,
                retryable: true,
                should_resync: false,
                ..
            }
        ));
    }
}
