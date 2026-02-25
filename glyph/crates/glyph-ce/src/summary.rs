//! Semantic summarization of code symbols

use crate::symbols::{Symbol, SymbolKind};

/// Generates summaries for symbols
pub struct Summarizer;

impl Summarizer {
    /// Generate a summary for a symbol
    pub fn summarize_symbol(symbol: &Symbol) -> String {
        let kind_desc = match symbol.kind {
            SymbolKind::Function => "A function",
            SymbolKind::Struct => "A struct",
            SymbolKind::Enum => "An enum",
            SymbolKind::Trait => "A trait",
            SymbolKind::Module => "A module",
            SymbolKind::Constant => "A constant",
            SymbolKind::Variable => "A variable",
            SymbolKind::Type => "A type alias",
            SymbolKind::Macro => "A macro",
            SymbolKind::Import => "An import",
        };

        let mut summary = format!("{} named `{}`", kind_desc, symbol.name);

        // Add signature info
        if let Some(ref sig) = symbol.signature {
            summary.push_str(&format!(" with signature: `{}`", sig));
        }

        // Add doc comment
        if let Some(ref doc) = symbol.doc {
            summary.push_str(&format!(". Documentation: {}", doc));
        }

        // Add location
        summary.push_str(&format!(
            ". Located in {:?} at line {}.",
            symbol.file.file_name().unwrap_or_default(),
            symbol.start_line
        ));

        summary
    }

    /// Generate a file summary from its symbols
    pub fn summarize_file(symbols: &[&Symbol]) -> String {
        if symbols.is_empty() {
            return "Empty or unparseable file.".to_string();
        }

        let mut parts = Vec::new();

        // Count by kind
        let funcs: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        let structs: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        let enums: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Enum)
            .collect();
        let traits: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Trait)
            .collect();

        if !funcs.is_empty() {
            let names: Vec<_> = funcs.iter().take(5).map(|s| s.name.as_str()).collect();
            if funcs.len() > 5 {
                parts.push(format!(
                    "{} functions including: {}, and {} more",
                    funcs.len(),
                    names.join(", "),
                    funcs.len() - 5
                ));
            } else {
                parts.push(format!("{} functions: {}", funcs.len(), names.join(", ")));
            }
        }

        if !structs.is_empty() {
            let names: Vec<_> = structs.iter().take(5).map(|s| s.name.as_str()).collect();
            parts.push(format!("{} structs: {}", structs.len(), names.join(", ")));
        }

        if !enums.is_empty() {
            let names: Vec<_> = enums.iter().take(5).map(|s| s.name.as_str()).collect();
            parts.push(format!("{} enums: {}", enums.len(), names.join(", ")));
        }

        if !traits.is_empty() {
            let names: Vec<_> = traits.iter().take(5).map(|s| s.name.as_str()).collect();
            parts.push(format!("{} traits: {}", traits.len(), names.join(", ")));
        }

        if parts.is_empty() {
            format!("File containing {} symbols", symbols.len())
        } else {
            format!("File containing: {}", parts.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::SymbolId;
    use std::path::PathBuf;

    #[test]
    fn test_summarize_symbol() {
        let symbol = Symbol {
            id: SymbolId::new(),
            name: "process_data".to_string(),
            kind: SymbolKind::Function,
            file: PathBuf::from("processor.rs"),
            start_line: 42,
            end_line: 60,
            signature: Some("fn process_data(input: &str) -> Result<Data, Error>".to_string()),
            doc: Some("Processes raw input data and returns structured output.".to_string()),
        };

        let summary = Summarizer::summarize_symbol(&symbol);

        assert!(summary.contains("function"));
        assert!(summary.contains("process_data"));
        assert!(summary.contains("line 42"));
    }
}
