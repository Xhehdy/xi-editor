//! Glyph Graph - Code Knowledge Graph with SQLite persistence
//!
//! Stores and queries relationships between code elements.

mod edges;
mod nodes;
mod query;
mod storage;

pub use edges::{Edge, EdgeKind};
pub use nodes::{Node, NodeId, NodeKind};
pub use query::QueryBuilder;

use rusqlite::{Connection, OptionalExtension};
use std::path::Path;
use thiserror::Error;
use tracing::info;

/// Graph errors
#[derive(Debug, Error)]
pub enum GraphError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Node not found: {0}")]
    NodeNotFound(NodeId),
    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}

/// The Code Knowledge Graph
pub struct CodeGraph {
    conn: Connection,
}

impl CodeGraph {
    /// Create an in-memory graph (for testing)
    pub fn in_memory() -> Result<Self, GraphError> {
        let conn = Connection::open_in_memory()?;
        let graph = Self { conn };
        graph.init_schema()?;
        Ok(graph)
    }

    /// Open or create a graph at the given path
    pub fn open(path: &Path) -> Result<Self, GraphError> {
        let conn = Connection::open(path)?;
        let graph = Self { conn };
        graph.init_schema()?;
        info!("Opened graph at {:?}", path);
        Ok(graph)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<(), GraphError> {
        self.conn.execute_batch(storage::SCHEMA)?;
        Ok(())
    }

    /// Add a node to the graph
    pub fn add_node(&self, node: &Node) -> Result<NodeId, GraphError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO nodes (id, kind, name, file, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                node.id.0,
                node.kind.as_str(),
                node.name,
                node.file,
                serde_json::to_string(&node.data).unwrap_or_default(),
            ],
        )?;
        Ok(node.id)
    }

    /// Add an edge to the graph
    pub fn add_edge(&self, edge: &Edge) -> Result<(), GraphError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO edges (from_id, to_id, kind, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                edge.from.0,
                edge.to.0,
                edge.kind.as_str(),
                serde_json::to_string(&edge.data).unwrap_or_default(),
            ],
        )?;
        Ok(())
    }

    /// Get a node by ID
    pub fn get_node(&self, id: NodeId) -> Result<Option<Node>, GraphError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, name, file, data FROM nodes WHERE id = ?1")?;

        let node = stmt
            .query_row([id.0], |row| {
                Ok(Node {
                    id: NodeId(row.get(0)?),
                    kind: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or(NodeKind::Concept),
                    name: row.get(2)?,
                    file: row.get(3)?,
                    data: serde_json::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })
            .optional()?;

        Ok(node)
    }

    /// Get all nodes of a kind
    pub fn get_nodes_by_kind(&self, kind: NodeKind) -> Result<Vec<Node>, GraphError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, name, file, data FROM nodes WHERE kind = ?1")?;

        let nodes = stmt
            .query_map([kind.as_str()], |row| {
                Ok(Node {
                    id: NodeId(row.get(0)?),
                    kind: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or(NodeKind::Concept),
                    name: row.get(2)?,
                    file: row.get(3)?,
                    data: serde_json::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get outgoing edges from a node
    pub fn get_outgoing(&self, from: NodeId) -> Result<Vec<Edge>, GraphError> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_id, to_id, kind, data FROM edges WHERE from_id = ?1")?;

        let edges = stmt
            .query_map([from.0], |row| {
                Ok(Edge {
                    from: NodeId(row.get(0)?),
                    to: NodeId(row.get(1)?),
                    kind: row.get::<_, String>(2)?.parse().unwrap_or(EdgeKind::Uses),
                    data: serde_json::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(edges)
    }

    /// Get incoming edges to a node
    pub fn get_incoming(&self, to: NodeId) -> Result<Vec<Edge>, GraphError> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_id, to_id, kind, data FROM edges WHERE to_id = ?1")?;

        let edges = stmt
            .query_map([to.0], |row| {
                Ok(Edge {
                    from: NodeId(row.get(0)?),
                    to: NodeId(row.get(1)?),
                    kind: row.get::<_, String>(2)?.parse().unwrap_or(EdgeKind::Uses),
                    data: serde_json::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(edges)
    }

    /// Get neighbors of a node (outgoing edges resolved to nodes)
    pub fn get_neighbors(
        &self,
        from: NodeId,
        edge_kind: Option<EdgeKind>,
    ) -> Result<Vec<Node>, GraphError> {
        let edges = self.get_outgoing(from)?;

        let filtered: Vec<_> = match edge_kind {
            Some(kind) => edges.into_iter().filter(|e| e.kind == kind).collect(),
            None => edges,
        };

        let mut neighbors = Vec::new();
        for edge in filtered {
            if let Some(node) = self.get_node(edge.to)? {
                neighbors.push(node);
            }
        }

        Ok(neighbors)
    }

    /// Find nodes by name (partial match)
    pub fn find_by_name(&self, name: &str) -> Result<Vec<Node>, GraphError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, name, file, data FROM nodes WHERE name LIKE ?1")?;

        let pattern = format!("%{}%", name);
        let nodes = stmt
            .query_map([pattern], |row| {
                Ok(Node {
                    id: NodeId(row.get(0)?),
                    kind: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or(NodeKind::Concept),
                    name: row.get(2)?,
                    file: row.get(3)?,
                    data: serde_json::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get nodes in a file
    pub fn get_nodes_in_file(&self, file: &str) -> Result<Vec<Node>, GraphError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, name, file, data FROM nodes WHERE file = ?1")?;

        let nodes = stmt
            .query_map([file], |row| {
                Ok(Node {
                    id: NodeId(row.get(0)?),
                    kind: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or(NodeKind::Concept),
                    name: row.get(2)?,
                    file: row.get(3)?,
                    data: serde_json::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Delete all nodes for a file (for re-indexing)
    pub fn delete_file(&self, file: &str) -> Result<usize, GraphError> {
        // First delete edges involving nodes from this file
        self.conn.execute(
            "DELETE FROM edges WHERE from_id IN (SELECT id FROM nodes WHERE file = ?1)
             OR to_id IN (SELECT id FROM nodes WHERE file = ?1)",
            [file],
        )?;

        // Then delete the nodes
        let deleted = self
            .conn
            .execute("DELETE FROM nodes WHERE file = ?1", [file])?;

        Ok(deleted)
    }

    /// Get total node count
    pub fn node_count(&self) -> Result<usize, GraphError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Get total edge count
    pub fn edge_count(&self) -> Result<usize, GraphError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_creation() {
        let graph = CodeGraph::in_memory().unwrap();
        assert_eq!(graph.node_count().unwrap(), 0);
    }

    #[test]
    fn test_add_and_get_node() {
        let graph = CodeGraph::in_memory().unwrap();

        let node = Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "test_fn".to_string(),
            file: Some("test.rs".to_string()),
            data: serde_json::json!({"line": 42}),
        };

        graph.add_node(&node).unwrap();

        let retrieved = graph.get_node(NodeId(1)).unwrap().unwrap();
        assert_eq!(retrieved.name, "test_fn");
    }

    #[test]
    fn test_add_edge_and_get_neighbors() {
        let graph = CodeGraph::in_memory().unwrap();

        let fn1 = Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "caller".to_string(),
            file: Some("test.rs".to_string()),
            data: serde_json::Value::Null,
        };

        let fn2 = Node {
            id: NodeId(2),
            kind: NodeKind::Function,
            name: "callee".to_string(),
            file: Some("test.rs".to_string()),
            data: serde_json::Value::Null,
        };

        graph.add_node(&fn1).unwrap();
        graph.add_node(&fn2).unwrap();

        let edge = Edge {
            from: NodeId(1),
            to: NodeId(2),
            kind: EdgeKind::Calls,
            data: serde_json::Value::Null,
        };

        graph.add_edge(&edge).unwrap();

        let neighbors = graph
            .get_neighbors(NodeId(1), Some(EdgeKind::Calls))
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].name, "callee");
    }
}
