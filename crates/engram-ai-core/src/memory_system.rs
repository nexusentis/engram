//! High-level facade for the Engram memory system.
//!
//! `MemorySystem` provides a simple, production-ready interface for ingesting
//! conversations, searching memories, and managing stored facts. It wires
//! together Qdrant storage, OpenAI embeddings, and LLM-based extraction.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use engram_core::{MemorySystem, Conversation, ConversationTurn};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let system = MemorySystem::builder()
//!     .openai_api_key("sk-...")
//!     .build()
//!     .await?;
//!
//! let conversation = Conversation::new("user_1", vec![
//!     ConversationTurn::user("I work at Anthropic on the safety team"),
//!     ConversationTurn::assistant("That's great!"),
//! ]);
//!
//! let ids = system.ingest(conversation).await?;
//! let results = system.search("user_1", "where does the user work?", 5).await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use uuid::Uuid;

use crate::embedding::EmbeddingProvider;
use crate::error::{Error, ExtractionError, Result, StorageError};
use crate::extraction::{ApiExtractor, ApiExtractorConfig, Conversation, ConversationTurn, Extractor};
use crate::storage::{QdrantConfig, QdrantStorage};
use crate::types::Memory;
use crate::RemoteEmbeddingProvider;

/// High-level facade for the Engram memory system.
///
/// Wraps Qdrant storage, an embedding provider, and an LLM extractor into a
/// single struct with a clean public API for ingest, search, and CRUD.
pub struct MemorySystem {
    qdrant: Arc<QdrantStorage>,
    embedder: Arc<dyn EmbeddingProvider>,
    extractor: Arc<ApiExtractor>,
}

impl MemorySystem {
    /// Create a `MemorySystem` from pre-built components.
    ///
    /// Prefer [`MemorySystemBuilder`] for most use cases.
    pub fn new(
        qdrant: Arc<QdrantStorage>,
        embedder: Arc<dyn EmbeddingProvider>,
        extractor: Arc<ApiExtractor>,
    ) -> Self {
        Self {
            qdrant,
            embedder,
            extractor,
        }
    }

    /// Start building a `MemorySystem` with sensible defaults.
    pub fn builder() -> MemorySystemBuilder {
        MemorySystemBuilder::default()
    }

    // ── Ingest ──────────────────────────────────────────────────────────

    /// Extract facts from a conversation and store them.
    ///
    /// Returns the UUIDs of the stored memories.
    pub async fn ingest(&self, conversation: Conversation) -> Result<Vec<Uuid>> {
        let user_id = conversation.user_id.clone();
        let session_id = conversation.session_id.clone();

        let facts = self.extractor.extract(&conversation).await?;
        let mut ids = Vec::with_capacity(facts.len());

        for fact in &facts {
            let mut memory = Memory::new(&user_id, &fact.content);
            memory.confidence = fact.confidence;
            memory.fact_type = fact.fact_type;
            memory.epistemic_type = fact.epistemic_type;
            memory.source_type = fact.source_type;
            memory.entity_ids = fact.entities.iter().map(|e| e.normalized_id.clone()).collect();
            memory.entity_names = fact.entities.iter().map(|e| e.name.clone()).collect();
            memory.entity_types = fact.entities.iter().map(|e| e.entity_type.clone()).collect();
            memory.observation_level = fact.observation_level.clone();
            if let Some(t_valid) = fact.t_valid {
                memory.t_valid = t_valid;
            }
            if let Some(ref sid) = session_id {
                memory.session_id = Some(sid.clone());
            }

            let vector = self.embedder.embed_document(&fact.content).await?;
            self.qdrant.upsert_memory(&memory, vector).await?;
            ids.push(memory.id);
        }

        Ok(ids)
    }

    /// Ingest pre-built conversation turns for a user.
    ///
    /// Convenience wrapper that creates a [`Conversation`] from turns.
    pub async fn ingest_turns(
        &self,
        user_id: &str,
        turns: &[ConversationTurn],
    ) -> Result<Vec<Uuid>> {
        let conversation = Conversation::new(user_id, turns.to_vec());
        self.ingest(conversation).await
    }

    // ── Search ──────────────────────────────────────────────────────────

    /// Semantic search across all memory collections for a user.
    ///
    /// Returns `(Memory, score)` pairs sorted by relevance (highest first).
    pub async fn search(
        &self,
        user_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Memory, f32)>> {
        let query_vector = self.embedder.embed_query(query).await?;
        self.qdrant
            .search_memories(user_id, query_vector, limit as u64, None)
            .await
    }

    // ── Store ───────────────────────────────────────────────────────────

    /// Store a simple text fact for a user.
    ///
    /// Creates a `Memory` with default metadata. For richer control, use
    /// [`store_memory`](Self::store_memory).
    pub async fn store_fact(&self, user_id: &str, content: &str) -> Result<Uuid> {
        let memory = Memory::new(user_id, content);
        self.store_memory(user_id, &memory).await
    }

    /// Store a pre-built `Memory` with full metadata control.
    ///
    /// The `user_id` parameter is used for validation — it must match
    /// `memory.user_id`.
    pub async fn store_memory(&self, user_id: &str, memory: &Memory) -> Result<Uuid> {
        if memory.user_id != user_id {
            return Err(Error::Storage(StorageError::Qdrant(format!(
                "user_id mismatch: expected '{}', got '{}'",
                user_id, memory.user_id
            ))));
        }
        let vector = self.embedder.embed_document(&memory.content).await?;
        self.qdrant.upsert_memory(memory, vector).await?;
        Ok(memory.id)
    }

    // ── Get / Delete ────────────────────────────────────────────────────

    /// Retrieve a memory by ID.
    pub async fn get_memory(&self, user_id: &str, memory_id: Uuid) -> Result<Option<Memory>> {
        self.qdrant.get_memory(user_id, memory_id).await
    }

    /// Soft-delete a memory (marks as expired, sets `is_latest = false`).
    ///
    /// Returns `true` if the memory was found and deleted.
    pub async fn delete_memory(&self, user_id: &str, memory_id: Uuid) -> Result<bool> {
        self.qdrant.delete_memory(user_id, memory_id).await
    }

    // ── Escape hatches ──────────────────────────────────────────────────

    /// Access the underlying Qdrant storage for advanced operations.
    pub fn storage(&self) -> &Arc<QdrantStorage> {
        &self.qdrant
    }

    /// Access the underlying embedding provider.
    pub fn embedder(&self) -> &Arc<dyn EmbeddingProvider> {
        &self.embedder
    }

    /// Access the underlying fact extractor.
    pub fn extractor(&self) -> &Arc<ApiExtractor> {
        &self.extractor
    }
}

// ── Builder ─────────────────────────────────────────────────────────────

/// Builder for constructing a [`MemorySystem`] with sensible defaults.
///
/// Defaults:
/// - Qdrant URL: `http://localhost:6334`
/// - Vector size: 1536 (matches `text-embedding-3-small`)
/// - Embedding model: `text-embedding-3-small`
/// - Extraction model: `gpt-4o-mini`
pub struct MemorySystemBuilder {
    qdrant_url: String,
    qdrant_vector_size: u64,
    openai_api_key: Option<String>,
    embedding_model: String,
    extraction_model: String,
}

impl Default for MemorySystemBuilder {
    fn default() -> Self {
        Self {
            qdrant_url: "http://localhost:6334".to_string(),
            qdrant_vector_size: 1536,
            openai_api_key: None,
            embedding_model: "text-embedding-3-small".to_string(),
            extraction_model: "gpt-4o-mini".to_string(),
        }
    }
}

impl MemorySystemBuilder {
    /// Set the Qdrant server URL (default: `http://localhost:6334`).
    pub fn qdrant_url(mut self, url: impl Into<String>) -> Self {
        self.qdrant_url = url.into();
        self
    }

    /// Set the vector dimension (default: 1536, matching `text-embedding-3-small`).
    pub fn vector_size(mut self, size: u64) -> Self {
        self.qdrant_vector_size = size;
        self
    }

    /// Set the OpenAI API key. If not called, falls back to `OPENAI_API_KEY` env var.
    pub fn openai_api_key(mut self, key: impl Into<String>) -> Self {
        self.openai_api_key = Some(key.into());
        self
    }

    /// Set the embedding model (default: `text-embedding-3-small`).
    pub fn embedding_model(mut self, model: impl Into<String>) -> Self {
        self.embedding_model = model.into();
        self
    }

    /// Set the extraction model (default: `gpt-4o-mini`).
    pub fn extraction_model(mut self, model: impl Into<String>) -> Self {
        self.extraction_model = model.into();
        self
    }

    /// Build the `MemorySystem`, connecting to Qdrant and initializing collections.
    pub async fn build(self) -> std::result::Result<MemorySystem, Error> {
        // Resolve API key
        let api_key = self
            .openai_api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| {
                ExtractionError::Api(
                    "No OpenAI API key: set OPENAI_API_KEY or call .openai_api_key()".into(),
                )
            })?;

        // Qdrant — explicitly set vector_size to avoid dimension mismatch with default (768)
        let qdrant_config = QdrantConfig::external(&self.qdrant_url)
            .with_vector_size(self.qdrant_vector_size);
        let qdrant: Arc<QdrantStorage> = Arc::new(QdrantStorage::new(qdrant_config).await?);

        // Retry initialization to handle Docker Compose race conditions
        let mut last_err = None;
        for attempt in 1..=3u32 {
            match qdrant.initialize().await {
                Ok(()) => {
                    last_err = None;
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < 3 {
                        tracing::warn!(attempt, "Qdrant not ready, retrying in 2s: {}", last_err.as_ref().unwrap());
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    } else {
                        tracing::error!(attempt, "Qdrant not ready after 3 attempts: {}", last_err.as_ref().unwrap());
                    }
                }
            }
        }
        if let Some(e) = last_err {
            return Err(e.into());
        }

        // Embedder
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(
            RemoteEmbeddingProvider::new(&api_key, Some(self.embedding_model))
                .map_err(Error::Llm)?,
        );

        // Extractor
        let extractor_config =
            ApiExtractorConfig::openai(&self.extraction_model).with_api_key(&api_key);
        let extractor = Arc::new(ApiExtractor::new(extractor_config));

        Ok(MemorySystem::new(qdrant, embedder, extractor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = MemorySystemBuilder::default();
        assert_eq!(builder.qdrant_url, "http://localhost:6334");
        assert_eq!(builder.qdrant_vector_size, 1536);
        assert_eq!(builder.embedding_model, "text-embedding-3-small");
        assert_eq!(builder.extraction_model, "gpt-4o-mini");
        assert!(builder.openai_api_key.is_none());
    }

    #[test]
    fn test_builder_chaining() {
        let builder = MemorySystem::builder()
            .qdrant_url("http://custom:6334")
            .vector_size(3072)
            .openai_api_key("sk-test")
            .embedding_model("text-embedding-3-large")
            .extraction_model("gpt-4o");

        assert_eq!(builder.qdrant_url, "http://custom:6334");
        assert_eq!(builder.qdrant_vector_size, 3072);
        assert_eq!(builder.openai_api_key, Some("sk-test".to_string()));
        assert_eq!(builder.embedding_model, "text-embedding-3-large");
        assert_eq!(builder.extraction_model, "gpt-4o");
    }
}
