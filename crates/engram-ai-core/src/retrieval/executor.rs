use tokio::task::JoinSet;

use super::channel::{ChannelConfig, ScoredResult, SearchChannel};
use crate::error::Result;

/// Execute multiple search channels in parallel
pub struct ChannelExecutor {
    config: ChannelConfig,
}

impl ChannelExecutor {
    /// Create a new channel executor with the given config
    pub fn new(config: ChannelConfig) -> Self {
        Self { config }
    }

    /// Execute all channels concurrently and collect results
    ///
    /// Each channel runs as a separate Tokio task. If a channel fails,
    /// it returns empty results instead of failing the entire search.
    pub async fn execute(
        &self,
        channels: Vec<Box<dyn SearchChannel + Send>>,
    ) -> Result<Vec<Vec<ScoredResult>>> {
        if channels.is_empty() {
            return Ok(vec![]);
        }

        let mut join_set = JoinSet::new();
        let config = self.config.clone();

        for channel in channels {
            let channel_config = config.clone();
            join_set.spawn(async move { channel.search(&channel_config).await });
        }

        let mut all_results = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(results)) => all_results.push(results),
                Ok(Err(e)) => {
                    tracing::warn!("Channel search failed: {}", e);
                    all_results.push(vec![]); // Continue with empty results
                }
                Err(e) => {
                    tracing::warn!("Channel task panicked: {}", e);
                    all_results.push(vec![]);
                }
            }
        }

        Ok(all_results)
    }

    /// Execute channels and flatten all results into a single list
    pub async fn execute_flat(
        &self,
        channels: Vec<Box<dyn SearchChannel + Send>>,
    ) -> Result<Vec<ScoredResult>> {
        let results = self.execute(channels).await?;
        Ok(results.into_iter().flatten().collect())
    }

    /// Get the config
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }
}

impl Default for ChannelExecutor {
    fn default() -> Self {
        Self::new(ChannelConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Memory;
    use async_trait::async_trait;

    /// Mock channel for testing
    struct MockChannel {
        name: String,
        results: Vec<ScoredResult>,
        should_fail: bool,
    }

    impl MockChannel {
        fn new(name: &str, results: Vec<ScoredResult>) -> Self {
            Self {
                name: name.to_string(),
                results,
                should_fail: false,
            }
        }

        fn failing(name: &str) -> Self {
            Self {
                name: name.to_string(),
                results: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl SearchChannel for MockChannel {
        async fn search(&self, _config: &ChannelConfig) -> Result<Vec<ScoredResult>> {
            if self.should_fail {
                Err(crate::error::StorageError::Qdrant("Mock failure".to_string()).into())
            } else {
                Ok(self.results.clone())
            }
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_executor_handles_empty_channels() {
        let executor = ChannelExecutor::default();
        let results = executor.execute(vec![]).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_executor_single_channel() {
        let memory = Memory::new("user", "Test content");
        let scored_result = ScoredResult::new(memory, 0.95, "mock");

        let channel = MockChannel::new("mock", vec![scored_result]);
        let executor = ChannelExecutor::default();

        let results = executor.execute(vec![Box::new(channel)]).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].len(), 1);
        assert_eq!(results[0][0].score, 0.95);
    }

    #[tokio::test]
    async fn test_executor_multiple_channels() {
        let memory1 = Memory::new("user", "Content 1");
        let memory2 = Memory::new("user", "Content 2");

        let channel1 = MockChannel::new("channel1", vec![ScoredResult::new(memory1, 0.9, "ch1")]);
        let channel2 = MockChannel::new("channel2", vec![ScoredResult::new(memory2, 0.8, "ch2")]);

        let executor = ChannelExecutor::default();
        let results = executor
            .execute(vec![Box::new(channel1), Box::new(channel2)])
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_executor_handles_channel_failure() {
        let memory = Memory::new("user", "Success content");

        let success_channel =
            MockChannel::new("success", vec![ScoredResult::new(memory, 0.9, "s")]);
        let failing_channel = MockChannel::failing("failure");

        let executor = ChannelExecutor::default();
        let results = executor
            .execute(vec![Box::new(success_channel), Box::new(failing_channel)])
            .await
            .unwrap();

        // Both channels should return results (one success, one empty)
        assert_eq!(results.len(), 2);

        // One should have results, one should be empty
        let total_results: usize = results.iter().map(|r| r.len()).sum();
        assert_eq!(total_results, 1);
    }

    #[tokio::test]
    async fn test_executor_execute_flat() {
        let memory1 = Memory::new("user", "Content 1");
        let memory2 = Memory::new("user", "Content 2");

        let channel1 = MockChannel::new("channel1", vec![ScoredResult::new(memory1, 0.9, "ch1")]);
        let channel2 = MockChannel::new("channel2", vec![ScoredResult::new(memory2, 0.8, "ch2")]);

        let executor = ChannelExecutor::default();
        let results = executor
            .execute_flat(vec![Box::new(channel1), Box::new(channel2)])
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_executor_with_custom_config() {
        let config = ChannelConfig::default().with_top_k(10).with_min_score(0.5);

        let executor = ChannelExecutor::new(config);

        assert_eq!(executor.config().top_k, 10);
        assert_eq!(executor.config().min_score, 0.5);
    }

    #[test]
    fn test_channel_config_default() {
        let config = ChannelConfig::default();
        assert_eq!(config.top_k, 50);
        assert_eq!(config.min_score, 0.0);
    }
}
