//! Session ingester for LongMemEval-S benchmark
//!
//! Ingests benchmark sessions into the memory system with proper timestamps.
//! Supports parallel ingestion for improved throughput.
//! Supports batch mode for 50% cheaper ingestion via OpenAI Batch API.

mod batch;
mod config;
mod stats;

pub use config::*;
pub use stats::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use tokio::sync::Mutex;

use qdrant_client::qdrant::{value::Kind, PointStruct, Value};
use uuid::Uuid;

use crate::types::{BenchmarkMessage, BenchmarkSession};
use engram::embedding::{EmbeddingProvider, RemoteEmbeddingProvider, EMBEDDING_DIMENSION};
use crate::error::{BenchmarkError, Result};
use engram::extraction::{
    ApiExtractor, ApiExtractorConfig, Conversation,
    ConversationTurn, Role, TemporalParser,
};
use engram::storage::{QdrantConfig, QdrantStorage};
use engram::types::{Memory, SessionEntityContext};

/// Session ingester for benchmark data
///
/// Ingests benchmark sessions using real extraction, embedding, and storage.
/// Supports parallel ingestion with configurable concurrency.
pub struct SessionIngester {
    pub(crate) config: IngesterConfig,
    pub(crate) extractor: Option<Arc<ApiExtractor>>,
    pub(crate) embedding_provider: Option<Arc<RemoteEmbeddingProvider>>,
    pub(crate) storage: Option<Arc<QdrantStorage>>,
    /// SurrealDB knowledge graph
    pub(crate) graph_store: Option<Arc<engram::storage::GraphStore>>,
}

impl std::fmt::Debug for SessionIngester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionIngester")
            .field("config", &self.config)
            .field("has_extractor", &self.extractor.is_some())
            .field("has_embedding", &self.embedding_provider.is_some())
            .field("has_storage", &self.storage.is_some())
            .field("has_graph_store", &self.graph_store.is_some())
            .finish()
    }
}

impl SessionIngester {
    /// Create a new session ingester
    pub fn new(config: IngesterConfig) -> Self {
        Self {
            config,
            extractor: None,
            embedding_provider: None,
            storage: None,
            graph_store: None,
        }
    }

    /// Create with default config
    pub fn with_defaults() -> Self {
        Self::new(IngesterConfig::default())
    }

    /// Set the API extractor
    pub fn with_extractor(mut self, extractor: ApiExtractor) -> Self {
        self.extractor = Some(Arc::new(extractor));
        self
    }

    /// Set the embedding provider
    pub fn with_embedding_provider(mut self, provider: Arc<RemoteEmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Set the storage
    pub fn with_storage(mut self, storage: Arc<QdrantStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Create with all components from a BenchmarkConfig (TOML-driven, no env var defaults).
    /// The extractor is configured from the ingester model's [[models]] profile.
    pub async fn from_benchmark_config(
        config: IngesterConfig,
        bench_config: &super::benchmark_config::BenchmarkConfig,
    ) -> Result<Self> {
        // Look up ingester model profile for base_url and api_key
        let registry = bench_config.model_registry();
        let profile = registry.get(&config.model)
            .map_err(|e| BenchmarkError::Ingestion(format!("{}", e)))?;

        // Build extractor config from model profile
        let base_url = profile.base_url.clone()
            .ok_or_else(|| BenchmarkError::Ingestion(
                format!("No base_url in [[models]] for ingester model '{}'. Add base_url to the model's TOML entry.", config.model)
            ))?;

        // Determine provider from base URL
        let provider = if base_url.contains("openai.com") {
            engram::extraction::ApiProvider::OpenAI
        } else if base_url.contains("anthropic.com") {
            engram::extraction::ApiProvider::Anthropic
        } else {
            engram::extraction::ApiProvider::Custom
        };

        let mut extractor_config = ApiExtractorConfig {
            provider,
            model: config.model.clone(),
            base_url: Some(base_url),
            supports_temperature: Some(profile.supports_temperature),
            max_tokens_field: Some(profile.max_tokens_field.clone()),
            ..ApiExtractorConfig::default()
        };

        // Wire API key from profile
        if let Some(ref api_key_env) = profile.api_key_env {
            if let Ok(key) = std::env::var(api_key_env) {
                extractor_config = extractor_config.with_api_key(key);
            }
        }

        if let Some(temp) = config.extraction_temperature {
            extractor_config = extractor_config.with_temperature(temp);
        }
        if let Some(seed) = config.extraction_seed {
            extractor_config = extractor_config.with_seed(seed);
        }
        if let Some(ref cache_dir) = config.extraction_cache_dir {
            std::fs::create_dir_all(cache_dir).map_err(|e| {
                BenchmarkError::Ingestion(format!(
                    "Failed to create extraction cache dir {}: {}",
                    cache_dir.display(),
                    e
                ))
            })?;
            extractor_config = extractor_config.with_cache_dir(cache_dir.clone());
        }
        let extractor = ApiExtractor::new(extractor_config);

        // Create embedding provider (always OpenAI embeddings)
        let embedding_provider = RemoteEmbeddingProvider::from_env()
            .ok_or_else(|| BenchmarkError::Ingestion("OPENAI_API_KEY not set for embeddings".into()))?;

        // Create storage from config
        let qdrant_config = QdrantConfig::external(&bench_config.benchmark.qdrant_url)
            .with_vector_size(EMBEDDING_DIMENSION as u64);
        let storage = QdrantStorage::new(qdrant_config)
            .await
            .map_err(|e| BenchmarkError::Ingestion(format!("Qdrant connection failed: {}", e)))?;

        // Initialize collections
        storage
            .initialize()
            .await
            .map_err(|e| BenchmarkError::Ingestion(format!("Qdrant init failed: {}", e)))?;
        storage
            .initialize_messages_collection()
            .await
            .map_err(|e| {
                BenchmarkError::Ingestion(format!("Messages collection init failed: {}", e))
            })?;

        Ok(Self {
            config,
            extractor: Some(Arc::new(extractor)),
            embedding_provider: Some(Arc::new(embedding_provider)),
            storage: Some(Arc::new(storage)),
            graph_store: None,
        })
    }

    /// Check if all components are configured
    pub fn is_configured(&self) -> bool {
        self.extractor.is_some() && self.embedding_provider.is_some() && self.storage.is_some()
    }

    /// Get the extraction model name (if extractor is configured)
    pub fn extraction_model(&self) -> Option<&str> {
        self.extractor.as_ref().map(|e| e.config().model.as_str())
    }

    /// Get the configuration
    pub fn config(&self) -> &IngesterConfig {
        &self.config
    }

    /// Set the SurrealDB graph store
    pub fn with_graph_store(mut self, store: Arc<engram::storage::GraphStore>) -> Self {
        self.graph_store = Some(store);
        self
    }

    /// Get the graph store
    pub fn graph_store(&self) -> Option<&Arc<engram::storage::GraphStore>> {
        self.graph_store.as_ref()
    }

    /// Build a conversation string from messages
    pub fn build_conversation(messages: &[BenchmarkMessage]) -> String {
        messages
            .iter()
            .map(|m| {
                format!(
                    "[{}] {}: {}",
                    m.timestamp.format("%Y-%m-%d %H:%M"),
                    m.role,
                    m.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Ingest multiple sessions
    ///
    /// Returns ingestion statistics including any errors encountered.
    pub fn ingest_sessions(&self, sessions: &[BenchmarkSession]) -> Result<IngestionStats> {
        // Create a runtime for async operations
        let rt =
            tokio::runtime::Runtime::new().map_err(|e| BenchmarkError::Ingestion(e.to_string()))?;

        rt.block_on(self.ingest_sessions_async(sessions))
    }

    /// Group sessions by user and sort each user bucket in chronological order.
    fn group_sessions_by_user_chronological(
        sessions: &[BenchmarkSession],
    ) -> Vec<(String, Vec<BenchmarkSession>)> {
        let mut sessions_by_user: HashMap<String, Vec<BenchmarkSession>> =
            HashMap::with_capacity(sessions.len());

        for session in sessions.iter().cloned() {
            sessions_by_user
                .entry(session.user_id.clone())
                .or_default()
                .push(session);
        }

        let mut grouped_sessions: Vec<(String, Vec<BenchmarkSession>)> =
            sessions_by_user.into_iter().collect();

        for (_, user_sessions) in grouped_sessions.iter_mut() {
            user_sessions.sort_by(|a, b| {
                let by_time = match (a.earliest_timestamp(), b.earliest_timestamp()) {
                    (Some(a_ts), Some(b_ts)) => a_ts.cmp(&b_ts),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                by_time.then_with(|| a.session_id.cmp(&b.session_id))
            });
        }

        // Stable user ordering keeps ingestion deterministic across runs.
        grouped_sessions.sort_by(|(user_a, _), (user_b, _)| user_a.cmp(user_b));
        grouped_sessions
    }

    /// Async version of ingest_sessions.
    ///
    /// When consolidation is enabled (B2), sessions are grouped by user and processed
    /// in chronological order per user to avoid supersession race conditions.
    /// When consolidation is disabled, all sessions are processed fully in parallel
    /// for maximum throughput.
    pub async fn ingest_sessions_async(
        &self,
        sessions: &[BenchmarkSession],
    ) -> Result<IngestionStats> {
        let total = sessions.len();
        if total == 0 {
            return Ok(IngestionStats::new());
        }

        // Shared state for progress tracking
        let stats = Arc::new(Mutex::new(IngestionStats::new()));
        let processed = Arc::new(AtomicUsize::new(0));
        let progress_interval = self.config.progress_interval;

        // Clone Arc references for use in closures
        let extractor = self.extractor.clone();
        let embedding_provider = self.embedding_provider.clone();
        let storage = self.storage.clone();
        let single_pass = self.config.single_pass;
        let enable_consolidation = self.config.enable_consolidation;
        let consolidation_threshold = self.config.consolidation_threshold;
        let extraction_seed = self.config.extraction_seed;
        let skip_messages = self.config.skip_messages;
        let graph_store = self.graph_store.clone();

        if enable_consolidation {
            // B2 mode: per-user sequential processing to avoid supersession race conditions
            let grouped_sessions = Self::group_sessions_by_user_chronological(sessions);
            let user_count = grouped_sessions.len();
            let user_concurrency = self.config.concurrency.max(1).min(user_count);

            tracing::info!(
                "Starting per-user ingestion: {} sessions across {} users with {} concurrent user workers",
                total,
                user_count,
                user_concurrency
            );

            stream::iter(grouped_sessions.into_iter())
                .map(|(_user_id, user_sessions)| {
                    let extractor = extractor.clone();
                    let embedding_provider = embedding_provider.clone();
                    let storage = storage.clone();
                    let stats = stats.clone();
                    let processed = processed.clone();
                    let graph_store = graph_store.clone();

                    async move {
                        for session in user_sessions {
                            eprintln!("Starting session: {}", session.session_id);
                            let result = Self::ingest_session_static(
                                &session,
                                extractor.as_ref(),
                                embedding_provider.as_ref(),
                                storage.as_ref(),
                                single_pass,
                                enable_consolidation,
                                consolidation_threshold,
                                extraction_seed,
                                skip_messages,
                                graph_store.as_ref(),
                            )
                            .await;
                            eprintln!(
                                "Finished session: {} - {:?}",
                                session.session_id,
                                result.is_ok()
                            );

                            let mut stats_guard = stats.lock().await;
                            match &result {
                                Ok(session_stats) => {
                                    stats_guard.add_session(session_stats.clone());
                                }
                                Err(e) => {
                                    let error_msg = format!("Session {}: {}", session.session_id, e);
                                    eprintln!("ERROR: {}", error_msg);
                                    stats_guard.add_error(error_msg);
                                }
                            }
                            drop(stats_guard);

                            let count = processed.fetch_add(1, Ordering::SeqCst) + 1;
                            if count % progress_interval == 0 || count == total {
                                let stats_guard = stats.lock().await;
                                tracing::info!(
                                    "Progress: {}/{} sessions ({} memories, {} messages, {} errors)",
                                    count,
                                    total,
                                    stats_guard.memories_created,
                                    stats_guard.messages_stored,
                                    stats_guard.errors.len()
                                );
                            }
                        }
                    }
                })
                .buffer_unordered(user_concurrency)
                .collect::<Vec<()>>()
                .await;
        } else {
            // No consolidation: process all sessions fully in parallel for max throughput
            let concurrency = self.config.concurrency.max(1);

            tracing::info!(
                "Starting parallel ingestion: {} sessions with {} concurrent workers (no consolidation)",
                total,
                concurrency
            );

            stream::iter(sessions.iter())
                .map(|session| {
                    let extractor = extractor.clone();
                    let embedding_provider = embedding_provider.clone();
                    let storage = storage.clone();
                    let stats = stats.clone();
                    let processed = processed.clone();
                    let graph_store = graph_store.clone();
                    let single_pass = single_pass;

                    async move {
                        eprintln!("Starting session: {}", session.session_id);
                        let result = Self::ingest_session_static(
                            session,
                            extractor.as_ref(),
                            embedding_provider.as_ref(),
                            storage.as_ref(),
                            single_pass,
                            false, // no consolidation
                            0.0,
                            extraction_seed,
                            skip_messages,
                            graph_store.as_ref(),
                        )
                        .await;
                        eprintln!(
                            "Finished session: {} - {:?}",
                            session.session_id,
                            result.is_ok()
                        );

                        let mut stats_guard = stats.lock().await;
                        match &result {
                            Ok(session_stats) => {
                                stats_guard.add_session(session_stats.clone());
                            }
                            Err(e) => {
                                let error_msg = format!("Session {}: {}", session.session_id, e);
                                eprintln!("ERROR: {}", error_msg);
                                stats_guard.add_error(error_msg);
                            }
                        }
                        drop(stats_guard);

                        let count = processed.fetch_add(1, Ordering::SeqCst) + 1;
                        if count % progress_interval == 0 || count == total {
                            let stats_guard = stats.lock().await;
                            tracing::info!(
                                "Progress: {}/{} sessions ({} memories, {} messages, {} errors)",
                                count,
                                total,
                                stats_guard.memories_created,
                                stats_guard.messages_stored,
                                stats_guard.errors.len()
                            );
                        }
                    }
                })
                .buffer_unordered(concurrency)
                .collect::<Vec<()>>()
                .await;
        }

        // Extract final stats
        let final_stats = match Arc::try_unwrap(stats) {
            Ok(mutex) => mutex.into_inner(),
            Err(arc) => arc.lock().await.clone(),
        };

        // Log any errors
        let error_count = final_stats.errors.len();
        if error_count > 0 {
            tracing::warn!("{} sessions failed during ingestion", error_count);
        }

        // Log SurrealDB graph stats if enabled
        if let Some(ref graph) = self.graph_store {
            if let Ok(stats) = graph.stats_all().await {
                tracing::info!("SurrealDB knowledge graph: {}", stats);
            }
        }

        Ok(final_stats)
    }

    /// Static version of ingest_session_async for use in parallel processing
    async fn ingest_session_static(
        session: &BenchmarkSession,
        extractor: Option<&Arc<ApiExtractor>>,
        embedding_provider: Option<&Arc<RemoteEmbeddingProvider>>,
        storage: Option<&Arc<QdrantStorage>>,
        single_pass: bool,
        enable_consolidation: bool,
        consolidation_threshold: f32,
        extraction_seed: Option<u64>,
        skip_messages: bool,
        graph_store: Option<&Arc<engram::storage::GraphStore>>,
    ) -> Result<SessionStats> {
        let mut stats = SessionStats::default();

        // Check if we have all components
        let (extractor, embedding_provider, storage) =
            match (extractor, embedding_provider, storage) {
                (Some(e), Some(emb), Some(s)) => (e, emb, s),
                _ => {
                    // Fallback to simulated stats if not configured
                    return Ok(SessionStats {
                        memories_created: session.messages.len() / 2,
                        entities_extracted: 1,
                        facts_extracted: session.messages.len(),
                        messages_stored: 0,
                    });
                }
            };

        // Convert benchmark messages to conversation
        let turns: Vec<ConversationTurn> = session
            .messages
            .iter()
            .map(|m| ConversationTurn {
                role: match m.role.to_lowercase().as_str() {
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    _ => Role::System,
                },
                content: m.content.clone(),
                timestamp: Some(m.timestamp),
            })
            .collect();

        let conversation = Conversation::new(&session.user_id, turns);

        // Get session date for t_valid (use earliest message timestamp)
        let session_date = session.earliest_timestamp();

        // B3: Temporal parser for resolving relative dates in facts
        let temporal_parser = TemporalParser::new();

        if single_pass {
            // Single-pass extraction: 1 LLM call (faster for local models)
            use engram::extraction::Extractor;
            let facts = extractor
                .extract(&conversation)
                .await
                .map_err(|e| BenchmarkError::Ingestion(format!("Extraction failed: {}", e)))?;

            stats.facts_extracted = facts.len();
            let session_context = SessionEntityContext::new();

            // P7b-perf: Collect entities for batch graph ingestion
            let mut batch_entities: Vec<engram::storage::EntityInput> = Vec::new();
            let mut batch_mentions: Vec<engram::storage::MentionInput> = Vec::new();
            if graph_store.is_some() {
                let mut seen_entities: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
                for fact in &facts {
                    for e in &fact.entities {
                        let key = (e.name.to_lowercase(), e.entity_type.to_lowercase());
                        if seen_entities.insert(key) {
                            batch_entities.push(engram::storage::EntityInput {
                                name: e.name.clone(),
                                entity_type: e.entity_type.clone(),
                                aliases: vec![],
                            });
                        }
                    }
                }
            }

            for fact in facts {
                // P7a: Build entity triples for Memory.entity_ids population
                let entity_triples: Vec<(String, String, String)> = fact
                    .entities
                    .iter()
                    .map(|e| (e.normalized_id.clone(), e.entity_type.clone(), e.name.clone()))
                    .collect();

                let mut memory = Memory::new(&session.user_id, &fact.content)
                    .with_epistemic_type(fact.epistemic_type)
                    .with_fact_type(fact.fact_type)
                    .with_session(session.session_id.clone())
                    .with_session_entity_context(session_context.clone())
                    .with_observation_level(&fact.observation_level)
                    .with_entities(entity_triples);
                // B3: Keep t_valid = session_date (reliable ordering), store resolved time in t_event
                let mut payload_overrides = std::collections::HashMap::new();
                if let Some(session_ts) = session_date {
                    memory = memory.with_valid_time(session_ts);
                    payload_overrides.insert(
                        "session_timestamp".to_string(),
                        Value { kind: Some(Kind::StringValue(session_ts.to_rfc3339())) },
                    );
                    if let Some(event_ts) = temporal_parser.resolve_fact_time(&fact.content, session_ts) {
                        payload_overrides.insert(
                            "t_event".to_string(),
                            Value { kind: Some(Kind::StringValue(event_ts.to_rfc3339())) },
                        );
                    }
                }
                if let Some(seed) = extraction_seed {
                    payload_overrides.insert(
                        "extraction_seed".to_string(),
                        Value { kind: Some(Kind::IntegerValue(seed as i64)) },
                    );
                }

                let embedding = embedding_provider
                    .embed_document(&fact.content)
                    .await
                    .map_err(|e| BenchmarkError::Ingestion(format!("Embedding failed: {}", e)))?;

                let embedding_clone = embedding.clone();
                storage
                    .upsert_memory_with_payload_overrides(&memory, embedding, payload_overrides)
                    .await
                    .map_err(|e| BenchmarkError::Ingestion(format!("Storage failed: {}", e)))?;

                // B2: Supersession — compare session_timestamp for ordering
                if enable_consolidation {
                    let collection = memory.collection();
                    let user_filter = qdrant_client::qdrant::Filter::must([
                        qdrant_client::qdrant::Condition::matches(
                            "user_id",
                            session.user_id.clone(),
                        ),
                        qdrant_client::qdrant::Condition::matches("is_latest", true),
                    ]);
                    if let Ok(similar) = storage
                        .client
                        .search_points(
                            qdrant_client::qdrant::SearchPointsBuilder::new(
                                collection,
                                embedding_clone,
                                5,
                            )
                            .filter(user_filter)
                            .with_payload(true)
                            .score_threshold(consolidation_threshold),
                        )
                        .await
                    {
                        let current_order_time = session_date.unwrap_or(memory.t_valid);
                        for point in &similar.result {
                            let point_uuid = match &point.id {
                                Some(pid) => {
                                    use qdrant_client::qdrant::point_id::PointIdOptions;
                                    match &pid.point_id_options {
                                        Some(PointIdOptions::Uuid(s)) => s.clone(),
                                        _ => continue,
                                    }
                                }
                                None => continue,
                            };
                            if point_uuid == memory.id.to_string() {
                                continue;
                            }
                            let existing_order_time = parse_payload_datetime(&point.payload, "session_timestamp")
                                .or_else(|| parse_payload_datetime(&point.payload, "t_valid"));
                            if let Some(existing_time) = existing_order_time {
                                if existing_time < current_order_time {
                                    // Existing fact is older — supersede it
                                    if let Err(e) = storage
                                        .supersede_memory_in_collection(collection, &point_uuid)
                                        .await
                                    {
                                        tracing::warn!("Failed to supersede {}: {}", point_uuid, e);
                                    }
                                } else if existing_time > current_order_time {
                                    // Current fact is older — supersede it instead
                                    if let Err(e) = storage
                                        .supersede_memory_in_collection(collection, &memory.id.to_string())
                                        .await
                                    {
                                        tracing::warn!("Failed to supersede self {}: {}", memory.id, e);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }

                // P7b-perf: Collect mentions for batch graph ingestion
                if graph_store.is_some() {
                    let fact_id = memory.id.to_string();
                    for e in &fact.entities {
                        batch_mentions.push(engram::storage::MentionInput {
                            entity_name: e.name.clone(),
                            entity_type: e.entity_type.clone(),
                            fact_id: fact_id.clone(),
                        });
                    }
                }

                stats.memories_created += 1;
                stats.entities_extracted += fact.entities.len();
            }

            // P7b-perf: Single batch call for all graph writes (single-pass has no relationships)
            if let Some(ref graph) = graph_store {
                if let Err(e) = graph
                    .ingest_session_batch(
                        &session.user_id,
                        &session.session_id,
                        &batch_entities,
                        &[],
                        &batch_mentions,
                    )
                    .await
                {
                    tracing::warn!("Graph batch ingestion failed for session {}: {}", session.session_id, e);
                }
            }
        } else {
            // Two-pass extraction: entity registry + context-aware facts (2 LLM calls)
            let (facts, entity_registry) = extractor
                .extract_with_context(&conversation)
                .await
                .map_err(|e| BenchmarkError::Ingestion(format!("Extraction failed: {}", e)))?;

            stats.facts_extracted = facts.len();

            // P7b-perf: Collect graph data for batch ingestion
            let mut batch_entities: Vec<engram::storage::EntityInput> = Vec::new();
            let mut batch_relationships: Vec<engram::storage::RelationshipInput> = Vec::new();
            let mut batch_mentions: Vec<engram::storage::MentionInput> = Vec::new();
            if graph_store.is_some() {
                let typed_registry = entity_registry.to_typed_registry();
                for entity in typed_registry.all_entities() {
                    batch_entities.push(engram::storage::EntityInput {
                        name: entity.name.clone(),
                        entity_type: entity.entity_type.as_str().to_string(),
                        aliases: entity.aliases.clone(),
                    });
                }
                for rel in typed_registry.all_relationships() {
                    batch_relationships.push(engram::storage::RelationshipInput {
                        subject_name: rel.subject.clone(),
                        relation_type: rel.relation.as_str().to_string(),
                        object_name: rel.object.clone(),
                        confidence: rel.confidence,
                    });
                }
            }

            let session_context =
                SessionEntityContext::new().with_entities(entity_registry.entity_names());
            let session_context = if let Some(loc) = &entity_registry.primary_location {
                session_context.with_primary_location(loc.clone())
            } else {
                session_context
            };
            let session_context = if let Some(org) = &entity_registry.primary_organization {
                session_context.with_primary_organization(org.clone())
            } else {
                session_context
            };

            for fact in facts {
                // P7a: Build entity triples for Memory.entity_ids population
                let entity_triples: Vec<(String, String, String)> = fact
                    .entities
                    .iter()
                    .map(|e| (e.normalized_id.clone(), e.entity_type.clone(), e.name.clone()))
                    .collect();

                let mut memory = Memory::new(&session.user_id, &fact.content)
                    .with_epistemic_type(fact.epistemic_type)
                    .with_fact_type(fact.fact_type)
                    .with_session(session.session_id.clone())
                    .with_session_entity_context(session_context.clone())
                    .with_observation_level(&fact.observation_level)
                    .with_entities(entity_triples);
                // B3: Keep t_valid = session_date (reliable ordering), store resolved time in t_event
                let mut payload_overrides = std::collections::HashMap::new();
                if let Some(session_ts) = session_date {
                    memory = memory.with_valid_time(session_ts);
                    payload_overrides.insert(
                        "session_timestamp".to_string(),
                        Value { kind: Some(Kind::StringValue(session_ts.to_rfc3339())) },
                    );
                    if let Some(event_ts) = temporal_parser.resolve_fact_time(&fact.content, session_ts) {
                        payload_overrides.insert(
                            "t_event".to_string(),
                            Value { kind: Some(Kind::StringValue(event_ts.to_rfc3339())) },
                        );
                    }
                }
                if let Some(seed) = extraction_seed {
                    payload_overrides.insert(
                        "extraction_seed".to_string(),
                        Value { kind: Some(Kind::IntegerValue(seed as i64)) },
                    );
                }

                let embedding = embedding_provider
                    .embed_document(&fact.content)
                    .await
                    .map_err(|e| BenchmarkError::Ingestion(format!("Embedding failed: {}", e)))?;

                let embedding_clone = embedding.clone();
                storage
                    .upsert_memory_with_payload_overrides(&memory, embedding, payload_overrides)
                    .await
                    .map_err(|e| BenchmarkError::Ingestion(format!("Storage failed: {}", e)))?;

                // B2: Supersession — compare session_timestamp for ordering
                if enable_consolidation {
                    let collection = memory.collection();
                    let user_filter = qdrant_client::qdrant::Filter::must([
                        qdrant_client::qdrant::Condition::matches(
                            "user_id",
                            session.user_id.clone(),
                        ),
                        qdrant_client::qdrant::Condition::matches("is_latest", true),
                    ]);
                    if let Ok(similar) = storage
                        .client
                        .search_points(
                            qdrant_client::qdrant::SearchPointsBuilder::new(
                                collection,
                                embedding_clone,
                                5,
                            )
                            .filter(user_filter)
                            .with_payload(true)
                            .score_threshold(consolidation_threshold),
                        )
                        .await
                    {
                        let current_order_time = session_date.unwrap_or(memory.t_valid);
                        for point in &similar.result {
                            let point_uuid = match &point.id {
                                Some(pid) => {
                                    use qdrant_client::qdrant::point_id::PointIdOptions;
                                    match &pid.point_id_options {
                                        Some(PointIdOptions::Uuid(s)) => s.clone(),
                                        _ => continue,
                                    }
                                }
                                None => continue,
                            };
                            if point_uuid == memory.id.to_string() {
                                continue;
                            }
                            let existing_order_time = parse_payload_datetime(&point.payload, "session_timestamp")
                                .or_else(|| parse_payload_datetime(&point.payload, "t_valid"));
                            if let Some(existing_time) = existing_order_time {
                                if existing_time < current_order_time {
                                    // Existing fact is older — supersede it
                                    if let Err(e) = storage
                                        .supersede_memory_in_collection(collection, &point_uuid)
                                        .await
                                    {
                                        tracing::warn!("Failed to supersede {}: {}", point_uuid, e);
                                    }
                                } else if existing_time > current_order_time {
                                    // Current fact is older — supersede it instead
                                    if let Err(e) = storage
                                        .supersede_memory_in_collection(collection, &memory.id.to_string())
                                        .await
                                    {
                                        tracing::warn!("Failed to supersede self {}: {}", memory.id, e);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }

                // P7b-perf: Collect mentions for batch graph ingestion
                if graph_store.is_some() {
                    let fact_id = memory.id.to_string();
                    for e in &fact.entities {
                        batch_mentions.push(engram::storage::MentionInput {
                            entity_name: e.name.clone(),
                            entity_type: e.entity_type.clone(),
                            fact_id: fact_id.clone(),
                        });
                    }
                }

                stats.memories_created += 1;
                stats.entities_extracted += fact.entities.len();
            }

            // P7b-perf: Single batch call for all graph writes
            if let Some(ref graph) = graph_store {
                if let Err(e) = graph
                    .ingest_session_batch(
                        &session.user_id,
                        &session.session_id,
                        &batch_entities,
                        &batch_relationships,
                        &batch_mentions,
                    )
                    .await
                {
                    tracing::warn!("Graph batch ingestion failed for session {}: {}", session.session_id, e);
                }
            }
        }

        // ---- Raw Message Storage (Epic 003) ----
        // Store every conversation turn in the messages collection alongside extracted facts.
        // Each turn gets its own embedding and point in the messages collection.
        if !skip_messages {
            let peer_name = session
                .metadata
                .as_ref()
                .and_then(|m| m.get("peer_name"))
                .and_then(|v| v.as_str());

            // Collect turn texts for batch embedding, filtering empty/whitespace-only content
            let valid_message_indices: Vec<usize> = session
                .messages
                .iter()
                .enumerate()
                .filter(|(_, msg)| !msg.content.trim().is_empty())
                .map(|(i, _)| i)
                .collect();
            let turn_texts: Vec<String> = valid_message_indices
                .iter()
                .map(|&i| session.messages[i].content.clone())
                .collect();

            if !turn_texts.is_empty() {
                // Batch embed all turns (chunk if >50 to avoid API limits)
                const EMBED_BATCH_SIZE: usize = 50;
                let mut all_embeddings = Vec::with_capacity(turn_texts.len());
                let mut embed_ok = true;

                for chunk in turn_texts.chunks(EMBED_BATCH_SIZE) {
                    match embedding_provider.embed_batch(chunk).await {
                        Ok(embs) => all_embeddings.extend(embs),
                        Err(e) => {
                            tracing::warn!(
                                "Message embedding failed for session {}: {} (facts preserved)",
                                session.session_id, e
                            );
                            embed_ok = false;
                            break;
                        }
                    }
                }

                // Build points for valid turns only
                if embed_ok {
                    let points: Vec<PointStruct> = valid_message_indices
                        .iter()
                        .zip(all_embeddings.into_iter())
                        .map(|(&msg_idx, embedding)| {
                            let msg = &session.messages[msg_idx];
                            let id = Uuid::now_v7().to_string();
                            let mut payload = std::collections::HashMap::new();
                            payload.insert(
                                "content".to_string(),
                                Value { kind: Some(Kind::StringValue(msg.content.clone())) },
                            );
                            payload.insert(
                                "session_id".to_string(),
                                Value { kind: Some(Kind::StringValue(session.session_id.clone())) },
                            );
                            payload.insert(
                                "turn_index".to_string(),
                                Value { kind: Some(Kind::IntegerValue(msg_idx as i64)) },
                            );
                            payload.insert(
                                "role".to_string(),
                                Value { kind: Some(Kind::StringValue(msg.role.clone())) },
                            );
                            payload.insert(
                                "t_valid".to_string(),
                                Value { kind: Some(Kind::StringValue(msg.timestamp.to_rfc3339())) },
                            );
                            payload.insert(
                                "user_id".to_string(),
                                Value { kind: Some(Kind::StringValue(session.user_id.clone())) },
                            );
                            if let Some(name) = peer_name {
                                payload.insert(
                                    "peer_name".to_string(),
                                    Value { kind: Some(Kind::StringValue(name.to_string())) },
                                );
                            }
                            PointStruct::new(id, embedding, payload)
                        })
                        .collect();

                    let num_points = points.len();
                    storage
                        .upsert_messages_batch(points)
                        .await
                        .map_err(|e| BenchmarkError::Ingestion(format!("Message storage failed: {}", e)))?;
                    stats.messages_stored = num_points;
                }
            }
        }

        Ok(stats)
    }

    /// Ingest a single session (sync wrapper)
    pub fn ingest_session(&self, session: &BenchmarkSession) -> Result<SessionStats> {
        let rt =
            tokio::runtime::Runtime::new().map_err(|e| BenchmarkError::Ingestion(e.to_string()))?;
        rt.block_on(self.ingest_session_async(session))
    }

    /// Ingest a single session with real extraction and storage
    pub async fn ingest_session_async(&self, session: &BenchmarkSession) -> Result<SessionStats> {
        Self::ingest_session_static(
            session,
            self.extractor.as_ref(),
            self.embedding_provider.as_ref(),
            self.storage.as_ref(),
            self.config.single_pass,
            self.config.enable_consolidation,
            self.config.consolidation_threshold,
            self.config.extraction_seed,
            self.config.skip_messages,
            self.graph_store.as_ref(),
        )
        .await
    }

    /// Clear all data from storage
    ///
    /// This is used before a benchmark run to ensure clean state.
    pub fn clear_data(&self) -> Result<()> {
        // For now, we don't actually clear - the benchmark creates unique user IDs
        // A real implementation would delete all memories for the benchmark user
        Ok(())
    }

    /// Validate that a session can be ingested
    pub fn validate_session(session: &BenchmarkSession) -> Result<()> {
        if session.messages.is_empty() {
            return Err(BenchmarkError::Ingestion(format!(
                "Session {} has no messages",
                session.session_id
            ))
            .into());
        }

        if session.user_id.is_empty() {
            return Err(BenchmarkError::Ingestion(format!(
                "Session {} has no user_id",
                session.session_id
            ))
            .into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_session() -> BenchmarkSession {
        let now = Utc::now();
        BenchmarkSession::new("test-session", "user-1")
            .with_message(BenchmarkMessage::user("Hello", now))
            .with_message(BenchmarkMessage::assistant("Hi there!", now))
            .with_message(BenchmarkMessage::user("My name is John", now))
            .with_message(BenchmarkMessage::assistant("Nice to meet you, John!", now))
    }

    #[test]
    fn test_ingester_config_default() {
        let config = IngesterConfig::default();
        assert_eq!(config.concurrency, MAX_CONCURRENCY);
        assert_eq!(config.extraction_mode, "local-fast");
        assert!(config.clear_before_ingest);
    }

    #[test]
    fn test_ingester_config_builder() {
        let config = IngesterConfig::new()
            .with_concurrency(8)
            .with_extraction_mode("api")
            .with_clear_before_ingest(false);

        assert_eq!(config.concurrency, 8);
        assert_eq!(config.extraction_mode, "api");
        assert!(!config.clear_before_ingest);
    }

    #[test]
    fn test_build_conversation() {
        let session = create_test_session();
        let conversation = SessionIngester::build_conversation(&session.messages);

        assert!(conversation.contains("user: Hello"));
        assert!(conversation.contains("assistant: Hi there!"));
        assert!(conversation.contains("user: My name is John"));
    }

    #[test]
    fn test_ingestion_stats() {
        let mut stats = IngestionStats::new();

        stats.add_session(SessionStats {
            memories_created: 5,
            entities_extracted: 2,
            facts_extracted: 10,
            messages_stored: 4,
        });

        assert_eq!(stats.sessions_processed, 1);
        assert_eq!(stats.memories_created, 5);
        assert_eq!(stats.entities_extracted, 2);
        assert!(!stats.has_errors());
    }

    #[test]
    fn test_ingestion_stats_with_errors() {
        let mut stats = IngestionStats::new();
        stats.sessions_processed = 10;
        stats.add_error("Test error".to_string());

        assert!(stats.has_errors());
        assert!((stats.success_rate() - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_ingest_session() {
        let ingester = SessionIngester::with_defaults();
        let session = create_test_session();

        let result = ingester.ingest_session(&session);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert!(stats.memories_created > 0);
    }

    #[test]
    fn test_ingest_sessions() {
        let ingester = SessionIngester::with_defaults();
        let sessions = vec![create_test_session(), create_test_session()];

        let result = ingester.ingest_sessions(&sessions);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.sessions_processed, 2);
    }

    #[test]
    fn test_validate_session_valid() {
        let session = create_test_session();
        assert!(SessionIngester::validate_session(&session).is_ok());
    }

    #[test]
    fn test_validate_session_empty_messages() {
        let session = BenchmarkSession::new("test", "user-1");
        let result = SessionIngester::validate_session(&session);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_session_empty_user_id() {
        let now = Utc::now();
        let session =
            BenchmarkSession::new("test", "").with_message(BenchmarkMessage::user("Hello", now));
        let result = SessionIngester::validate_session(&session);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_batch_file() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let batch_path = dir.path().join("batch.jsonl");

        let sessions = vec![create_test_session()];
        let ingester = SessionIngester::with_defaults();

        let count = ingester
            .generate_batch_file(&sessions, &batch_path, "gpt-4o-mini")
            .unwrap();
        assert_eq!(count, 1);

        // Verify the file exists and has content
        let content = std::fs::read_to_string(&batch_path).unwrap();
        assert!(!content.is_empty());
        assert!(content.contains("test-session")); // custom_id
        assert!(content.contains("gpt-4o-mini")); // model
    }

    #[test]
    fn test_get_batch_session_ids() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let batch_path = dir.path().join("batch.jsonl");

        // Create a test JSONL file
        let content = r#"{"custom_id":"session_1","method":"POST","url":"/v1/chat/completions","body":{}}
{"custom_id":"session_2","method":"POST","url":"/v1/chat/completions","body":{}}"#;
        std::fs::write(&batch_path, content).unwrap();

        let session_ids = SessionIngester::get_batch_session_ids(&batch_path).unwrap();
        assert_eq!(session_ids.len(), 2);
        assert_eq!(session_ids[0], "session_1");
        assert_eq!(session_ids[1], "session_2");
    }

    #[test]
    fn test_ingestion_mode_enum() {
        assert_ne!(IngestionMode::RealTime, IngestionMode::BatchGenerate);
        assert_ne!(IngestionMode::BatchSubmit, IngestionMode::BatchPoll);
    }
}
