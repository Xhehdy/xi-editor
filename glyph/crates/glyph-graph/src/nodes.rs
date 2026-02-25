//! Node types for the knowledge graph

use serde::{Deserialize, Serialize};

/// Unique node identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub i64);

impl NodeId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

/// Types of nodes in the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Function,
    Struct,
    Enum,
    Trait,
    Module,
    Type,
    Constant,
    Concept, // AI-extracted concept
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::File => "file",
            NodeKind::Function => "function",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
            NodeKind::Trait => "trait",
            NodeKind::Module => "module",
            NodeKind::Type => "type",
            NodeKind::Constant => "constant",
            NodeKind::Concept => "concept",
        }
    }
}

impl std::str::FromStr for NodeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file" => Ok(NodeKind::File),
            "function" => Ok(NodeKind::Function),
            "struct" => Ok(NodeKind::Struct),
            "enum" => Ok(NodeKind::Enum),
            "trait" => Ok(NodeKind::Trait),
            "module" => Ok(NodeKind::Module),
            "type" => Ok(NodeKind::Type),
            "constant" => Ok(NodeKind::Constant),
            "concept" => Ok(NodeKind::Concept),
            _ => Err(format!("unknown NodeKind: {}", s)),
        }
    }
}

/// A node in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub file: Option<String>,
    pub data: serde_json::Value,
}

impl Node {
    pub fn new_file(id: i64, path: &str) -> Self {
        Self {
            id: NodeId(id),
            kind: NodeKind::File,
            name: path.to_string(),
            file: Some(path.to_string()),
            data: serde_json::Value::Null,
        }
    }

    pub fn new_function(id: i64, name: &str, file: &str) -> Self {
        Self {
            id: NodeId(id),
            kind: NodeKind::Function,
            name: name.to_string(),
            file: Some(file.to_string()),
            data: serde_json::Value::Null,
        }
    }

    pub fn new_struct(id: i64, name: &str, file: &str) -> Self {
        Self {
            id: NodeId(id),
            kind: NodeKind::Struct,
            name: name.to_string(),
            file: Some(file.to_string()),
            data: serde_json::Value::Null,
        }
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }
}
