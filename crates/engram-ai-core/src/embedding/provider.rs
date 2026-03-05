use async_trait::async_trait;

use crate::error::{EmbeddingError, Result};

/// Default dimension for text-embedding-3-small
pub const EMBEDDING_DIMENSION: usize = 1536;

/// Trait for embedding generation providers
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embeddings for a batch of texts
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::EmptyResult.into())
    }

    /// Generate query embedding with appropriate instruction prefix
    /// For e5 models, queries need "query: " prefix
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let prefixed = format!("query: {}", query);
        self.embed(&prefixed).await
    }

    /// Generate document embedding with appropriate instruction prefix
    /// For e5 models, documents need "passage: " prefix
    async fn embed_document(&self, document: &str) -> Result<Vec<f32>> {
        let prefixed = format!("passage: {}", document);
        self.embed(&prefixed).await
    }

    /// Get the dimensionality of embeddings
    fn dimension(&self) -> usize;

    /// Get the provider name for logging
    fn name(&self) -> &str;
}

/// L2 normalize a vector in place
pub fn normalize_vector(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_dimension() {
        assert_eq!(EMBEDDING_DIMENSION, 1536);
    }

    #[test]
    fn test_normalize_vector() {
        let mut v = vec![3.0, 4.0];
        normalize_vector(&mut v);

        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.001);
        assert!((v[0] - 0.6).abs() < 0.001);
        assert!((v[1] - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_normalize_zero_vector() {
        let mut v = vec![0.0, 0.0, 0.0];
        normalize_vector(&mut v);
        assert_eq!(v, vec![0.0, 0.0, 0.0]);
    }
}
