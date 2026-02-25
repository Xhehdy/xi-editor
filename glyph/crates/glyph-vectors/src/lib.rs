//! Glyph Vectors - Vector store abstraction for semantic search
//!
//! Supports multiple backends: Pinecone (cloud), Qdrant (local), InMemory (testing)

mod memory;
mod pinecone;

pub use memory::InMemoryStore;
pub use pinecone::PineconeStore;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Vector store errors
#[derive(Debug, Error)]
pub enum VectorError {
    #[error("Failed to connect: {0}")]
    ConnectionError(String),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid embedding dimension")]
    InvalidDimension,
}

/// A matched vector from a query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMatch {
    /// Vector ID
    pub id: String,
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Associated metadata
    pub metadata: serde_json::Value,
}

/// Abstract vector store trait
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Upsert a vector with metadata
    async fn upsert(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<(), VectorError>;

    /// Query for similar vectors
    async fn query(
        &self,
        embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<VectorMatch>, VectorError>;

    /// Delete a vector by ID
    async fn delete(&self, id: &str) -> Result<(), VectorError>;

    /// Check if the store is healthy/connected
    async fn health_check(&self) -> Result<bool, VectorError>;
}

/// Embedding dimension for common models
pub mod dimensions {
    /// OpenAI text-embedding-3-small
    pub const OPENAI_SMALL: usize = 1536;
    /// OpenAI text-embedding-3-large
    pub const OPENAI_LARGE: usize = 3072;
    /// Cohere embed-v3
    pub const COHERE_V3: usize = 1024;
    /// Local models (e.g., sentence-transformers)
    pub const LOCAL_384: usize = 384;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store() {
        let store = InMemoryStore::new(3);

        // Upsert
        store
            .upsert(
                "vec1",
                vec![1.0, 0.0, 0.0],
                serde_json::json!({"label": "x"}),
            )
            .await
            .unwrap();
        store
            .upsert(
                "vec2",
                vec![0.0, 1.0, 0.0],
                serde_json::json!({"label": "y"}),
            )
            .await
            .unwrap();
        store
            .upsert(
                "vec3",
                vec![0.7, 0.7, 0.0],
                serde_json::json!({"label": "xy"}),
            )
            .await
            .unwrap();

        // Query - should find vec1 and vec3 as closest to [1,0,0]
        let results = store.query(vec![1.0, 0.0, 0.0], 2).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "vec1"); // Exact match

        // Delete
        store.delete("vec1").await.unwrap();
        let results = store.query(vec![1.0, 0.0, 0.0], 2).await.unwrap();
        assert_ne!(results[0].id, "vec1");
    }
}
