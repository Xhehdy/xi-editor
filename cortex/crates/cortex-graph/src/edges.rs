//! Edge types for the knowledge graph

use crate::nodes::NodeId;
use serde::{Deserialize, Serialize};

/// Types of edges in the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    /// File contains a symbol
    Contains,
    /// Function calls another function
    Calls,
    /// File imports another file
    Imports,
    /// Type implements a trait
    Implements,
    /// Symbol has a type
    TypeOf,
    /// Module contains a module
    ChildOf,
    /// Symbol uses another symbol
    Uses,
    /// Symbol modifies another symbol
    Modifies,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Contains => "contains",
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Implements => "implements",
            EdgeKind::TypeOf => "type_of",
            EdgeKind::ChildOf => "child_of",
            EdgeKind::Uses => "uses",
            EdgeKind::Modifies => "modifies",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "contains" => EdgeKind::Contains,
            "calls" => EdgeKind::Calls,
            "imports" => EdgeKind::Imports,
            "implements" => EdgeKind::Implements,
            "type_of" => EdgeKind::TypeOf,
            "child_of" => EdgeKind::ChildOf,
            "uses" => EdgeKind::Uses,
            "modifies" => EdgeKind::Modifies,
            _ => EdgeKind::Uses, // Default fallback
        }
    }
}

/// An edge in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub data: serde_json::Value,
}

impl Edge {
    pub fn new(from: NodeId, to: NodeId, kind: EdgeKind) -> Self {
        Self {
            from,
            to,
            kind,
            data: serde_json::Value::Null,
        }
    }

    pub fn contains(container: NodeId, item: NodeId) -> Self {
        Self::new(container, item, EdgeKind::Contains)
    }

    pub fn calls(caller: NodeId, callee: NodeId) -> Self {
        Self::new(caller, callee, EdgeKind::Calls)
    }

    pub fn imports(importer: NodeId, imported: NodeId) -> Self {
        Self::new(importer, imported, EdgeKind::Imports)
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }
}
