//! Storage layer for Qdrant and SurrealDB

mod config;
mod qdrant;
#[cfg(feature = "graph")]
pub mod surrealdb_graph;

pub use config::QdrantConfig;
pub use qdrant::{QdrantStorage, COLLECTIONS};
#[cfg(feature = "graph")]
pub use surrealdb_graph::{DisambiguationCandidate, EntityInput, GraphStore, MentionInput, RelationshipInput};
