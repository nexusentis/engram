use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::provider::{normalize_vector, EmbeddingProvider, EMBEDDING_DIMENSION};
use crate::error::{EmbeddingError, LlmError, Result};
use crate::llm::{HttpLlmClient, LlmClientConfig};

/// Remote embedding provider using OpenAI API
pub struct RemoteEmbeddingProvider {
    llm_client: HttpLlmClient,
    model: String,
    normalize: bool,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

impl RemoteEmbeddingProvider {
    /// Create new remote provider with OpenAI API
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
    ) -> std::result::Result<Self, crate::error::LlmError> {
        let config = LlmClientConfig {
            max_retries: 5,
            request_timeout_secs: 30,
            ..Default::default()
        };
        let llm_client = HttpLlmClient::new(api_key)?
            .with_base_url("https://api.openai.com/v1/embeddings")
            .with_llm_config(config)?;

        Ok(Self {
            llm_client,
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            normalize: true,
        })
    }

    /// Create from environment variable.
    ///
    /// Returns `None` if `OPENAI_API_KEY` is not set.
    /// Logs a warning and returns `None` if provider initialization fails.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("OPENAI_API_KEY").ok()?;
        match Self::new(key, None) {
            Ok(provider) => Some(provider),
            Err(e) => {
                tracing::warn!("Failed to initialize RemoteEmbeddingProvider: {e}");
                None
            }
        }
    }

    /// Set custom base URL (for OpenAI-compatible APIs)
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.llm_client = self.llm_client.with_base_url(base_url);
        self
    }

    /// Set normalization
    pub fn with_normalize(mut self, normalize: bool) -> Self {
        self.normalize = normalize;
        self
    }

    /// Set model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait]
impl EmbeddingProvider for RemoteEmbeddingProvider {
    /// Override: OpenAI models don't use e5-style prefixes
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.embed(query).await
    }

    /// Override: OpenAI models don't use e5-style prefixes
    async fn embed_document(&self, document: &str) -> Result<Vec<f32>> {
        self.embed(document).await
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let request = EmbeddingRequest {
            input: texts.to_vec(),
            model: self.model.clone(),
        };

        let body = serde_json::to_value(&request)
            .map_err(|e| EmbeddingError::ApiRequest(e.to_string()))?;

        let response_json = self
            .llm_client
            .send_request(&body)
            .await
            .map_err(|e| match e {
                LlmError::Request(_) => EmbeddingError::ApiRequest(e.to_string()),
                LlmError::InvalidResponse(_) => EmbeddingError::ResponseParsing(e.to_string()),
                _ => EmbeddingError::ApiResponse(e.to_string()),
            })?;

        let result: EmbeddingResponse = serde_json::from_value(response_json)
            .map_err(|e| EmbeddingError::ResponseParsing(e.to_string()))?;

        // Sort by index to maintain order
        let mut data = result.data;
        data.sort_by_key(|d| d.index);

        let mut embeddings: Vec<Vec<f32>> = data.into_iter().map(|d| d.embedding).collect();

        // Normalize if configured
        if self.normalize {
            for embedding in &mut embeddings {
                normalize_vector(embedding);
            }
        }

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIMENSION
    }

    fn name(&self) -> &str {
        "openai-remote"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_provider_creation() {
        let provider =
            RemoteEmbeddingProvider::new("test-key", Some("test-model".to_string())).unwrap();

        assert_eq!(provider.dimension(), 1536);
        assert_eq!(provider.name(), "openai-remote");
        assert_eq!(provider.model, "test-model");
    }

    #[test]
    fn test_remote_provider_default_model() {
        let provider = RemoteEmbeddingProvider::new("test-key", None).unwrap();
        assert_eq!(provider.model, "text-embedding-3-small");
    }

    #[test]
    fn test_remote_provider_with_base_url() {
        // Verifies builder chain works; actual URL is internal to HttpLlmClient
        let _provider = RemoteEmbeddingProvider::new("test-key", None)
            .unwrap()
            .with_base_url("http://localhost:8080");
    }

    #[test]
    fn test_remote_provider_with_model() {
        let provider = RemoteEmbeddingProvider::new("test-key", None)
            .unwrap()
            .with_model("text-embedding-3-large");

        assert_eq!(provider.model, "text-embedding-3-large");
    }

    #[test]
    fn test_remote_provider_with_normalize() {
        let provider = RemoteEmbeddingProvider::new("test-key", None)
            .unwrap()
            .with_normalize(false);

        assert!(!provider.normalize);
    }

    #[tokio::test]
    async fn test_embed_batch_empty() {
        let provider = RemoteEmbeddingProvider::new("test-key", None).unwrap();
        let result = provider.embed_batch(&[]).await.unwrap();
        assert!(result.is_empty());
    }

    // Integration test - requires API key
    #[tokio::test]
    #[ignore]
    async fn test_embed_batch_real() {
        let provider = RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY not set");

        let texts = vec!["Hello world".to_string(), "Another sentence".to_string()];

        let embeddings = provider.embed_batch(&texts).await.unwrap();

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), 1536); // text-embedding-3-small dimension

        // Check normalization
        let norm: f32 = embeddings[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }
}
