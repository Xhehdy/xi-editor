//! Glyph LLM - Interface to AI models (via Sidecar)

use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("API error: {0}")]
    Api(String),
}

#[derive(Serialize)]
struct EmbeddingRequest {
    text: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    vector: Vec<f32>,
}

#[derive(Serialize)]
struct CompletionRequest {
    prompt: String,
    context: Option<String>,
}

#[derive(Deserialize)]
struct CompletionResponse {
    text: String,
}

/// Client for the LangChain sidecar
pub struct LlmClient {
    base_url: String,
    client: reqwest::Client,
}

impl LlmClient {
    pub fn new(url: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url: url.trim_end_matches('/').to_string(),
            client,
        }
    }

    /// Generate embeddings for text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError> {
        let res = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .json(&EmbeddingRequest {
                text: text.to_string(),
            })
            .send()
            .await
            .map_err(|e| LlmError::Connection(e.to_string()))?;

        if !res.status().is_success() {
            return Err(LlmError::Api(res.text().await.unwrap_or_default()));
        }

        let body: EmbeddingResponse = res.json().await.map_err(|e| LlmError::Api(e.to_string()))?;

        Ok(body.vector)
    }

    /// Generate completion (chat)
    pub async fn complete(&self, prompt: &str, context: Option<&str>) -> Result<String, LlmError> {
        let res = self
            .client
            .post(format!("{}/completion", self.base_url))
            .json(&CompletionRequest {
                prompt: prompt.to_string(),
                context: context.map(|s| s.to_string()),
            })
            .send()
            .await
            .map_err(|e| LlmError::Connection(e.to_string()))?;

        if !res.status().is_success() {
            return Err(LlmError::Api(res.text().await.unwrap_or_default()));
        }

        let body: CompletionResponse =
            res.json().await.map_err(|e| LlmError::Api(e.to_string()))?;

        Ok(body.text)
    }
}
