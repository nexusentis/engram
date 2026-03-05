//! API module
//!
//! Provides REST and MCP APIs for memory operations.

pub mod auth;
pub mod mcp;
#[cfg(feature = "metrics")]
pub mod metrics;
mod types;

pub use auth::{
    authenticate, extract_bearer_token, hash_token, should_skip_auth, verify_token, AuthConfig,
    AuthError, AuthState,
};
#[cfg(feature = "metrics")]
pub use metrics::{Metrics, METRICS};
pub use types::{
    DeleteResponse, ErrorResponse, HealthResponse, HistoryRequest, IngestRequest, IngestResponse,
    MemoryDetail, MemoryResponse, MemoryResult, MemoryVersion, MessageInput, SearchFilters,
    SearchRequest, SearchResponse, TimeRange, UserMemoriesRequest, UserMemoriesResponse,
};
