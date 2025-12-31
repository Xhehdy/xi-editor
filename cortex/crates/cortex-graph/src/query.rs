//! Query builder for the knowledge graph

use crate::{CodeGraph, Edge, EdgeKind, GraphError, Node, NodeId, NodeKind};

/// Fluent query builder
pub struct QueryBuilder<'a> {
    graph: &'a CodeGraph,
    start: Option<NodeId>,
    edge_kind: Option<EdgeKind>,
    node_kind: Option<NodeKind>,
    depth: usize,
}

impl<'a> QueryBuilder<'a> {
    pub fn new(graph: &'a CodeGraph) -> Self {
        Self {
            graph,
            start: None,
            edge_kind: None,
            node_kind: None,
            depth: 1,
        }
    }

    /// Start from a specific node
    pub fn from(mut self, node_id: NodeId) -> Self {
        self.start = Some(node_id);
        self
    }

    /// Filter by edge kind
    pub fn via(mut self, edge_kind: EdgeKind) -> Self {
        self.edge_kind = Some(edge_kind);
        self
    }

    /// Filter results by node kind
    pub fn of_kind(mut self, node_kind: NodeKind) -> Self {
        self.node_kind = Some(node_kind);
        self
    }

    /// Set traversal depth
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Execute query and get nodes
    pub fn nodes(self) -> Result<Vec<Node>, GraphError> {
        let Some(start) = self.start else {
            return Err(GraphError::InvalidQuery("No start node specified".into()));
        };

        let mut result = self.graph.get_neighbors(start, self.edge_kind)?;

        // Filter by node kind if specified
        if let Some(kind) = self.node_kind {
            result.retain(|n| n.kind == kind);
        }

        Ok(result)
    }

    /// Execute query and get edges
    pub fn edges(self) -> Result<Vec<Edge>, GraphError> {
        let Some(start) = self.start else {
            return Err(GraphError::InvalidQuery("No start node specified".into()));
        };

        let mut edges = self.graph.get_outgoing(start)?;

        // Filter by edge kind if specified
        if let Some(kind) = self.edge_kind {
            edges.retain(|e| e.kind == kind);
        }

        Ok(edges)
    }
}

impl CodeGraph {
    /// Start a query
    pub fn query(&self) -> QueryBuilder<'_> {
        QueryBuilder::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_builder() {
        let graph = CodeGraph::in_memory().unwrap();
        
        let file = Node::new_file(1, "test.rs");
        let func = Node::new_function(2, "test_fn", "test.rs");
        
        graph.add_node(&file).unwrap();
        graph.add_node(&func).unwrap();
        graph.add_edge(&Edge::contains(NodeId(1), NodeId(2))).unwrap();

        let functions = graph.query()
            .from(NodeId(1))
            .via(EdgeKind::Contains)
            .of_kind(NodeKind::Function)
            .nodes()
            .unwrap();

        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "test_fn");
    }
}
