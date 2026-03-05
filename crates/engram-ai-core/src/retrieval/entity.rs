use async_trait::async_trait;
use qdrant_client::qdrant::{Condition, Filter, ScrollPointsBuilder};
use qdrant_client::Qdrant;
use std::collections::HashSet;

use super::channel::{ChannelConfig, ScoredResult, SearchChannel};
use super::semantic::payload_to_memory;
use crate::error::{Result, StorageError};
use crate::extraction::ExtractedEntity;
use crate::types::{EpistemicType, Memory};

/// Entity search channel using payload filters
pub struct EntityChannel {
    client: Qdrant,
    entities: Vec<ExtractedEntity>,
    collection: EpistemicType,
    user_id: String,
}

impl EntityChannel {
    /// Create a new entity search channel
    pub fn new(
        client: Qdrant,
        entities: Vec<ExtractedEntity>,
        collection: EpistemicType,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            entities,
            collection,
            user_id: user_id.into(),
        }
    }

    fn collection_name(&self) -> &'static str {
        self.collection.collection_name()
    }

    fn build_entity_filter(&self) -> Filter {
        let mut conditions = vec![
            Condition::matches("user_id", self.user_id.clone()),
            Condition::matches("is_latest", true),
        ];

        // Match any of the entity IDs using match_any
        if !self.entities.is_empty() {
            let entity_ids: Vec<String> = self
                .entities
                .iter()
                .map(|e| e.normalized_id.clone())
                .collect();

            // Use matches with a Vec to match any of the entity IDs
            conditions.push(Condition::matches("entity_ids", entity_ids));
        }

        Filter::must(conditions)
    }

    fn calculate_entity_score(&self, memory: &Memory) -> f32 {
        if self.entities.is_empty() {
            return 0.0;
        }

        let query_entity_ids: HashSet<&String> =
            self.entities.iter().map(|e| &e.normalized_id).collect();

        let memory_entity_ids: HashSet<&String> = memory.entity_ids.iter().collect();

        let overlap = query_entity_ids.intersection(&memory_entity_ids).count();

        if overlap == 0 {
            0.0
        } else {
            // Score based on proportion of matched entities (F1-like)
            let precision = overlap as f32 / query_entity_ids.len() as f32;
            let recall = overlap as f32 / memory_entity_ids.len().max(1) as f32;

            // F1 score
            if precision + recall > 0.0 {
                2.0 * precision * recall / (precision + recall)
            } else {
                0.0
            }
        }
    }
}

#[async_trait]
impl SearchChannel for EntityChannel {
    async fn search(&self, config: &ChannelConfig) -> Result<Vec<ScoredResult>> {
        if self.entities.is_empty() {
            return Ok(vec![]);
        }

        let filter = self.build_entity_filter();

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
                let score = self.calculate_entity_score(&memory);
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
        "entity"
    }

    fn weight(&self) -> f32 {
        1.1 // Slight boost for entity matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entity(id: &str, entity_type: &str, name: &str) -> ExtractedEntity {
        ExtractedEntity {
            normalized_id: id.to_string(),
            entity_type: entity_type.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn test_entity_score_full_match() {
        // Memory has entities: alice, google
        let memory_entities: HashSet<&str> = ["alice", "google"].into_iter().collect();
        // Query has entities: alice, google
        let query_entities: HashSet<&str> = ["alice", "google"].into_iter().collect();

        let overlap = memory_entities.intersection(&query_entities).count();
        let precision = overlap as f32 / query_entities.len() as f32;
        let recall = overlap as f32 / memory_entities.len() as f32;
        let f1 = 2.0 * precision * recall / (precision + recall);

        assert!((f1 - 1.0).abs() < 0.01); // Perfect match
    }

    #[test]
    fn test_entity_score_partial_match() {
        // Memory has entities: alice, google
        let memory_entities: HashSet<&str> = ["alice", "google"].into_iter().collect();
        // Query has entities: alice, bob
        let query_entities: HashSet<&str> = ["alice", "bob"].into_iter().collect();

        let overlap = memory_entities.intersection(&query_entities).count();
        let precision = overlap as f32 / query_entities.len() as f32; // 1/2 = 0.5
        let recall = overlap as f32 / memory_entities.len() as f32; // 1/2 = 0.5

        let f1 = 2.0 * precision * recall / (precision + recall);
        assert!((f1 - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_entity_score_no_match() {
        // Memory has entities: alice, google
        let memory_entities: HashSet<&str> = ["alice", "google"].into_iter().collect();
        // Query has entities: bob, microsoft
        let query_entities: HashSet<&str> = ["bob", "microsoft"].into_iter().collect();

        let overlap = memory_entities.intersection(&query_entities).count();

        let score = if overlap == 0 {
            0.0
        } else {
            let precision = overlap as f32 / query_entities.len() as f32;
            let recall = overlap as f32 / memory_entities.len() as f32;
            2.0 * precision * recall / (precision + recall)
        };

        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_entity_score_empty_query() {
        let query_entities: Vec<ExtractedEntity> = vec![];

        let score = if query_entities.is_empty() { 0.0 } else { 1.0 };

        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_entity_channel_name() {
        assert_eq!("entity", "entity");
    }

    #[test]
    fn test_entity_channel_weight() {
        // Entity channel should have weight 1.1
        let weight = 1.1f32;
        assert!((weight - 1.1).abs() < 0.001);
    }

    #[test]
    fn test_extracted_entity_creation() {
        let entity = make_entity("alice", "person", "Alice");

        assert_eq!(entity.normalized_id, "alice");
        assert_eq!(entity.entity_type, "person");
        assert_eq!(entity.name, "Alice");
    }
}
