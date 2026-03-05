//! Extraction pipeline for converting conversations to memories

mod api_config;
mod api_extractor;
mod batch_client;
mod batch_extractor;
mod context;
mod entity_extractor;
mod entity_registry;
mod entity_types;
mod extractor;
mod relationships;
mod temporal_parser;
mod types;

pub use api_config::{ApiExtractorConfig, ApiProvider};
pub use api_extractor::ApiExtractor;
pub use batch_client::{BatchClient, BatchPollResult, BatchStatus};
pub use batch_extractor::{BatchExtractor, BatchRequest, BatchResultLine};
pub use extractor::Extractor;
pub use temporal_parser::TemporalParser;
pub use types::{Conversation, ConversationTurn, ExtractedFact, Role};

// Used by the retrieval module within this crate
pub(crate) use entity_extractor::EntityExtractor;
pub(crate) use types::ExtractedEntity;
