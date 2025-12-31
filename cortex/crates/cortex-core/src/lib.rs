//! Cortex Core - Main coordinator for the Cortex IDE
//!
//! This is the brain that ties everything together.

pub mod editor;

use cortex_buffer::{Buffer, BufferId};
use cortex_ce::ComprehensionEngine;
use cortex_events::ViewId;
use cortex_graph::CodeGraph;
use cortex_llm::LlmClient;
use cortex_protocol::{CoreToUi, SymbolInfo, UiToCore};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tracing::{error, info};

use cortex_syntax::{SyntaxHighlighter, Language};

/// The main Cortex core state
pub struct Cortex {
    /// Active buffers
    buffers: Arc<RwLock<HashMap<BufferId, Buffer>>>,
    /// View to buffer mapping
    views: Arc<RwLock<HashMap<ViewId, BufferId>>>,
    /// Buffer language mapping
    languages: Arc<RwLock<HashMap<BufferId, Language>>>,
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

impl Cortex {
    /// Creates a new Cortex instance
    pub fn new() -> Self {
        // Use in-memory graph for now
        let graph = CodeGraph::in_memory().expect("Failed to create code graph");
        
        // Initialize LLM client (pointing to sidecar)
        let llm = LlmClient::new("http://127.0.0.1:8000");

        Self {
            buffers: Arc::new(RwLock::new(HashMap::new())),
            views: Arc::new(RwLock::new(HashMap::new())),
            languages: Arc::new(RwLock::new(HashMap::new())),
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
    async fn highlight(&self, content: &str, lang: Language) -> Vec<cortex_protocol::HighlightSpan> {
        let mut highlighter = self.highlighter.write().await;
        let spans = highlighter.highlight(content, lang).unwrap_or_default();
        
        // Convert from syntax span to protocol span
        spans.into_iter().map(|s| cortex_protocol::HighlightSpan {
            start: s.start,
            end: s.end,
            highlight: s.highlight,
        }).collect()
    }

    /// Handles an incoming message from the UI
    pub async fn handle_message(&self, msg: UiToCore) -> CoreToUi {
        match msg {
            UiToCore::Hello { client_version } => {
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
                    let ext = std::path::Path::new(p).extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    Language::from_extension(ext)
                } else {
                    Language::Plain
                };

                // Load content from path or create empty
                let content = if let Some(ref p) = path {
                    std::fs::read_to_string(p).unwrap_or_default()
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

                info!("Created view {:?} for buffer {:?} (Lang: {:?})", view_id, buffer_id, lang);
                
                // Obstruct content for ViewCreated (protocol defines it but maybe we shouldn't send full content twice? 
                // Protocol says ViewCreated has content. Let's send it.)
                // But ViewCreated struct in protocol doesn't have spans?
                // Wait, ViewCreated in protocol: ViewCreated { view_id, content }
                // So we send ViewCreated FIRST.
                // THEN we should probably send ApplyHighlights?
                // Or maybe I should have updated ViewCreated to include spans? 
                // Implementation plan said: Add HighlightSpan to CoreToUi::SetContent, Add ApplyHighlights.
                // User didn't specify changing ViewCreated.
                // So I will send ApplyHighlights immediately after? 
                // But handle_message returns ONE message.
                // I'll send SetContent instead of ViewCreated? No, ViewCreated implies new view ID.
                // For now, I'll return ViewCreated. The UI will request content/highlights or we'll rely on a separate event.
                // Actually, let's change HandleMessage to return SetContent? No.
                // Wait, the UI usually requests content or gets it via ViewCreated.
                // Let's stick to returning ViewCreated. The UI can ask for highlights, OR 
                // I can verify if I can send multiple messages? No, request-response.
                // I'll stick to 1-to-1 for now.
                // If I want highlights immediately, I should have added spans to ViewCreated.
                // But I didn't in step 1.
                // Let's modify handle_message to return SetContent for GetContent request which will include spans.
                
                CoreToUi::ViewCreated {
                    view_id,
                    content,
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
                    }
                    info!("Closed view {:?}", view_id);
                }
                CoreToUi::Event(cortex_events::CoreEvent::BufferChanged { view_id })
            }

            UiToCore::Input { view_id, event } => {
                // Get buffer ID for this view
                let buffer_id = {
                    let views = self.views.read().await;
                    views.get(&view_id).copied()
                };

                if let Some(buffer_id) = buffer_id {
                    let mut buffers = self.buffers.write().await;
                    if let Some(buffer) = buffers.get_mut(&buffer_id) {
                        // Handle the input event
                        match event {
                            cortex_events::InputEvent::Insert { text } => {
                                let len = buffer.len();
                                let _ = buffer.edit(len..len, &text);
                            }
                            cortex_events::InputEvent::Undo => {
                                buffer.undo();
                            }
                            cortex_events::InputEvent::Redo => {
                                buffer.redo();
                            }
                            _ => {
                                // TODO: Handle other events
                            }
                        }
                    }
                }

                CoreToUi::Event(cortex_events::CoreEvent::BufferChanged { view_id })
            }

            UiToCore::GetContent { view_id } => {
                let (buffer_id, lang) = {
                    let views = self.views.read().await;
                    if let Some(&bid) = views.get(&view_id) {
                        let langs = self.languages.read().await;
                        (Some(bid), langs.get(&bid).copied().unwrap_or(Language::Plain))
                    } else {
                        (None, Language::Plain)
                    }
                };

                if let Some(buffer_id) = buffer_id {
                    let buffers = self.buffers.read().await;
                    if let Some(buffer) = buffers.get(&buffer_id) {
                        let content = buffer.content(); 
                        let spans = self.highlight(&content, lang).await;
                        
                        return CoreToUi::SetContent {
                            view_id,
                            content,
                            spans,
                        };
                    }
                }

                CoreToUi::Error {
                    message: format!("View {:?} not found", view_id),
                }
            }

            UiToCore::Chat { message, context_files } => {
                // Build context (simplified for now)
                let context = if !context_files.is_empty() {
                    Some(format!("User is looking at specific files: {:?}", context_files))
                } else {
                    None
                };

                match self.llm.complete(&message, context.as_deref()).await {
                    Ok(response) => CoreToUi::ChatToken {
                        token: response,
                        done: true,
                    },
                    Err(e) => CoreToUi::Error {
                        message: format!("AI Error: {}", e),
                    },
                }
            }

            UiToCore::RequestCompletion { view_id, .. } => {
                // Mock ghost text
                CoreToUi::GhostText {
                    view_id,
                    text: " // AI Completion".to_string(), 
                }
            }

            UiToCore::IndexFile { path } => {
                // Read file and index it
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        return CoreToUi::Error {
                            message: format!("Failed to read file: {}", e),
                        };
                    }
                };

                // Index with CE
                let mut ce = self.ce.write().await;
                let path_buf = Path::new(&path);
                match ce.index_file(path_buf, &content) {
                    Ok(symbol_ids) => {
                        info!("Indexed {} symbols from {}", symbol_ids.len(), path);
                        
                        // Also add to graph
                        let file_node = cortex_graph::Node::new_file(
                            self.next_id() as i64,
                            &path,
                        );
                        if let Ok(mut graph) = self.graph.lock() {
                             let _ = graph.add_node(&file_node);
                        } else {
                            error!("Failed to lock graph");
                        }

                        CoreToUi::FileIndexed {
                            path,
                            symbol_count: symbol_ids.len(),
                        }
                    }
                    Err(e) => CoreToUi::Error {
                        message: format!("Indexing failed: {}", e),
                    },
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

                CoreToUi::Symbols { symbols: symbol_infos }
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

                CoreToUi::Symbols { symbols: symbol_infos }
            }
        }
    }
}

impl Default for Cortex {
    fn default() -> Self {
        Self::new()
    }
}
