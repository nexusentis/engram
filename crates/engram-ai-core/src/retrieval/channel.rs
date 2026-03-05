use crate::error::Result;
use crate::types::Memory;
use async_trait::async_trait;

/// A scored search result from a channel
#[derive(Debug, Clone)]
pub struct ScoredResult {
    pub memory: Memory,
    pub score: f32,
    pub channel_name: String,
}

impl ScoredResult {
    /// Create a new scored result
    pub fn new(memory: Memory, score: f32, channel_name: impl Into<String>) -> Self {
        Self {
            memory,
            score,
            channel_name: channel_name.into(),
        }
    }
}

/// Configuration for search channels
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub top_k: usize,
    pub min_score: f32,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            top_k: 50,
            min_score: 0.0,
        }
    }
}

impl ChannelConfig {
    /// Create a new channel config with specified top_k
    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k;
        self
    }

    /// Create a new channel config with specified min_score
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = min_score;
        self
    }
}

/// Trait for search channels
#[async_trait]
pub trait SearchChannel: Send + Sync {
    /// Execute the search and return scored results
    async fn search(&self, config: &ChannelConfig) -> Result<Vec<ScoredResult>>;

    /// Name of this channel for logging and fusion
    fn name(&self) -> &str;

    /// Weight multiplier for RRF fusion (default 1.0)
    fn weight(&self) -> f32 {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_config_default() {
        let config = ChannelConfig::default();
        assert_eq!(config.top_k, 50);
        assert_eq!(config.min_score, 0.0);
    }

    #[test]
    fn test_channel_config_builder() {
        let config = ChannelConfig::default().with_top_k(100).with_min_score(0.5);

        assert_eq!(config.top_k, 100);
        assert_eq!(config.min_score, 0.5);
    }

    #[test]
    fn test_scored_result_creation() {
        let memory = Memory::new("user", "test content");
        let result = ScoredResult::new(memory.clone(), 0.95, "semantic");

        assert_eq!(result.score, 0.95);
        assert_eq!(result.channel_name, "semantic");
        assert_eq!(result.memory.content, "test content");
    }
}
