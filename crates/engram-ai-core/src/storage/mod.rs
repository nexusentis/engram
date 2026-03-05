//! Storage layer for Qdrant, SQLite, and SurrealDB

mod audit;
mod config;
mod database;
mod qdrant;
mod session_store;
mod sqlite_config;
#[cfg(feature = "graph")]
pub mod surrealdb_graph;

pub use audit::{AuditEntry, AuditLog, Operation};
pub use config::QdrantConfig;
pub use database::Database;
pub use qdrant::{QdrantStorage, COLLECTIONS};
pub use session_store::SessionStore;
pub use sqlite_config::SqliteConfig;
#[cfg(feature = "graph")]
pub use surrealdb_graph::{DisambiguationCandidate, EntityInput, GraphStore, MentionInput, RelationshipInput};
