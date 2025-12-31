//! In-memory vector store for testing and development

use crate::{VectorError, VectorMatch, VectorStore};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// In-memory vector store (for testing)
pub struct InMemoryStore {
    dimension: usize,
    vectors: RwLock<HashMap<String, StoredVector>>,
}

struct StoredVector {
    embedding: Vec<f32>,
    metadata: serde_json::Value,
}

impl InMemoryStore {
    /// Create a new in-memory store with given dimension
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            vectors: RwLock::new(HashMap::new()),
        }
    }

    /// Compute cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
}

#[async_trait]
impl VectorStore for InMemoryStore {
    async fn upsert(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<(), VectorError> {
        if embedding.len() != self.dimension {
            return Err(VectorError::InvalidDimension);
        }

        let mut vectors = self.vectors.write().await;
        vectors.insert(
            id.to_string(),
            StoredVector { embedding, metadata },
        );
        Ok(())
    }

    async fn query(
        &self,
        embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<VectorMatch>, VectorError> {
        if embedding.len() != self.dimension {
            return Err(VectorError::InvalidDimension);
        }

        let vectors = self.vectors.read().await;
        
        let mut scores: Vec<(String, f32, serde_json::Value)> = vectors
            .iter()
            .map(|(id, stored)| {
                let score = Self::cosine_similarity(&embedding, &stored.embedding);
                (id.clone(), score, stored.metadata.clone())
            })
            .collect();

        // Sort by score descending
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top_k
        let results: Vec<VectorMatch> = scores
            .into_iter()
            .take(top_k)
            .map(|(id, score, metadata)| VectorMatch { id, score, metadata })
            .collect();

        Ok(results)
    }

    async fn delete(&self, id: &str) -> Result<(), VectorError> {
        let mut vectors = self.vectors.write().await;
        vectors.remove(id);
        Ok(())
    }

    async fn health_check(&self) -> Result<bool, VectorError> {
        Ok(true) // Always healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((InMemoryStore::cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!((InMemoryStore::cosine_similarity(&a, &c) - 0.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_dimension_check() {
        let store = InMemoryStore::new(3);
        let result = store.upsert("test", vec![1.0, 0.0], serde_json::json!({})).await;
        assert!(matches!(result, Err(VectorError::InvalidDimension)));
    }
}
