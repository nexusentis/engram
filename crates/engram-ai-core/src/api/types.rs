//! API request and response types
//!
//! Data transfer objects for the REST API endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// Request to ingest a conversation
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct IngestRequest {
    /// User ID for the memories
    pub user_id: String,
    /// Conversation messages
    pub messages: Vec<MessageInput>,
    /// Optional metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Session ID for grouping conversations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// A message in a conversation
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct MessageInput {
    /// Role (user, assistant, system)
    pub role: String,
    /// Message content
    pub content: String,
    /// Optional timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

/// Response from ingestion
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    /// IDs of created memories
    pub memory_ids: Vec<Uuid>,
    /// Number of facts extracted
    pub facts_extracted: usize,
    /// Entities found in the conversation
    pub entities_found: Vec<String>,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

/// Search request
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct SearchRequest {
    /// Search query
    pub query: String,
    /// User ID to scope search
    pub user_id: String,
    /// Maximum results to return (default: 10)
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Optional filters
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<SearchFilters>,
    /// Include version history (default: false)
    #[serde(default)]
    pub include_history: bool,
}

fn default_limit() -> usize {
    10
}

/// Search filters
#[derive(Debug, Clone, Deserialize, Serialize, Default, ToSchema)]
pub struct SearchFilters {
    /// Filter by fact types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact_types: Option<Vec<String>>,
    /// Filter by entity IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_ids: Option<Vec<String>>,
    /// Minimum confidence score
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
    /// Time range filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<TimeRange>,
}

/// Time range for filtering
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct TimeRange {
    /// Start time (inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<DateTime<Utc>>,
    /// End time (exclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<DateTime<Utc>>,
}

impl TimeRange {
    /// Create a time range
    pub fn new(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> Self {
        Self { start, end }
    }

    /// Check if a timestamp is within this range
    pub fn contains(&self, timestamp: DateTime<Utc>) -> bool {
        let after_start = self.start.is_none_or(|s| timestamp >= s);
        let before_end = self.end.is_none_or(|e| timestamp < e);
        after_start && before_end
    }
}

/// Search response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResponse {
    /// Matched memories
    pub memories: Vec<MemoryResult>,
    /// Total number found
    pub total_found: usize,
    /// Search time in milliseconds
    pub search_time_ms: u64,
    /// Whether the search abstained
    pub abstained: bool,
    /// Reason for abstention
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_reason: Option<String>,
}

/// Memory result in search
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryResult {
    /// Memory ID
    pub id: Uuid,
    /// Content text
    pub content: String,
    /// Confidence score
    pub confidence: f32,
    /// Retrieval score
    pub score: f32,
    /// Fact type
    pub fact_type: String,
    /// Entity IDs
    pub entities: Vec<String>,
    /// When the fact became valid
    pub t_valid: DateTime<Utc>,
    /// When the memory was created
    pub t_created: DateTime<Utc>,
    /// ID of memory this supersedes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_id: Option<Uuid>,
}

/// Single memory response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryResponse {
    /// Memory details
    pub memory: MemoryDetail,
    /// Version history (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<MemoryVersion>>,
}

/// Detailed memory information
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryDetail {
    /// Memory ID
    pub id: Uuid,
    /// User ID
    pub user_id: String,
    /// Content text
    pub content: String,
    /// Confidence score
    pub confidence: f32,
    /// Source type (UserExplicit, UserImplied, etc.)
    pub source_type: String,
    /// Fact type (State, Event, Preference, Relation)
    pub fact_type: String,
    /// Epistemic type (World, Experience, Opinion, Observation)
    pub epistemic_type: String,
    /// Entity IDs
    pub entities: Vec<String>,
    /// When the fact became valid
    pub t_valid: DateTime<Utc>,
    /// When the memory was created
    pub t_created: DateTime<Utc>,
    /// When the memory expired (if superseded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t_expired: Option<DateTime<Utc>>,
    /// ID of memory this supersedes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_id: Option<Uuid>,
    /// IDs of memories this was derived from
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from_ids: Vec<Uuid>,
    /// Whether this is the latest version
    pub is_latest: bool,
}

/// Memory version in history
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryVersion {
    /// Memory ID
    pub id: Uuid,
    /// Content text
    pub content: String,
    /// When this version became valid
    pub t_valid: DateTime<Utc>,
    /// Version number (1-indexed)
    pub version: u32,
}

/// Delete response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeleteResponse {
    /// Memory ID
    pub id: Uuid,
    /// Whether deletion succeeded
    pub deleted: bool,
    /// When the memory was deleted
    pub deleted_at: DateTime<Utc>,
}

/// History request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HistoryRequest {
    /// Memory ID to get history for
    pub memory_id: Uuid,
    /// Maximum versions to return (default: 50)
    #[serde(default = "default_history_limit")]
    pub limit: usize,
}

fn default_history_limit() -> usize {
    50
}

/// User memories request
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, IntoParams)]
pub struct UserMemoriesRequest {
    /// User ID to list memories for
    #[serde(default)]
    pub user_id: String,
    /// Maximum memories to return (default: 10)
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: usize,
    /// Include expired memories (default: false)
    #[serde(default)]
    pub include_expired: bool,
}

impl Default for UserMemoriesRequest {
    fn default() -> Self {
        Self {
            user_id: String::new(),
            limit: 10,
            offset: 0,
            include_expired: false,
        }
    }
}

/// User memories response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserMemoriesResponse {
    /// Memories for the user
    pub memories: Vec<MemoryResult>,
    /// Total memories available
    pub total: usize,
    /// Limit used in request
    pub limit: usize,
    /// Offset used in request
    pub offset: usize,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Overall status
    pub status: String,
    /// Qdrant connection status
    pub qdrant: bool,
    /// API version
    pub version: String,
}

impl HealthResponse {
    /// Create a healthy response
    pub fn healthy() -> Self {
        Self {
            status: "healthy".to_string(),
            qdrant: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Create a degraded response
    pub fn degraded(qdrant: bool) -> Self {
        Self {
            status: "degraded".to_string(),
            qdrant,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// API error response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,
    /// Error code
    pub code: String,
    /// Additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
            details: None,
        }
    }

    /// Add details to the error
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Create a not found error
    pub fn not_found(id: impl std::fmt::Display) -> Self {
        Self::new(format!("Resource not found: {}", id), "NOT_FOUND")
    }

    /// Create a validation error
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(message, "VALIDATION_ERROR")
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(message, "INTERNAL_ERROR")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingest_request_serialization() {
        let req = IngestRequest {
            user_id: "user-1".to_string(),
            messages: vec![MessageInput {
                role: "user".to_string(),
                content: "Hello world".to_string(),
                timestamp: None,
            }],
            metadata: None,
            session_id: Some("session-1".to_string()),
        };

        let json = serde_json::to_string(&req).unwrap();
        let parsed: IngestRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.user_id, "user-1");
        assert_eq!(parsed.messages.len(), 1);
    }

    #[test]
    fn test_search_request_defaults() {
        let json = r#"{"query": "test", "user_id": "user-1"}"#;
        let req: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.limit, 10); // Default
        assert!(!req.include_history); // Default
    }

    #[test]
    fn test_search_filters() {
        let filters = SearchFilters {
            fact_types: Some(vec!["State".to_string()]),
            entity_ids: None,
            min_confidence: Some(0.8),
            time_range: None,
        };

        let json = serde_json::to_string(&filters).unwrap();
        let parsed: SearchFilters = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.min_confidence, Some(0.8));
    }

    #[test]
    fn test_time_range_contains() {
        let now = Utc::now();
        let past = now - chrono::Duration::hours(1);
        let future = now + chrono::Duration::hours(1);

        // No bounds
        let range = TimeRange::new(None, None);
        assert!(range.contains(now));

        // Only start bound
        let range = TimeRange::new(Some(past), None);
        assert!(range.contains(now));
        assert!(!range.contains(past - chrono::Duration::hours(1)));

        // Only end bound
        let range = TimeRange::new(None, Some(future));
        assert!(range.contains(now));
        assert!(!range.contains(future + chrono::Duration::hours(1)));

        // Both bounds
        let range = TimeRange::new(Some(past), Some(future));
        assert!(range.contains(now));
        assert!(!range.contains(past - chrono::Duration::hours(1)));
    }

    #[test]
    fn test_error_response() {
        let err = ErrorResponse::not_found("memory-123");
        assert_eq!(err.code, "NOT_FOUND");
        assert!(err.error.contains("memory-123"));

        let err = ErrorResponse::validation("Invalid input")
            .with_details(serde_json::json!({"field": "user_id"}));
        assert_eq!(err.code, "VALIDATION_ERROR");
        assert!(err.details.is_some());
    }

    #[test]
    fn test_health_response() {
        let healthy = HealthResponse::healthy();
        assert_eq!(healthy.status, "healthy");
        assert!(healthy.qdrant);

        let degraded = HealthResponse::degraded(false);
        assert_eq!(degraded.status, "degraded");
        assert!(!degraded.qdrant);
    }

    #[test]
    fn test_user_memories_request_defaults() {
        let json = r#"{}"#;
        let req: UserMemoriesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.limit, 10);
        assert_eq!(req.offset, 0);
        assert!(!req.include_expired);
    }

    #[test]
    fn test_memory_result_serialization() {
        let result = MemoryResult {
            id: Uuid::now_v7(),
            content: "Test content".to_string(),
            confidence: 0.9,
            score: 0.85,
            fact_type: "State".to_string(),
            entities: vec!["alice".to_string()],
            t_valid: Utc::now(),
            t_created: Utc::now(),
            supersedes_id: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Test content"));
    }
}
