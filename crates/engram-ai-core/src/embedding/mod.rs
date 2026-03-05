//! Embedding generation for semantic search
//!
//! Provides the `EmbeddingProvider` trait and a remote provider backed by
//! OpenAI text-embedding-3-small.

mod provider;
mod remote;

pub use provider::{normalize_vector, EmbeddingProvider, EMBEDDING_DIMENSION};
pub use remote::RemoteEmbeddingProvider;
