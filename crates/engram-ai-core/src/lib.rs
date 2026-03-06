//! Engram Core: Rust-native AI agent memory system
//!
//! Core library providing types, storage, extraction, embedding, and retrieval
//! for a high-performance memory system for AI agents.

pub mod agent;
pub mod api;
pub mod config;
pub mod embedding;
pub mod error;
pub mod extraction;
pub mod llm;
pub mod memory_system;
pub mod retrieval;
pub mod storage;
pub mod types;

// Config
pub use config::{AgentConfig, Config, EnsembleConfig, GateConfig};

// Error types
pub use error::{Error, Result};

// Domain types
pub use types::{Entity, EpistemicType, FactType, Memory, Session, SourceType};

// Embedding
pub use embedding::{EmbeddingProvider, RemoteEmbeddingProvider};

// Extraction
pub use extraction::{
    ApiExtractor, ApiExtractorConfig, ApiProvider, BatchClient, BatchExtractor, Conversation,
    ConversationTurn, ExtractedFact, Extractor, Role, TemporalParser,
};

// LLM
pub use llm::{HttpLlmClient, LlmClient, LlmClientConfig, ModelProfile, ModelRegistry};

// Retrieval
pub use retrieval::{AbstentionConfig, ConfidenceScorer, QueryAnalyzer, RerankedResult};

// Storage
pub use storage::{QdrantConfig, QdrantStorage};

// Memory system facade
pub use memory_system::{IngestResult, MemorySystem, MemorySystemBuilder};
