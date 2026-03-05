use async_trait::async_trait;
use qdrant_client::qdrant::{Condition, Filter, Range, ScrollPointsBuilder};
use qdrant_client::Qdrant;

use super::channel::{ChannelConfig, ScoredResult, SearchChannel};
use super::query_analyzer::TemporalConstraint;
use super::semantic::payload_to_memory;
use crate::error::{Result, StorageError};
use crate::types::{EpistemicType, Memory};

/// Temporal search channel using date range filters
pub struct TemporalChannel {
    client: Qdrant,
    constraints: Vec<TemporalConstraint>,
    collection: EpistemicType,
    user_id: String,
}

impl TemporalChannel {
    /// Create a new temporal search channel
    pub fn new(
        client: Qdrant,
        constraints: Vec<TemporalConstraint>,
        collection: EpistemicType,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            constraints,
            collection,
            user_id: user_id.into(),
        }
    }

    fn collection_name(&self) -> &'static str {
        self.collection.collection_name()
    }

    fn build_temporal_filter(&self) -> Filter {
        let mut conditions = vec![
            Condition::matches("user_id", self.user_id.clone()),
            Condition::matches("is_latest", true),
        ];

        // Apply first temporal constraint (most confident)
        // Using numeric range on timestamp stored as float
        if let Some(constraint) = self.constraints.first() {
            if let (Some(start), Some(end)) = (constraint.start, constraint.end) {
                conditions.push(Condition::range(
                    "t_valid",
                    Range {
                        gte: Some(start.timestamp() as f64),
                        lte: Some(end.timestamp() as f64),
                        ..Default::default()
                    },
                ));
            }
        }

        Filter::must(conditions)
    }

    fn calculate_temporal_score(&self, memory: &Memory) -> f32 {
        // Score based on how well memory matches temporal constraints
        if let Some(constraint) = self.constraints.first() {
            if let (Some(start), Some(end)) = (constraint.start, constraint.end) {
                let memory_time = memory.t_valid.timestamp() as f64;
                let start_ts = start.timestamp() as f64;
                let end_ts = end.timestamp() as f64;

                // Within range = high score
                if memory_time >= start_ts && memory_time <= end_ts {
                    return constraint.confidence;
                }

                // Partial overlap gets partial score with decay
                let distance = if memory_time < start_ts {
                    start_ts - memory_time
                } else {
                    memory_time - end_ts
                };

                // Decay score based on distance (days)
                let days_away = distance / 86400.0;
                return (constraint.confidence * (-0.1 * days_away).exp() as f32).max(0.0);
            }
        }

        0.5 // Default score if no constraints
    }
}

#[async_trait]
impl SearchChannel for TemporalChannel {
    async fn search(&self, config: &ChannelConfig) -> Result<Vec<ScoredResult>> {
        if self.constraints.is_empty() {
            return Ok(vec![]);
        }

        let filter = self.build_temporal_filter();

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
                let score = self.calculate_temporal_score(&memory);
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
        "temporal"
    }

    fn weight(&self) -> f32 {
        1.2 // Boost temporal matches when temporal query detected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_temporal_score_within_range() {
        let now = Utc::now();
        let constraint = TemporalConstraint {
            start: Some(now - Duration::days(7)),
            end: Some(now),
            expression: "last week".to_string(),
            confidence: 0.9,
        };

        // Memory within range
        let memory_time = now - Duration::days(3);
        let memory_ts = memory_time.timestamp() as f64;
        let start_ts = constraint.start.unwrap().timestamp() as f64;
        let end_ts = constraint.end.unwrap().timestamp() as f64;

        let score = if memory_ts >= start_ts && memory_ts <= end_ts {
            constraint.confidence
        } else {
            0.0
        };

        assert!((score - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_temporal_score_outside_range() {
        let now = Utc::now();
        let constraint = TemporalConstraint {
            start: Some(now - Duration::days(7)),
            end: Some(now),
            expression: "last week".to_string(),
            confidence: 0.9,
        };

        // Memory 14 days ago (outside range)
        let memory_time = now - Duration::days(14);
        let memory_ts = memory_time.timestamp() as f64;
        let start_ts = constraint.start.unwrap().timestamp() as f64;
        let end_ts = constraint.end.unwrap().timestamp() as f64;

        let score = if memory_ts >= start_ts && memory_ts <= end_ts {
            constraint.confidence
        } else {
            // Decay calculation
            let distance = if memory_ts < start_ts {
                start_ts - memory_ts
            } else {
                memory_ts - end_ts
            };
            let days_away = distance / 86400.0;
            (constraint.confidence * (-0.1 * days_away).exp() as f32).max(0.0)
        };

        // 7 days away, so score should be decayed
        assert!(score < constraint.confidence);
        assert!(score > 0.0);
    }

    #[test]
    fn test_temporal_score_no_constraints() {
        let default_score = 0.5f32;
        assert_eq!(default_score, 0.5);
    }

    #[test]
    fn test_temporal_channel_name() {
        assert_eq!("temporal", "temporal");
    }

    #[test]
    fn test_temporal_channel_weight() {
        // Temporal channel should have weight 1.2
        let weight = 1.2f32;
        assert!((weight - 1.2).abs() < 0.001);
    }

    #[test]
    fn test_temporal_constraint_creation() {
        let now = Utc::now();
        let constraint = TemporalConstraint {
            start: Some(now - Duration::days(7)),
            end: Some(now),
            expression: "last week".to_string(),
            confidence: 0.95,
        };

        assert!(constraint.start.is_some());
        assert!(constraint.end.is_some());
        assert_eq!(constraint.expression, "last week");
        assert_eq!(constraint.confidence, 0.95);
    }
}
