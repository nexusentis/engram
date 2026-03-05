use qdrant_client::qdrant::{
    value::Kind, Condition, CountPointsBuilder, CreateCollectionBuilder,
    CreateFieldIndexCollectionBuilder, Distance, FieldType, Filter, GetPointsBuilder, PointId,
    PointStruct, ScrollPointsBuilder, SearchPointsBuilder, SetPayloadPointsBuilder,
    UpsertPointsBuilder, Value, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use uuid::Uuid;

use crate::error::{Result, StorageError};
use crate::types::Memory;

use super::config::QdrantConfig;

/// Collections used by the memory system (fact collections)
pub const COLLECTIONS: [&str; 4] = ["world", "experience", "opinion", "observation"];

/// Redact credentials from a URL for safe logging (e.g., `http://user:pass@host` → `http://***@host`).
fn redact_url(url: &str) -> String {
    if let Some(at) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            return format!("{}://***@{}", &url[..scheme_end], &url[at + 1..]);
        }
    }
    url.to_string()
}

/// Collection for raw conversation messages
pub const MESSAGES_COLLECTION: &str = "messages";

pub struct QdrantStorage {
    /// The underlying Qdrant client. Public for direct access when needed.
    pub client: Qdrant,
    config: QdrantConfig,
}

impl QdrantStorage {
    /// Create a new Qdrant storage instance
    pub async fn new(config: QdrantConfig) -> Result<Self> {
        let url = config
            .url
            .as_ref()
            .ok_or_else(|| StorageError::Qdrant("URL required for Qdrant connection".into()))?;

        let client = Qdrant::from_url(url)
            .build()
            .map_err(|e| StorageError::Qdrant(format!("Failed to connect to Qdrant at {}: {e}", redact_url(url))))?;

        Ok(Self { client, config })
    }

    /// Create storage without connecting. For test use only — all operations will fail.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_unconnected(config: QdrantConfig) -> Self {
        let url = config.url.as_deref().unwrap_or("http://localhost:6334");
        let client = Qdrant::from_url(url).build().unwrap();
        Self { client, config }
    }

    /// Initialize all four collections with indexes
    pub async fn initialize(&self) -> Result<()> {
        for collection in COLLECTIONS {
            self.create_collection_if_not_exists(collection).await?;
            self.create_indexes(collection).await?;
        }
        Ok(())
    }

    async fn create_collection_if_not_exists(&self, name: &str) -> Result<()> {
        let safe_url = redact_url(self.config.url.as_deref().unwrap_or("unknown"));
        let exists = self
            .client
            .collection_exists(name)
            .await
            .map_err(|e| StorageError::Qdrant(format!("Failed to check collection '{name}' at {safe_url}: {e}")))?;

        if !exists {
            self.client
                .create_collection(CreateCollectionBuilder::new(name).vectors_config(
                    VectorParamsBuilder::new(self.config.vector_size, Distance::Cosine),
                ))
                .await
                .map_err(|e| StorageError::Qdrant(format!("Failed to create collection '{name}' at {safe_url}: {e}")))?;

            tracing::info!("Created collection: {}", name);
        }

        Ok(())
    }

    async fn create_indexes(&self, collection: &str) -> Result<()> {
        let keyword_fields = [
            "user_id",
            "session_id",
            "entity_ids",
            "entity_types",
            "topic_tags",
            "supersedes_id",
            "content_hash",
            "source_type",
            "observation_level",
        ];

        for field in keyword_fields {
            self.client
                .create_field_index(CreateFieldIndexCollectionBuilder::new(
                    collection,
                    field,
                    FieldType::Keyword,
                ))
                .await
                .ok(); // Ignore if index already exists
        }

        // Boolean index
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection,
                "is_latest",
                FieldType::Bool,
            ))
            .await
            .ok();

        // Float index
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection,
                "confidence",
                FieldType::Float,
            ))
            .await
            .ok();

        // Datetime indexes
        for field in ["t_created", "t_valid", "session_timestamp", "t_event"] {
            self.client
                .create_field_index(CreateFieldIndexCollectionBuilder::new(
                    collection,
                    field,
                    FieldType::Datetime,
                ))
                .await
                .ok();
        }

        // Full-text index on content
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection,
                "content",
                FieldType::Text,
            ))
            .await
            .ok();

        tracing::debug!("Created indexes for collection: {}", collection);
        Ok(())
    }

    /// Upsert a memory with its embedding vector
    pub async fn upsert_memory(&self, memory: &Memory, vector: Vec<f32>) -> Result<()> {
        self.upsert_memory_with_payload_overrides(memory, vector, std::collections::HashMap::new()).await
    }

    /// Upsert a memory with its embedding vector and additional payload fields
    pub async fn upsert_memory_with_payload_overrides(
        &self,
        memory: &Memory,
        vector: Vec<f32>,
        payload_overrides: std::collections::HashMap<String, Value>,
    ) -> Result<()> {
        let collection = memory.collection();
        let payload =
            serde_json::to_value(memory).map_err(|e| StorageError::Qdrant(e.to_string()))?;

        let mut qdrant_payload = json_to_payload(&payload);
        qdrant_payload.extend(payload_overrides);

        let point = PointStruct::new(memory.id.to_string(), vector, qdrant_payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(collection, vec![point]))
            .await
            .map_err(|e| StorageError::Qdrant(format!("Failed to upsert memory {} to '{collection}': {e}", memory.id)))?;

        tracing::debug!("Upserted memory {} to {}", memory.id, collection);
        Ok(())
    }

    /// Get a memory by ID from any collection
    pub async fn get_memory(&self, user_id: &str, memory_id: Uuid) -> Result<Option<Memory>> {
        for collection in COLLECTIONS {
            if let Some(memory) = self
                .get_memory_from_collection(collection, user_id, memory_id)
                .await?
            {
                return Ok(Some(memory));
            }
        }
        Ok(None)
    }

    async fn get_memory_from_collection(
        &self,
        collection: &str,
        user_id: &str,
        memory_id: Uuid,
    ) -> Result<Option<Memory>> {
        let result = self
            .client
            .get_points(
                GetPointsBuilder::new(collection, vec![PointId::from(memory_id.to_string())])
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await
            .map_err(|e| StorageError::Qdrant(format!("Failed to get memory {memory_id} from '{collection}': {e}")))?;

        for point in result.result {
            if let Some(payload) = point.payload.get("user_id") {
                if let Some(Kind::StringValue(uid)) = &payload.kind {
                    if uid == user_id {
                        let memory = payload_to_memory(&point.payload)?;
                        return Ok(Some(memory));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Get a raw point by UUID string from a specific collection (returns payload only)
    pub async fn get_point_by_id(
        &self,
        collection: &str,
        point_id: &str,
    ) -> Result<Option<qdrant_client::qdrant::RetrievedPoint>> {
        let result = self
            .client
            .get_points(
                GetPointsBuilder::new(collection, vec![PointId::from(point_id.to_string())])
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        Ok(result.result.into_iter().next())
    }

    /// Get a raw point by UUID string, searching across all epistemic collections.
    /// Use when you have a fact_id but don't know which collection it lives in.
    /// Returns the first match found. Propagates transport/connection errors;
    /// treats "not found in this collection" as expected and continues.
    pub async fn get_point_by_id_any_collection(
        &self,
        point_id: &str,
    ) -> Result<Option<qdrant_client::qdrant::RetrievedPoint>> {
        for collection in COLLECTIONS {
            match self.get_point_by_id(collection, point_id).await {
                Ok(Some(point)) => return Ok(Some(point)),
                Ok(None) => continue,
                Err(e) => {
                    eprintln!(
                        "[WARN] get_point_by_id_any_collection: error in '{}' for '{}': {}",
                        collection, point_id, e
                    );
                    // Continue to next collection — may be a transient issue with one collection
                }
            }
        }
        Ok(None)
    }

    /// Soft-delete a memory (set t_expired)
    pub async fn delete_memory(&self, user_id: &str, memory_id: Uuid) -> Result<bool> {
        if let Some(mut memory) = self.get_memory(user_id, memory_id).await? {
            memory.t_expired = Some(chrono::Utc::now());
            memory.is_latest = false;

            let collection = memory.collection();
            let payload =
                serde_json::to_value(&memory).map_err(|e| StorageError::Qdrant(e.to_string()))?;

            self.client
                .set_payload(
                    SetPayloadPointsBuilder::new(collection, json_to_payload(&payload))
                        .points_selector(vec![PointId::from(memory_id.to_string())]),
                )
                .await
                .map_err(|e| StorageError::Qdrant(format!("Failed to delete memory {memory_id} in '{collection}': {e}")))?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Mark a memory as superseded (set is_latest=false, t_expired=now)
    /// Used by knowledge update consolidation
    pub async fn supersede_memory_in_collection(
        &self,
        collection: &str,
        memory_id: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now();
        let payload = std::collections::HashMap::from([
            (
                "is_latest".to_string(),
                Value {
                    kind: Some(Kind::BoolValue(false)),
                },
            ),
            (
                "t_expired".to_string(),
                Value {
                    kind: Some(Kind::StringValue(now.to_rfc3339())),
                },
            ),
        ]);

        self.client
            .set_payload(
                SetPayloadPointsBuilder::new(collection, payload)
                    .points_selector(vec![PointId::from(memory_id.to_string())]),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        Ok(())
    }

    /// Find similar facts in a collection by embedding (for contradiction detection)
    pub async fn find_similar_facts(
        &self,
        embedding: Vec<f32>,
        threshold: f32,
        limit: u64,
    ) -> Result<Vec<(String, String, f32, String)>> {
        // Returns: (point_id, content, score, collection)
        let mut results = Vec::new();

        for coll in COLLECTIONS {
            let search_result = self
                .client
                .search_points(
                    SearchPointsBuilder::new(coll, embedding.clone(), limit)
                        .filter(Filter::must([Condition::matches("is_latest", true)]))
                        .with_payload(true)
                        .with_vectors(false)
                        .score_threshold(threshold),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            for scored_point in search_result.result {
                let content = scored_point
                    .payload
                    .get("content")
                    .and_then(|v| match &v.kind {
                        Some(Kind::StringValue(s)) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let point_id = match scored_point.id {
                    Some(id) => format!("{:?}", id),
                    None => String::new(),
                };
                results.push((point_id, content, scored_point.score, coll.to_string()));
            }
        }

        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }

    /// List all current memories for a user
    pub async fn list_user_memories(
        &self,
        user_id: &str,
        collection: Option<&str>,
        limit: u64,
    ) -> Result<Vec<Memory>> {
        let collections = match collection {
            Some(c) => vec![c],
            None => COLLECTIONS.to_vec(),
        };

        let mut memories = Vec::new();

        for coll in collections {
            let filter = Filter::must([
                Condition::matches("user_id", user_id.to_string()),
                Condition::matches("is_latest", true),
            ]);

            let result = self
                .client
                .scroll(
                    ScrollPointsBuilder::new(coll)
                        .filter(filter)
                        .limit(limit as u32)
                        .with_payload(true)
                        .with_vectors(false),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            for point in result.result {
                if let Ok(memory) = payload_to_memory(&point.payload) {
                    memories.push(memory);
                }
            }
        }

        Ok(memories)
    }

    /// Search memories by vector similarity for a user
    pub async fn search_memories(
        &self,
        user_id: &str,
        query_vector: Vec<f32>,
        limit: u64,
        collection: Option<&str>,
    ) -> Result<Vec<(Memory, f32)>> {
        let collections = match collection {
            Some(c) => vec![c],
            None => COLLECTIONS.to_vec(),
        };

        let mut results = Vec::new();

        for coll in collections {
            let filter = Filter::must([
                Condition::matches("user_id", user_id.to_string()),
                Condition::matches("is_latest", true),
            ]);

            let search_result = self
                .client
                .search_points(
                    SearchPointsBuilder::new(coll, query_vector.clone(), limit)
                        .filter(filter)
                        .with_payload(true)
                        .with_vectors(false),
                )
                .await
                .map_err(|e| StorageError::Qdrant(format!("Failed to search memories in '{coll}': {e}")))?;

            for scored_point in search_result.result {
                if let Ok(memory) = payload_to_memory(&scored_point.payload) {
                    results.push((memory, scored_point.score));
                }
            }
        }

        // Sort by score descending and take top limit
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        Ok(results)
    }

    /// Search memories with a custom Qdrant filter
    ///
    /// Unlike `search_memories()`, this accepts an arbitrary filter, enabling
    /// temporal filtering, past-state queries, etc.
    pub async fn search_memories_with_filter(
        &self,
        filter: Filter,
        query_vector: Vec<f32>,
        limit: u64,
        collections: Option<&[&str]>,
    ) -> Result<Vec<(Memory, f32)>> {
        let colls: Vec<&str> = match collections {
            Some(c) => c.to_vec(),
            None => COLLECTIONS.to_vec(),
        };

        let mut results = Vec::new();

        for coll in colls {
            let search_result = self
                .client
                .search_points(
                    SearchPointsBuilder::new(coll, query_vector.clone(), limit)
                        .filter(filter.clone())
                        .with_payload(true)
                        .with_vectors(false),
                )
                .await
                .map_err(|e| StorageError::Qdrant(format!("Failed to search memories in '{coll}': {e}")))?;

            for scored_point in search_result.result {
                if let Ok(memory) = payload_to_memory(&scored_point.payload) {
                    results.push((memory, scored_point.score));
                }
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        Ok(results)
    }

    /// Hybrid search over fact collections (vector + fulltext with RRF fusion)
    ///
    /// Combines vector search results with full-text keyword matches for better recall.
    /// Full-text helps find exact keyword matches that vector embeddings may rank lower.
    pub async fn search_memories_hybrid(
        &self,
        filter: Filter,
        query_vector: Vec<f32>,
        query_text: &str,
        limit: u64,
        collections: Option<&[&str]>,
    ) -> Result<Vec<(Memory, f32)>> {
        let colls: Vec<&str> = match collections {
            Some(c) => c.to_vec(),
            None => COLLECTIONS.to_vec(),
        };

        // Vector search across all collections
        let mut vector_results: Vec<(Memory, f32)> = Vec::new();
        for coll in colls.iter() {
            let search_result = self
                .client
                .search_points(
                    SearchPointsBuilder::new(*coll, query_vector.clone(), limit)
                        .filter(filter.clone())
                        .with_payload(true)
                        .with_vectors(false),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            for scored_point in search_result.result {
                if let Ok(memory) = payload_to_memory(&scored_point.payload) {
                    vector_results.push((memory, scored_point.score));
                }
            }
        }
        vector_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Full-text search across all collections
        let text_condition: qdrant_client::qdrant::Condition =
            Condition::matches_text("content", query_text).into();
        let mut fulltext_must = filter.must.clone();
        fulltext_must.push(text_condition);
        let fulltext_filter = Filter {
            must: fulltext_must,
            should: filter.should.clone(),
            must_not: filter.must_not.clone(),
            min_should: filter.min_should.clone(),
        };

        let mut fulltext_results: Vec<Memory> = Vec::new();
        for coll in colls.iter() {
            let scroll_result = self
                .client
                .scroll(
                    ScrollPointsBuilder::new(*coll)
                        .filter(fulltext_filter.clone())
                        .limit(limit as u32)
                        .with_payload(true),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            for point in scroll_result.result {
                if let Ok(memory) = payload_to_memory(&point.payload) {
                    fulltext_results.push(memory);
                }
            }
        }

        // RRF fusion
        let k = 60.0_f32;
        let mut rrf_scores: std::collections::HashMap<String, (f32, Memory)> =
            std::collections::HashMap::new();

        for (rank, (memory, _score)) in vector_results.into_iter().enumerate() {
            let id = memory.id.to_string();
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            rrf_scores.entry(id).or_insert((0.0, memory)).0 += rrf_score;
        }

        for (rank, memory) in fulltext_results.into_iter().enumerate() {
            let id = memory.id.to_string();
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            let entry = rrf_scores.entry(id).or_insert((0.0, memory));
            entry.0 += rrf_score;
        }

        let mut fused: Vec<(Memory, f32)> = rrf_scores
            .into_values()
            .map(|(score, mem)| (mem, score))
            .collect();
        fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // A5: Session-diverse retrieval — cap at max_per_session results per session_id,
        // backfill from lower-ranked results from different sessions
        let max_per_session = 3u64;
        if fused.len() > limit as usize {
            let mut session_counts: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            let mut diverse: Vec<(Memory, f32)> = Vec::with_capacity(limit as usize);
            let mut overflow: Vec<(Memory, f32)> = Vec::new();

            for item in fused {
                let sid = item.0.session_id.clone().unwrap_or_default();
                let count = session_counts.entry(sid).or_insert(0);
                if *count < max_per_session {
                    *count += 1;
                    diverse.push(item);
                } else {
                    overflow.push(item);
                }
                if diverse.len() >= limit as usize {
                    break;
                }
            }

            // Backfill from overflow if we haven't hit the limit
            if diverse.len() < limit as usize {
                for item in overflow {
                    diverse.push(item);
                    if diverse.len() >= limit as usize {
                        break;
                    }
                }
            }
            fused = diverse;
        } else {
            fused.truncate(limit as usize);
        }

        Ok(fused)
    }

    // ---- Messages Collection Methods (Epic 003) ----

    /// Initialize the messages collection with appropriate indexes
    pub async fn initialize_messages_collection(&self) -> Result<()> {
        self.create_collection_if_not_exists(MESSAGES_COLLECTION)
            .await?;

        // Keyword indexes for filtering
        for field in ["session_id", "role", "peer_name", "user_id"] {
            self.client
                .create_field_index(CreateFieldIndexCollectionBuilder::new(
                    MESSAGES_COLLECTION,
                    field,
                    FieldType::Keyword,
                ))
                .await
                .ok();
        }

        // Integer index for turn ordering
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                MESSAGES_COLLECTION,
                "turn_index",
                FieldType::Integer,
            ))
            .await
            .ok();

        // Fulltext index for grep/search
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                MESSAGES_COLLECTION,
                "content",
                FieldType::Text,
            ))
            .await
            .ok();

        // Datetime index for temporal filtering
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                MESSAGES_COLLECTION,
                "t_valid",
                FieldType::Datetime,
            ))
            .await
            .ok();

        tracing::info!("Initialized messages collection with indexes");
        Ok(())
    }

    /// Upsert a single raw message turn into the messages collection
    pub async fn upsert_message(
        &self,
        id: &str,
        vector: Vec<f32>,
        content: &str,
        session_id: &str,
        turn_index: u64,
        role: &str,
        t_valid: Option<chrono::DateTime<chrono::Utc>>,
        peer_name: Option<&str>,
    ) -> Result<()> {
        let mut payload = std::collections::HashMap::new();
        payload.insert(
            "content".to_string(),
            Value {
                kind: Some(Kind::StringValue(content.to_string())),
            },
        );
        payload.insert(
            "session_id".to_string(),
            Value {
                kind: Some(Kind::StringValue(session_id.to_string())),
            },
        );
        payload.insert(
            "turn_index".to_string(),
            Value {
                kind: Some(Kind::IntegerValue(turn_index as i64)),
            },
        );
        payload.insert(
            "role".to_string(),
            Value {
                kind: Some(Kind::StringValue(role.to_string())),
            },
        );

        if let Some(t) = t_valid {
            payload.insert(
                "t_valid".to_string(),
                Value {
                    kind: Some(Kind::StringValue(t.to_rfc3339())),
                },
            );
        }
        if let Some(name) = peer_name {
            payload.insert(
                "peer_name".to_string(),
                Value {
                    kind: Some(Kind::StringValue(name.to_string())),
                },
            );
        }

        let point = PointStruct::new(id.to_string(), vector, payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(MESSAGES_COLLECTION, vec![point]))
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        Ok(())
    }

    /// Batch upsert raw message points into messages collection
    pub async fn upsert_messages_batch(&self, points: Vec<PointStruct>) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }
        self.client
            .upsert_points(UpsertPointsBuilder::new(MESSAGES_COLLECTION, points))
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;
        Ok(())
    }

    /// Hybrid search over messages collection (vector + fulltext, returns vector results)
    ///
    /// Searches messages by vector similarity. Fulltext fusion is handled at the
    /// answerer level when merging with fact results.
    pub async fn search_messages_hybrid(
        &self,
        query_vector: Vec<f32>,
        query_text: &str,
        filter: Option<Filter>,
        top_k: usize,
    ) -> Result<Vec<qdrant_client::qdrant::ScoredPoint>> {
        // Vector search
        let mut vector_search =
            SearchPointsBuilder::new(MESSAGES_COLLECTION, query_vector, top_k as u64)
                .with_payload(true);

        if let Some(ref f) = filter {
            vector_search = vector_search.filter(f.clone());
        }

        let vector_results = self
            .client
            .search_points(vector_search)
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?
            .result;

        // Fulltext search
        let text_condition = Condition::matches_text("content", query_text);
        let fulltext_filter = if let Some(ref f) = filter {
            let mut combined_must: Vec<qdrant_client::qdrant::Condition> = f.must.clone();
            combined_must.push(text_condition.into());
            Filter {
                must: combined_must,
                should: f.should.clone(),
                must_not: f.must_not.clone(),
                min_should: f.min_should.clone(),
            }
        } else {
            Filter::must([text_condition])
        };

        let fulltext_results = self
            .client
            .scroll(
                ScrollPointsBuilder::new(MESSAGES_COLLECTION)
                    .filter(fulltext_filter)
                    .limit(top_k as u32)
                    .with_payload(true),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?
            .result;

        // RRF fusion: score each result by 1/(k+rank) across both channels
        let k = 60.0_f32; // RRF constant
        let mut rrf_scores: std::collections::HashMap<
            String,
            (f32, qdrant_client::qdrant::ScoredPoint),
        > = std::collections::HashMap::new();

        // Score vector results by rank
        for (rank, point) in vector_results.into_iter().enumerate() {
            let id = point
                .id
                .as_ref()
                .map(|id| format!("{:?}", id))
                .unwrap_or_default();
            let score = 1.0 / (k + rank as f32 + 1.0);
            rrf_scores.entry(id).or_insert((0.0, point)).0 += score;
        }

        // Score fulltext results by rank
        for (rank, point) in fulltext_results.into_iter().enumerate() {
            let id = point
                .id
                .as_ref()
                .map(|id| format!("{:?}", id))
                .unwrap_or_default();
            let score = 1.0 / (k + rank as f32 + 1.0);
            let entry = rrf_scores.entry(id).or_insert_with(|| {
                (
                    0.0,
                    qdrant_client::qdrant::ScoredPoint {
                        id: point.id,
                        payload: point.payload,
                        score: 0.0,
                        version: 0,
                        vectors: None,
                        shard_key: point.shard_key,
                        order_value: point.order_value,
                    },
                )
            });
            entry.0 += score;
        }

        // Sort by RRF score descending
        let mut fused: Vec<_> = rrf_scores.into_values().collect();
        fused.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // A5: Session-diverse retrieval for messages — cap per session_id
        let max_per_session = 5usize;
        let mut session_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut diverse: Vec<(f32, qdrant_client::qdrant::ScoredPoint)> = Vec::with_capacity(top_k);
        let mut overflow: Vec<(f32, qdrant_client::qdrant::ScoredPoint)> = Vec::new();

        for item in fused {
            let sid = item
                .1
                .payload
                .get("session_id")
                .and_then(|v| v.kind.as_ref())
                .and_then(|k| {
                    if let Kind::StringValue(s) = k {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let count = session_counts.entry(sid).or_insert(0);
            if *count < max_per_session {
                *count += 1;
                diverse.push(item);
            } else {
                overflow.push(item);
            }
            if diverse.len() >= top_k {
                break;
            }
        }
        if diverse.len() < top_k {
            for item in overflow {
                diverse.push(item);
                if diverse.len() >= top_k {
                    break;
                }
            }
        }

        Ok(diverse
            .into_iter()
            .map(|(score, mut point)| {
                point.score = score;
                point
            })
            .collect())
    }

    /// Scroll messages collection with a filter (for grep, session context, etc.)
    pub async fn scroll_messages(
        &self,
        filter: Filter,
        limit: u32,
    ) -> Result<Vec<qdrant_client::qdrant::RetrievedPoint>> {
        let result = self
            .client
            .scroll(
                ScrollPointsBuilder::new(MESSAGES_COLLECTION)
                    .filter(filter)
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;
        Ok(result.result)
    }

    /// Scroll any collection with a filter (generic helper for cross-collection queries)
    pub async fn scroll_collection(
        &self,
        collection: &str,
        filter: Filter,
        limit: u32,
    ) -> Result<Vec<qdrant_client::qdrant::RetrievedPoint>> {
        let result = self
            .client
            .scroll(
                ScrollPointsBuilder::new(collection)
                    .filter(filter)
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;
        Ok(result.result)
    }

    /// Scroll all messages belonging to any of the given session IDs.
    /// Used by session-level NDCG retrieval to fetch complete sessions.
    pub async fn scroll_messages_by_session_ids(
        &self,
        session_ids: &[String],
        limit_per_session: u32,
    ) -> Result<Vec<qdrant_client::qdrant::RetrievedPoint>> {
        if session_ids.is_empty() {
            return Ok(vec![]);
        }

        // Use should-filter to match any of the session IDs (OR)
        let conditions: Vec<qdrant_client::qdrant::Condition> = session_ids
            .iter()
            .map(|sid| Condition::matches("session_id", sid.clone()).into())
            .collect();

        let filter = Filter {
            must: vec![],
            should: conditions,
            must_not: vec![],
            min_should: None,
        };

        let total_limit = limit_per_session * session_ids.len() as u32;
        self.scroll_messages(filter, total_limit).await
    }

    /// Get message count in the messages collection
    pub async fn get_messages_count(&self) -> Result<u64> {
        let exists = self
            .client
            .collection_exists(MESSAGES_COLLECTION)
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        if !exists {
            return Ok(0);
        }

        let info = self
            .client
            .collection_info(MESSAGES_COLLECTION)
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;

        Ok(info.result.and_then(|r| r.points_count).unwrap_or(0))
    }

    /// Delete and recreate all collections (for clean benchmark runs)
    pub async fn clear_all_collections(&self) -> Result<()> {
        // Delete fact collections
        for collection in COLLECTIONS {
            let exists = self
                .client
                .collection_exists(collection)
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            if exists {
                self.client
                    .delete_collection(collection)
                    .await
                    .map_err(|e| StorageError::Qdrant(e.to_string()))?;
                tracing::info!("Deleted collection: {}", collection);
            }
        }

        // Delete messages collection
        let msg_exists = self
            .client
            .collection_exists(MESSAGES_COLLECTION)
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;
        if msg_exists {
            self.client
                .delete_collection(MESSAGES_COLLECTION)
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;
            tracing::info!("Deleted collection: {}", MESSAGES_COLLECTION);
        }

        // Recreate with indexes
        self.initialize().await?;
        self.initialize_messages_collection().await?;
        tracing::info!("Recreated all collections");
        Ok(())
    }

    /// Fetch all memories belonging to specific sessions
    ///
    /// Used for session-level retrieval expansion: after finding top facts,
    /// fetch all facts from those sessions to provide full conversational context.
    pub async fn get_memories_by_session_ids(
        &self,
        user_id: &str,
        session_ids: &[String],
        limit_per_session: u64,
    ) -> Result<Vec<Memory>> {
        if session_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut all_memories = Vec::new();

        for coll in COLLECTIONS {
            // Build filter: user_id must match AND session_id must be one of the given IDs
            let filter = Filter {
                must: vec![Condition::matches("user_id", user_id.to_string()).into()],
                should: session_ids
                    .iter()
                    .map(|sid| Condition::matches("session_id", sid.to_string()).into())
                    .collect(),
                must_not: vec![],
                min_should: None,
            };

            let scroll_result = self
                .client
                .scroll(
                    ScrollPointsBuilder::new(coll)
                        .filter(filter)
                        .limit((limit_per_session * session_ids.len() as u64).min(500) as u32)
                        .with_payload(true)
                        .with_vectors(false),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            for point in scroll_result.result {
                if let Ok(memory) = payload_to_memory(&point.payload) {
                    all_memories.push(memory);
                }
            }
        }

        Ok(all_memories)
    }

    /// Search memories by full-text keyword matching
    ///
    /// Uses Qdrant's text index on `content` field. Returns matching memories
    /// with a keyword overlap score (fraction of query keywords found in content).
    /// Keywords are matched individually using OR logic (should filter).
    pub async fn search_memories_fulltext(
        &self,
        user_id: &str,
        keywords: &[String],
        limit: u64,
        collections: Option<&[&str]>,
    ) -> Result<Vec<(Memory, f32)>> {
        if keywords.is_empty() {
            return Ok(vec![]);
        }

        let colls: Vec<&str> = match collections {
            Some(c) => c.to_vec(),
            None => COLLECTIONS.to_vec(),
        };

        // Build filter: must match user_id, should match any keyword in content
        // Use matches_text() for full-text search (tokenized), not matches() which does exact keyword match
        let should_conditions: Vec<qdrant_client::qdrant::Condition> = keywords
            .iter()
            .map(|kw| Condition::matches_text("content", kw).into())
            .collect();

        let filter = Filter {
            must: vec![Condition::matches("user_id", user_id.to_string()).into()],
            should: should_conditions,
            must_not: vec![],
            min_should: None,
        };

        let mut results = Vec::new();

        for coll in colls {
            let scroll_result = self
                .client
                .scroll(
                    ScrollPointsBuilder::new(coll)
                        .filter(filter.clone())
                        .limit(limit as u32)
                        .with_payload(true)
                        .with_vectors(false),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            for point in scroll_result.result {
                if let Ok(memory) = payload_to_memory(&point.payload) {
                    // Score by keyword overlap: fraction of keywords found in content
                    let content_lower = memory.content.to_lowercase();
                    let matched = keywords
                        .iter()
                        .filter(|kw| content_lower.contains(&kw.to_lowercase()))
                        .count();
                    let score = matched as f32 / keywords.len() as f32;
                    results.push((memory, score));
                }
            }
        }

        // Sort by score descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        Ok(results)
    }

    /// Check if Qdrant is healthy
    pub async fn health_check(&self) -> Result<bool> {
        self.client
            .health_check()
            .await
            .map(|_| true)
            .map_err(|e| StorageError::Qdrant(e.to_string()).into())
    }

    /// Count total points for a specific user across all collections (facts + messages)
    pub async fn count_user_points(&self, user_id: &str) -> Result<u64> {
        let user_filter = Filter::must([Condition::matches("user_id", user_id.to_string())]);
        let mut total = 0u64;

        // Count across fact collections
        for collection in COLLECTIONS {
            let exists = self
                .client
                .collection_exists(collection)
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;
            if !exists {
                continue;
            }

            let resp = self
                .client
                .count(
                    CountPointsBuilder::new(collection)
                        .filter(user_filter.clone())
                        .exact(true),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;
            total += resp.result.map(|r| r.count).unwrap_or(0);
        }

        // Count messages collection
        let msg_exists = self
            .client
            .collection_exists(MESSAGES_COLLECTION)
            .await
            .map_err(|e| StorageError::Qdrant(e.to_string()))?;
        if msg_exists {
            let resp = self
                .client
                .count(
                    CountPointsBuilder::new(MESSAGES_COLLECTION)
                        .filter(user_filter)
                        .exact(true),
                )
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;
            total += resp.result.map(|r| r.count).unwrap_or(0);
        }

        Ok(total)
    }

    /// Get memory counts per collection
    pub async fn get_collection_counts(&self) -> Result<Vec<(String, u64)>> {
        let mut counts = Vec::new();

        for collection in COLLECTIONS {
            let info = self
                .client
                .collection_info(collection)
                .await
                .map_err(|e| StorageError::Qdrant(e.to_string()))?;

            let count = info.result.and_then(|r| r.points_count).unwrap_or(0);
            counts.push((collection.to_string(), count));
        }

        Ok(counts)
    }
}

// Helper: Convert serde_json::Value to Qdrant payload
fn json_to_payload(value: &serde_json::Value) -> std::collections::HashMap<String, Value> {
    let mut payload = std::collections::HashMap::new();

    if let serde_json::Value::Object(map) = value {
        for (k, v) in map {
            payload.insert(k.clone(), json_value_to_qdrant(v));
        }
    }

    payload
}

fn json_value_to_qdrant(value: &serde_json::Value) -> Value {
    Value {
        kind: Some(match value {
            serde_json::Value::Null => Kind::NullValue(0),
            serde_json::Value::Bool(b) => Kind::BoolValue(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Kind::IntegerValue(i)
                } else {
                    Kind::DoubleValue(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => Kind::StringValue(s.clone()),
            serde_json::Value::Array(arr) => Kind::ListValue(qdrant_client::qdrant::ListValue {
                values: arr.iter().map(json_value_to_qdrant).collect(),
            }),
            serde_json::Value::Object(map) => Kind::StructValue(qdrant_client::qdrant::Struct {
                fields: map
                    .iter()
                    .map(|(k, v)| (k.clone(), json_value_to_qdrant(v)))
                    .collect(),
            }),
        }),
    }
}

// Helper: Convert Qdrant payload back to Memory
fn payload_to_memory(payload: &std::collections::HashMap<String, Value>) -> Result<Memory> {
    let json = qdrant_payload_to_json(payload);
    serde_json::from_value(json).map_err(|e| StorageError::Qdrant(e.to_string()).into())
}

fn qdrant_payload_to_json(payload: &std::collections::HashMap<String, Value>) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = payload
        .iter()
        .map(|(k, v)| (k.clone(), qdrant_value_to_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

fn qdrant_value_to_json(value: &Value) -> serde_json::Value {
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
        Some(Kind::StructValue(s)) => qdrant_payload_to_json(&s.fields),
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collections_count() {
        assert_eq!(COLLECTIONS.len(), 4);
        assert!(COLLECTIONS.contains(&"world"));
        assert!(COLLECTIONS.contains(&"experience"));
        assert!(COLLECTIONS.contains(&"opinion"));
        assert!(COLLECTIONS.contains(&"observation"));
    }

    #[test]
    fn test_json_to_payload_conversion() {
        let json = serde_json::json!({
            "user_id": "test_user",
            "confidence": 0.95,
            "is_latest": true
        });

        let payload = json_to_payload(&json);

        assert!(payload.contains_key("user_id"));
        assert!(payload.contains_key("confidence"));
        assert!(payload.contains_key("is_latest"));
    }

    #[test]
    fn test_payload_roundtrip() {
        let original = serde_json::json!({
            "name": "test",
            "count": 42,
            "active": true,
            "tags": ["a", "b", "c"]
        });

        let payload = json_to_payload(&original);
        let recovered = qdrant_payload_to_json(&payload);

        assert_eq!(original, recovered);
    }

    #[test]
    fn test_memory_routes_to_correct_collection() {
        use crate::types::EpistemicType;

        // Test that each epistemic type routes to correct collection
        let world_memory =
            Memory::new("user", "fact about the world").with_epistemic_type(EpistemicType::World);
        assert_eq!(world_memory.collection(), "world");

        let experience_memory =
            Memory::new("user", "my experience").with_epistemic_type(EpistemicType::Experience);
        assert_eq!(experience_memory.collection(), "experience");

        let opinion_memory =
            Memory::new("user", "I think this").with_epistemic_type(EpistemicType::Opinion);
        assert_eq!(opinion_memory.collection(), "opinion");

        let observation_memory = Memory::new("user", "observed entity data")
            .with_epistemic_type(EpistemicType::Observation);
        assert_eq!(observation_memory.collection(), "observation");
    }

    #[test]
    fn test_all_collections_defined() {
        // Verify all four epistemic types have corresponding collections
        assert_eq!(COLLECTIONS.len(), 4);

        // Each collection name should match an epistemic type's collection_name()
        use crate::types::EpistemicType;
        let types = [
            EpistemicType::World,
            EpistemicType::Experience,
            EpistemicType::Opinion,
            EpistemicType::Observation,
        ];

        for typ in types {
            let collection_name = typ.collection_name();
            assert!(
                COLLECTIONS.contains(&collection_name),
                "Collection {} not found in COLLECTIONS",
                collection_name
            );
        }
    }

    #[test]
    fn test_messages_collection_constant() {
        assert_eq!(MESSAGES_COLLECTION, "messages");
        // Messages collection is separate from fact collections
        assert!(!COLLECTIONS.contains(&MESSAGES_COLLECTION));
    }
}
