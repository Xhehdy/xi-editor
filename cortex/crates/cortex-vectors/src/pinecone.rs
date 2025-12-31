//! Pinecone vector store implementation

use crate::{VectorError, VectorMatch, VectorStore};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Pinecone vector store
pub struct PineconeStore {
    api_key: String,
    index_host: String,
    client: Client,
    namespace: Option<String>,
}

#[derive(Serialize)]
struct UpsertRequest {
    vectors: Vec<PineconeVector>,
    namespace: Option<String>,
}

#[derive(Serialize)]
struct PineconeVector {
    id: String,
    values: Vec<f32>,
    metadata: serde_json::Value,
}

#[derive(Serialize)]
struct QueryRequest {
    vector: Vec<f32>,
    #[serde(rename = "topK")]
    top_k: usize,
    #[serde(rename = "includeMetadata")]
    include_metadata: bool,
    namespace: Option<String>,
}

#[derive(Deserialize)]
struct QueryResponse {
    matches: Vec<PineconeMatch>,
}

#[derive(Deserialize)]
struct PineconeMatch {
    id: String,
    score: f32,
    metadata: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct DeleteRequest {
    ids: Vec<String>,
    namespace: Option<String>,
}

impl PineconeStore {
    /// Create a new Pinecone store
    /// 
    /// # Arguments
    /// * `api_key` - Your Pinecone API key
    /// * `index_host` - Your index host (e.g., "my-index-abc123.svc.us-east1-gcp.pinecone.io")
    pub fn new(api_key: &str, index_host: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            index_host: index_host.to_string(),
            client: Client::new(),
            namespace: None,
        }
    }

    /// Set the namespace for this store
    pub fn with_namespace(mut self, namespace: &str) -> Self {
        self.namespace = Some(namespace.to_string());
        self
    }

    /// Create from environment variables
    /// 
    /// Expects:
    /// - `PINECONE_API_KEY`
    /// - `PINECONE_INDEX_HOST`
    pub fn from_env() -> Result<Self, VectorError> {
        let api_key = std::env::var("PINECONE_API_KEY")
            .map_err(|_| VectorError::ConnectionError("PINECONE_API_KEY not set".into()))?;
        let index_host = std::env::var("PINECONE_INDEX_HOST")
            .map_err(|_| VectorError::ConnectionError("PINECONE_INDEX_HOST not set".into()))?;
        
        Ok(Self::new(&api_key, &index_host))
    }

    fn vectors_url(&self) -> String {
        format!("https://{}/vectors", self.index_host)
    }
}

#[async_trait]
impl VectorStore for PineconeStore {
    async fn upsert(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<(), VectorError> {
        let request = UpsertRequest {
            vectors: vec![PineconeVector {
                id: id.to_string(),
                values: embedding,
                metadata,
            }],
            namespace: self.namespace.clone(),
        };

        let response = self.client
            .post(format!("{}/upsert", self.vectors_url()))
            .header("Api-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| VectorError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!("Pinecone upsert failed: {}", error_text);
            return Err(VectorError::ApiError(error_text));
        }

        info!("Upserted vector: {}", id);
        Ok(())
    }

    async fn query(
        &self,
        embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<VectorMatch>, VectorError> {
        let request = QueryRequest {
            vector: embedding,
            top_k,
            include_metadata: true,
            namespace: self.namespace.clone(),
        };

        let response = self.client
            .post(format!("{}/query", self.vectors_url()))
            .header("Api-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| VectorError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(VectorError::ApiError(error_text));
        }

        let query_response: QueryResponse = response
            .json()
            .await
            .map_err(|e| VectorError::ApiError(e.to_string()))?;

        let matches = query_response.matches
            .into_iter()
            .map(|m| VectorMatch {
                id: m.id,
                score: m.score,
                metadata: m.metadata.unwrap_or(serde_json::Value::Null),
            })
            .collect();

        Ok(matches)
    }

    async fn delete(&self, id: &str) -> Result<(), VectorError> {
        let request = DeleteRequest {
            ids: vec![id.to_string()],
            namespace: self.namespace.clone(),
        };

        let response = self.client
            .post(format!("{}/delete", self.vectors_url()))
            .header("Api-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| VectorError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(VectorError::ApiError(error_text));
        }

        info!("Deleted vector: {}", id);
        Ok(())
    }

    async fn health_check(&self) -> Result<bool, VectorError> {
        // Pinecone doesn't have a dedicated health endpoint for indexes
        // We'll just check if we can describe the index stats
        let response = self.client
            .get(format!("{}/describe_index_stats", self.vectors_url()))
            .header("Api-Key", &self.api_key)
            .send()
            .await
            .map_err(|e| VectorError::ConnectionError(e.to_string()))?;

        Ok(response.status().is_success())
    }
}
