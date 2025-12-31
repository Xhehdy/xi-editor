//! Cortex Syntax - Tree-sitter powered syntax highlighting
//!
//! Provides semantic syntax highlighting for multiple languages.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

/// Syntax highlighting errors
#[derive(Debug, Error)]
pub enum SyntaxError {
    #[error("Unknown language: {0}")]
    UnknownLanguage(String),
    #[error("Highlighting failed: {0}")]
    HighlightFailed(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Supported languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    JavaScript,
    Python,
    Json,
    Plain,
}

impl Language {
    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "js" | "jsx" | "ts" | "tsx" => Language::JavaScript,
            "py" | "pyw" => Language::Python,
            "json" => Language::Json,
            _ => Language::Plain,
        }
    }
    
    /// Get the tree-sitter language
    fn tree_sitter_language(&self) -> Option<tree_sitter::Language> {
        match self {
            Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
            Language::Json => Some(tree_sitter_json::LANGUAGE.into()),
            Language::Plain => None,
        }
    }
}

/// Standard highlight names (maps to theme colors)
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "keyword",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// A highlighted region in the text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub highlight: String,
}

/// The syntax highlighter
pub struct SyntaxHighlighter {
    highlighter: Highlighter,
    configs: std::collections::HashMap<Language, HighlightConfiguration>,
}

impl SyntaxHighlighter {
    /// Creates a new syntax highlighter
    pub fn new() -> Result<Self, SyntaxError> {
        let mut configs = std::collections::HashMap::new();
        
        // Configure Rust
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            configs.insert(Language::Rust, config);
        }
        
        // Configure JavaScript
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ) {
            config.configure(HIGHLIGHT_NAMES);
            configs.insert(Language::JavaScript, config);
        }
        
        // Configure Python
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            configs.insert(Language::Python, config);
        }
        
        // Configure JSON
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            configs.insert(Language::Json, config);
        }
        
        Ok(Self {
            highlighter: Highlighter::new(),
            configs,
        })
    }
    
    /// Highlights source code and returns spans
    pub fn highlight(&mut self, source: &str, lang: Language) -> Result<Vec<HighlightSpan>, SyntaxError> {
        let config = match self.configs.get(&lang) {
            Some(c) => c,
            None => return Ok(vec![]), // Plain text, no highlighting
        };
        
        let mut spans = Vec::new();
        
        let events = self.highlighter
            .highlight(config, source.as_bytes(), None, |_| None)
            .map_err(|e| SyntaxError::HighlightFailed(format!("{:?}", e)))?;
        
        let mut current_highlight: Option<usize> = None;
        let mut byte_offset = 0;
        
        for event in events {
            match event.map_err(|e| SyntaxError::HighlightFailed(format!("{:?}", e)))? {
                HighlightEvent::Source { start, end } => {
                    if let Some(hl_idx) = current_highlight {
                        if hl_idx < HIGHLIGHT_NAMES.len() {
                            spans.push(HighlightSpan {
                                start,
                                end,
                                highlight: HIGHLIGHT_NAMES[hl_idx].to_string(),
                            });
                        }
                    }
                    byte_offset = end;
                }
                HighlightEvent::HighlightStart(Highlight(idx)) => {
                    current_highlight = Some(idx);
                }
                HighlightEvent::HighlightEnd => {
                    current_highlight = None;
                }
            }
        }
        
        Ok(spans)
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new().expect("Failed to create syntax highlighter")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rust_highlighting() {
        let mut highlighter = SyntaxHighlighter::new().unwrap();
        let code = r#"fn main() { println!("Hello"); }"#;
        let spans = highlighter.highlight(code, Language::Rust).unwrap();
        
        // Should have some highlights
        assert!(!spans.is_empty());
        
        // "fn" should be a keyword
        let fn_span = spans.iter().find(|s| s.start == 0 && s.end == 2);
        assert!(fn_span.is_some());
    }
    
    #[test]
    fn test_language_detection() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("js"), Language::JavaScript);
        assert_eq!(Language::from_extension("txt"), Language::Plain);
    }
}
