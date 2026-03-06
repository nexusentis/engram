//! Shared types and payload extraction helpers for tool execution.

use qdrant_client::qdrant::value::Kind;

/// Structured result from tool execution.
///
/// Contains both the human-readable text output and structured metadata
/// (sessions found, content snippets, result count) for downstream processing.
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    /// Human-readable text output
    pub text: String,
    /// Session IDs found in the results
    pub sessions: std::collections::HashSet<String>,
    /// Content snippets from results (fact content or message content)
    pub content_snippets: Vec<String>,
    /// Number of results returned
    pub result_count: usize,
    /// Qdrant point UUIDs for facts returned
    pub fact_ids: Vec<String>,
}

// Re-export agent response types from LLM module
pub use crate::llm::{AgentResponse, CompletionResult, ToolCall};

/// Extract a string value from a Qdrant payload map.
pub fn get_string_payload(
    payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> String {
    payload
        .get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::StringValue(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Extract an integer value from a Qdrant payload map.
pub fn get_int_payload(
    payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> i64 {
    payload
        .get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::IntegerValue(i)) => Some(*i),
            _ => None,
        })
        .unwrap_or(0)
}
