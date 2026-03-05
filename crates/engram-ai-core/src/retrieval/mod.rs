//! Retrieval engine for hybrid search
//!
//! This module implements four-way parallel retrieval:
//! - Semantic: Dense vector search via embeddings
//! - Keyword: BM25-style sparse search
//! - Temporal: Time-bounded filtering
//! - Entity: Entity-based lookup
//!
//! Results are fused using Reciprocal Rank Fusion (RRF).
//!
//! For multi-hop queries, the decomposer breaks complex queries
//! into sequential sub-queries. The confidence scorer determines
//! when to abstain from answering.

mod channel;
mod confidence;
mod decomposer;
mod engine;
mod entity;
mod executor;
mod fusion;
mod keyword;
mod query_analyzer;
mod semantic;
mod temporal;
mod temporal_context;
mod temporal_filter;
mod temporal_scoring;

// Public API — used by engram-bench and other consumers
pub use confidence::{AbstentionConfig, AbstentionReason, AbstentionResult, ConfidenceScorer};
pub use engine::RerankedResult;
pub use query_analyzer::{QueryAnalysis, QueryAnalyzer, TemporalIntent};
pub use temporal_filter::TemporalFilterBuilder;
