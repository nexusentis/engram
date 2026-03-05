use async_trait::async_trait;
use qdrant_client::qdrant::{Condition, Filter, SearchParams, SearchPointsBuilder};
use qdrant_client::Qdrant;
use std::collections::HashMap;

use super::channel::{ChannelConfig, ScoredResult, SearchChannel};
use crate::error::{Result, StorageError};
use crate::types::{EpistemicType, Memory};

/// Semantic search channel using vector similarity
pub struct SemanticChannel {
    client: Qdrant,
    query_embedding: Vec<f32>,
    collection: EpistemicType,
    user_id: String,
}

impl SemanticChannel {
    /// Create a new semantic search channel
    pub fn new(
        client: Qdrant,
        query_embedding: Vec<f32>,
        collection: EpistemicType,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            query_embedding,
            collection,
            user_id: user_id.into(),
        }
    }

    fn collection_name(&self) -> &'static str {
        self.collection.collection_name()
    }
}

#[async_trait]
impl SearchChannel for SemanticChannel {
    async fn search(&self, config: &ChannelConfig) -> Result<Vec<ScoredResult>> {
        let filter = Filter::must([
            Condition::matches("user_id", self.user_id.clone()),
            Condition::matches("is_latest", true),
        ]);

        let search_result = self
            .client
            .search_points(
                SearchPointsBuilder::new(
                    self.collection_name(),
                    self.query_embedding.clone(),
                    config.top_k as u64,
                )
                .filter(filter)
                .with_payload(true)
                .with_vectors(false)
                .params(SearchParams {
                    hnsw_ef: Some(128),
                    exact: Some(false),
                    ..Default::default()
                })
                .score_threshold(config.min_score),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        let results = search_result
            .result
            .into_iter()
            .filter_map(|point| {
                let memory = payload_to_memory(&point.payload).ok()?;
                Some(ScoredResult::new(memory, point.score, self.name()))
            })
            .collect();

        Ok(results)
    }

    fn name(&self) -> &str {
        "semantic"
    }

    fn weight(&self) -> f32 {
        1.0
    }
}

/// Convert Qdrant payload to Memory
pub fn payload_to_memory(
    payload: &HashMap<String, qdrant_client::qdrant::Value>,
) -> Result<Memory> {
    let mut map = serde_json::Map::new();

    for (k, v) in payload {
        map.insert(k.clone(), qdrant_value_to_json(v));
    }

    serde_json::from_value(serde_json::Value::Object(map))
        .map_err(|e| StorageError::Qdrant(e.to_string()).into())
}

fn qdrant_value_to_json(value: &qdrant_client::qdrant::Value) -> serde_json::Value {
    use qdrant_client::qdrant::value::Kind;

    match &value.kind {
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Kind::IntegerValue(i)) => serde_json::Value::Number((*i).into()),
        Some(Kind::DoubleValue(d)) => serde_json::Number::from_f64(*d)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Kind::ListValue(list)) => {
            serde_json::Value::Array(list.values.iter().map(qdrant_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &s.fields {
                map.insert(k.clone(), qdrant_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_name_mapping() {
        assert_eq!(EpistemicType::World.collection_name(), "world");
        assert_eq!(EpistemicType::Experience.collection_name(), "experience");
        assert_eq!(EpistemicType::Opinion.collection_name(), "opinion");
        assert_eq!(EpistemicType::Observation.collection_name(), "observation");
    }

    #[test]
    fn test_semantic_channel_name() {
        assert_eq!("semantic", "semantic");
    }

    #[test]
    fn test_qdrant_value_to_json_string() {
        use qdrant_client::qdrant::{value::Kind, Value};

        let value = Value {
            kind: Some(Kind::StringValue("test".to_string())),
        };

        let json = qdrant_value_to_json(&value);
        assert_eq!(json, serde_json::Value::String("test".to_string()));
    }

    #[test]
    fn test_qdrant_value_to_json_bool() {
        use qdrant_client::qdrant::{value::Kind, Value};

        let value = Value {
            kind: Some(Kind::BoolValue(true)),
        };

        let json = qdrant_value_to_json(&value);
        assert_eq!(json, serde_json::Value::Bool(true));
    }

    #[test]
    fn test_qdrant_value_to_json_number() {
        use qdrant_client::qdrant::{value::Kind, Value};

        let value = Value {
            kind: Some(Kind::IntegerValue(42)),
        };

        let json = qdrant_value_to_json(&value);
        assert_eq!(json, serde_json::json!(42));
    }

    #[test]
    fn test_qdrant_value_to_json_array() {
        use qdrant_client::qdrant::{value::Kind, ListValue, Value};

        let value = Value {
            kind: Some(Kind::ListValue(ListValue {
                values: vec![
                    Value {
                        kind: Some(Kind::StringValue("a".to_string())),
                    },
                    Value {
                        kind: Some(Kind::StringValue("b".to_string())),
                    },
                ],
            })),
        };

        let json = qdrant_value_to_json(&value);
        assert_eq!(json, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn test_qdrant_value_to_json_null() {
        use qdrant_client::qdrant::Value;

        let value = Value { kind: None };

        let json = qdrant_value_to_json(&value);
        assert_eq!(json, serde_json::Value::Null);
    }
}
