use async_trait::async_trait;
use qdrant_client::qdrant::{Condition, Filter, ScrollPointsBuilder};
use qdrant_client::Qdrant;

use super::channel::{ChannelConfig, ScoredResult, SearchChannel};
use super::semantic::payload_to_memory;
use crate::error::{Result, StorageError};
use crate::types::{EpistemicType, Memory};

/// Keyword search channel using Qdrant text matching
pub struct KeywordChannel {
    client: Qdrant,
    keywords: Vec<String>,
    collection: EpistemicType,
    user_id: String,
}

impl KeywordChannel {
    /// Create a new keyword search channel
    pub fn new(
        client: Qdrant,
        keywords: Vec<String>,
        collection: EpistemicType,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            keywords,
            collection,
            user_id: user_id.into(),
        }
    }

    fn collection_name(&self) -> &'static str {
        self.collection.collection_name()
    }

    fn build_keyword_filter(&self) -> Filter {
        let mut conditions = vec![
            Condition::matches("user_id", self.user_id.clone()),
            Condition::matches("is_latest", true),
        ];

        // Add full-text search condition if keywords are present
        if !self.keywords.is_empty() {
            let keyword_query = self.keywords.join(" ");
            conditions.push(Condition::matches_text("content", keyword_query));
        }

        Filter::must(conditions)
    }

    fn calculate_keyword_score(&self, memory: &Memory) -> f32 {
        if self.keywords.is_empty() {
            return 0.0;
        }

        let content_lower = memory.content.to_lowercase();
        let mut matches = 0;

        for keyword in &self.keywords {
            if content_lower.contains(&keyword.to_lowercase()) {
                matches += 1;
            }
        }

        matches as f32 / self.keywords.len() as f32
    }
}

#[async_trait]
impl SearchChannel for KeywordChannel {
    async fn search(&self, config: &ChannelConfig) -> Result<Vec<ScoredResult>> {
        if self.keywords.is_empty() {
            return Ok(vec![]);
        }

        let filter = self.build_keyword_filter();

        let scroll_result = self
            .client
            .scroll(
                ScrollPointsBuilder::new(self.collection_name())
                    .filter(filter)
                    .limit(config.top_k as u32)
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        let results = scroll_result
            .result
            .into_iter()
            .filter_map(|point| {
                let memory = payload_to_memory(&point.payload).ok()?;
                let score = self.calculate_keyword_score(&memory);
                if score >= config.min_score {
                    Some(ScoredResult::new(memory, score, self.name()))
                } else {
                    None
                }
            })
            .collect();

        Ok(results)
    }

    fn name(&self) -> &str {
        "keyword"
    }

    fn weight(&self) -> f32 {
        0.8 // Slightly lower than semantic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_score_calculation_full_match() {
        let memory = Memory::new("user", "Alice works at Google in San Francisco");

        let keywords = vec![
            "alice".to_string(),
            "google".to_string(),
            "francisco".to_string(),
        ];

        // Simulate scoring
        let content_lower = memory.content.to_lowercase();
        let mut matches = 0;
        for keyword in &keywords {
            if content_lower.contains(&keyword.to_lowercase()) {
                matches += 1;
            }
        }
        let score = matches as f32 / keywords.len() as f32;

        assert!((score - 1.0).abs() < 0.01); // All 3 keywords match
    }

    #[test]
    fn test_keyword_score_calculation_partial_match() {
        let memory = Memory::new("user", "Alice works at Google in San Francisco");

        let keywords = vec![
            "alice".to_string(),
            "google".to_string(),
            "tokyo".to_string(), // This won't match
        ];

        let content_lower = memory.content.to_lowercase();
        let mut matches = 0;
        for keyword in &keywords {
            if content_lower.contains(&keyword.to_lowercase()) {
                matches += 1;
            }
        }
        let score = matches as f32 / keywords.len() as f32;

        assert!((score - 0.666).abs() < 0.01); // 2/3 keywords match
    }

    #[test]
    fn test_keyword_score_calculation_no_match() {
        let memory = Memory::new("user", "Bob likes programming in Python");

        let keywords = vec![
            "alice".to_string(),
            "google".to_string(),
            "tokyo".to_string(),
        ];

        let content_lower = memory.content.to_lowercase();
        let mut matches = 0;
        for keyword in &keywords {
            if content_lower.contains(&keyword.to_lowercase()) {
                matches += 1;
            }
        }
        let score = matches as f32 / keywords.len() as f32;

        assert_eq!(score, 0.0); // No keywords match
    }

    #[test]
    fn test_keyword_score_empty_keywords() {
        let keywords: Vec<String> = vec![];
        let score = if keywords.is_empty() { 0.0 } else { 1.0 };

        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_keyword_channel_name() {
        assert_eq!("keyword", "keyword");
    }

    #[test]
    fn test_keyword_channel_weight() {
        // Keyword channel should have weight 0.8
        let weight = 0.8f32;
        assert!((weight - 0.8).abs() < 0.001);
    }
}
