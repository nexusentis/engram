use async_trait::async_trait;

use super::types::{Conversation, ExtractedFact};
use crate::error::Result;

/// Configuration for extractors
#[derive(Debug, Clone)]
pub struct ExtractorConfig {
    /// Minimum confidence to accept an extraction
    pub confidence_threshold: f32,
    /// Maximum facts to extract per conversation
    pub max_facts: usize,
    /// Whether to extract entities
    pub extract_entities: bool,
    /// Whether to extract temporal markers
    pub extract_temporal: bool,
}

impl Default for ExtractorConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.5,
            max_facts: 5,
            extract_entities: true,
            extract_temporal: true,
        }
    }
}

/// Trait for all memory extractors
#[async_trait]
pub trait Extractor: Send + Sync {
    /// Extract memories from a conversation
    async fn extract(&self, conversation: &Conversation) -> Result<Vec<ExtractedFact>>;

    /// Get the model name used by this extractor
    fn model_name(&self) -> &str;

    /// Get the confidence threshold for fallback decisions
    fn confidence_threshold(&self) -> f32;
}

/// Box type for dynamic extractor dispatch
pub type BoxedExtractor = Box<dyn Extractor>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extractor_config_default() {
        let config = ExtractorConfig::default();
        assert_eq!(config.confidence_threshold, 0.5);
        assert_eq!(config.max_facts, 5);
        assert!(config.extract_entities);
        assert!(config.extract_temporal);
    }
}
