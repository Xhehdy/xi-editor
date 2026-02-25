//! Symbol types and symbol table

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique symbol identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub u64);

static NEXT_SYMBOL_ID: AtomicU64 = AtomicU64::new(1);

impl SymbolId {
    pub fn new() -> Self {
        Self(NEXT_SYMBOL_ID.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for SymbolId {
    fn default() -> Self {
        Self::new()
    }
}

/// Types of code symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Module,
    Constant,
    Variable,
    Type,
    Macro,
    Import,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Module => "module",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Type => "type",
            SymbolKind::Macro => "macro",
            SymbolKind::Import => "import",
        }
    }
}

/// A code symbol (function, struct, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: Option<String>,
    pub doc: Option<String>,
}

impl Symbol {
    /// Get a short description of the symbol
    pub fn short_desc(&self) -> String {
        format!(
            "{} {} at {:?}:{}",
            self.kind.as_str(),
            self.name,
            self.file.file_name().unwrap_or_default(),
            self.start_line
        )
    }
}

/// Symbol table with multiple indexes
pub struct SymbolTable {
    symbols: HashMap<SymbolId, Symbol>,
    by_name: HashMap<String, Vec<SymbolId>>,
    by_file: HashMap<PathBuf, Vec<SymbolId>>,
    by_kind: HashMap<SymbolKind, Vec<SymbolId>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
            by_name: HashMap::new(),
            by_file: HashMap::new(),
            by_kind: HashMap::new(),
        }
    }

    /// Insert a symbol into the table
    pub fn insert(&mut self, symbol: Symbol) {
        let id = symbol.id;
        let name = symbol.name.clone();
        let file = symbol.file.clone();
        let kind = symbol.kind;

        self.symbols.insert(id, symbol);

        self.by_name.entry(name).or_default().push(id);
        self.by_file.entry(file).or_default().push(id);
        self.by_kind.entry(kind).or_default().push(id);
    }

    /// Get a symbol by ID
    pub fn get(&self, id: &SymbolId) -> Option<&Symbol> {
        self.symbols.get(id)
    }

    /// Find symbols by name
    pub fn find_by_name(&self, name: &str) -> Vec<&Symbol> {
        self.by_name
            .get(name)
            .map(|ids| ids.iter().filter_map(|id| self.symbols.get(id)).collect())
            .unwrap_or_default()
    }

    /// Find symbols in a file
    pub fn find_by_file(&self, file: &PathBuf) -> Vec<&Symbol> {
        self.by_file
            .get(file)
            .map(|ids| ids.iter().filter_map(|id| self.symbols.get(id)).collect())
            .unwrap_or_default()
    }

    /// Find symbols by kind
    pub fn find_by_kind(&self, kind: SymbolKind) -> Vec<&Symbol> {
        self.by_kind
            .get(&kind)
            .map(|ids| ids.iter().filter_map(|id| self.symbols.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get total count
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Remove symbols for a file (for re-indexing)
    pub fn remove_file(&mut self, file: &PathBuf) -> Vec<SymbolId> {
        let mut removed_ids = Vec::new();

        if let Some(ids) = self.by_file.remove(file) {
            for id in ids {
                if let Some(symbol) = self.symbols.remove(&id) {
                    removed_ids.push(id);

                    // Clean up other indexes
                    let mut remove_name_key = false;
                    if let Some(name_ids) = self.by_name.get_mut(&symbol.name) {
                        name_ids.retain(|&i| i != id);
                        remove_name_key = name_ids.is_empty();
                    }
                    if remove_name_key {
                        self.by_name.remove(&symbol.name);
                    }

                    let mut remove_kind_key = false;
                    if let Some(kind_ids) = self.by_kind.get_mut(&symbol.kind) {
                        kind_ids.retain(|&i| i != id);
                        remove_kind_key = kind_ids.is_empty();
                    }
                    if remove_kind_key {
                        self.by_kind.remove(&symbol.kind);
                    }
                }
            }
        }

        removed_ids
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table() {
        let mut table = SymbolTable::new();

        let symbol = Symbol {
            id: SymbolId::new(),
            name: "test_fn".to_string(),
            kind: SymbolKind::Function,
            file: PathBuf::from("test.rs"),
            start_line: 1,
            end_line: 10,
            signature: Some("fn test_fn() -> bool".to_string()),
            doc: None,
        };

        table.insert(symbol);

        assert_eq!(table.len(), 1);
        assert_eq!(table.find_by_name("test_fn").len(), 1);
        assert_eq!(table.find_by_kind(SymbolKind::Function).len(), 1);
    }

    #[test]
    fn test_remove_file_cleans_indexes() {
        let mut table = SymbolTable::new();
        let file_a = PathBuf::from("a.rs");
        let file_b = PathBuf::from("b.rs");

        table.insert(Symbol {
            id: SymbolId::new(),
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            file: file_a.clone(),
            start_line: 1,
            end_line: 1,
            signature: None,
            doc: None,
        });
        table.insert(Symbol {
            id: SymbolId::new(),
            name: "bar".to_string(),
            kind: SymbolKind::Function,
            file: file_a.clone(),
            start_line: 2,
            end_line: 2,
            signature: None,
            doc: None,
        });
        table.insert(Symbol {
            id: SymbolId::new(),
            name: "baz".to_string(),
            kind: SymbolKind::Function,
            file: file_b.clone(),
            start_line: 1,
            end_line: 1,
            signature: None,
            doc: None,
        });

        let removed = table.remove_file(&file_a);
        assert_eq!(removed.len(), 2);
        assert!(table.find_by_file(&file_a).is_empty());
        assert_eq!(table.find_by_file(&file_b).len(), 1);
        assert_eq!(table.len(), 1);
    }
}
