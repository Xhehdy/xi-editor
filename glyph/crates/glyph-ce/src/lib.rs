//! Glyph Comprehension Engine - Semantic understanding of codebases
//!
//! Extracts symbols, builds understanding, and provides context for agents.

mod summary;
mod symbols;

pub use summary::Summarizer;
pub use symbols::{Symbol, SymbolId, SymbolKind, SymbolTable};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::info;

/// CE errors
#[derive(Debug, Error)]
pub enum CEError {
    #[error("Failed to parse file: {0}")]
    ParseError(String),
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),
    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),
}

/// The main Comprehension Engine
pub struct ComprehensionEngine {
    symbol_table: SymbolTable,
    summaries: HashMap<SymbolId, String>,
    indexed_files: HashMap<PathBuf, Vec<SymbolId>>,
}

impl ComprehensionEngine {
    pub fn new() -> Self {
        Self {
            symbol_table: SymbolTable::new(),
            summaries: HashMap::new(),
            indexed_files: HashMap::new(),
        }
    }

    /// Index a file and extract its symbols
    pub fn index_file(&mut self, path: &Path, content: &str) -> Result<Vec<SymbolId>, CEError> {
        use glyph_syntax::Language;

        let path_buf = path.to_path_buf();

        // Remove stale symbol data before re-indexing.
        let removed_ids = self.symbol_table.remove_file(&path_buf);
        for id in removed_ids {
            self.summaries.remove(&id);
        }
        self.indexed_files.remove(&path_buf);

        // Detect language
        let lang = path
            .extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Plain);

        if matches!(lang, Language::Plain) {
            // Skip plain text files
            return Ok(vec![]);
        }

        // Extract symbols using tree-sitter
        let symbols = self.extract_symbols(path, content, lang)?;

        // Store in symbol table
        let symbol_ids: Vec<SymbolId> = symbols.iter().map(|s| s.id).collect();

        for symbol in symbols {
            self.symbol_table.insert(symbol);
        }

        // Track indexed file
        self.indexed_files.insert(path_buf, symbol_ids.clone());

        info!("Indexed {} symbols from {:?}", symbol_ids.len(), path);
        Ok(symbol_ids)
    }

    /// Extract symbols from code
    fn extract_symbols(
        &self,
        path: &Path,
        content: &str,
        lang: glyph_syntax::Language,
    ) -> Result<Vec<Symbol>, CEError> {
        use glyph_syntax::SyntaxHighlighter;

        let mut highlighter =
            SyntaxHighlighter::new().map_err(|e| CEError::ParseError(e.to_string()))?;

        let spans = highlighter
            .highlight(content, lang)
            .map_err(|e| CEError::ParseError(e.to_string()))?;

        // Convert highlight spans to symbols
        let mut symbols = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for span in spans {
            // Only extract function and type symbols
            let kind = match span.highlight.as_str() {
                "function" => Some(SymbolKind::Function),
                "type" => Some(SymbolKind::Type),
                "keyword" if is_definition_keyword(&span, content) => {
                    // Skip keywords that aren't definitions
                    None
                }
                _ => None,
            };

            if let Some(kind) = kind {
                let name = &content[span.start..span.end];
                let line = content[..span.start].matches('\n').count() + 1;

                symbols.push(Symbol {
                    id: SymbolId::new(),
                    name: name.to_string(),
                    kind,
                    file: path.to_path_buf(),
                    start_line: line,
                    end_line: line, // Simplified - would need more parsing
                    signature: extract_signature(&lines, line),
                    doc: extract_doc_comment(&lines, line),
                });
            }
        }

        Ok(symbols)
    }

    /// Get all symbols in a file
    pub fn symbols_in_file(&self, path: &Path) -> Vec<&Symbol> {
        self.indexed_files
            .get(path)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.symbol_table.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find symbols by name
    pub fn find_by_name(&self, name: &str) -> Vec<&Symbol> {
        self.symbol_table.find_by_name(name)
    }

    /// Get a symbol by ID
    pub fn get_symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.symbol_table.get(id)
    }

    /// Get or generate a summary for a symbol
    pub fn get_summary(&mut self, id: &SymbolId) -> Option<String> {
        if let Some(summary) = self.summaries.get(id) {
            return Some(summary.clone());
        }

        // Generate summary from symbol
        if let Some(symbol) = self.symbol_table.get(id) {
            let summary = Summarizer::summarize_symbol(symbol);
            self.summaries.insert(*id, summary.clone());
            return Some(summary);
        }

        None
    }

    /// Get context for a specific line in a file
    pub fn get_context(&self, path: &Path, line: usize) -> Vec<&Symbol> {
        self.symbols_in_file(path)
            .into_iter()
            .filter(|s| s.start_line <= line && s.end_line >= line)
            .collect()
    }

    /// Get all indexed files
    pub fn indexed_files(&self) -> Vec<&PathBuf> {
        self.indexed_files.keys().collect()
    }

    /// Get total symbol count
    pub fn symbol_count(&self) -> usize {
        self.symbol_table.len()
    }
}

impl Default for ComprehensionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a keyword span represents a definition
fn is_definition_keyword(span: &glyph_syntax::HighlightSpan, content: &str) -> bool {
    let keyword = &content[span.start..span.end];
    matches!(
        keyword,
        "fn" | "struct" | "enum" | "trait" | "impl" | "const" | "static" | "type" | "mod"
    )
}

/// Extract signature from lines around a definition
fn extract_signature(lines: &[&str], line: usize) -> Option<String> {
    if line > 0 && line <= lines.len() {
        let line_content = lines[line - 1].trim();
        // Simple extraction - just take the line
        if !line_content.is_empty() {
            return Some(line_content.to_string());
        }
    }
    None
}

/// Extract doc comment above a definition
fn extract_doc_comment(lines: &[&str], line: usize) -> Option<String> {
    if line <= 1 {
        return None;
    }

    let mut doc_lines = Vec::new();
    let mut current = line - 2; // Line before the definition

    while current < lines.len() {
        let content = lines[current].trim();
        if content.starts_with("///") {
            doc_lines.push(content.trim_start_matches("///").trim());
        } else if content.starts_with("//!") {
            doc_lines.push(content.trim_start_matches("//!").trim());
        } else if !content.is_empty() {
            break;
        }

        if current == 0 {
            break;
        }
        current -= 1;
    }

    if doc_lines.is_empty() {
        None
    } else {
        doc_lines.reverse();
        Some(doc_lines.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ce_creation() {
        let ce = ComprehensionEngine::new();
        assert_eq!(ce.symbol_count(), 0);
    }
}
