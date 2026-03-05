//! Shared types and payload extraction helpers for tool execution.

use qdrant_client::qdrant::value::Kind;

// ---- Structured Tool Execution Result ----

/// Structured result from tool execution (for recall harness)
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    /// Human-readable text output (same as execute() returns)
    pub text: String,
    /// Session IDs found in the results
    pub sessions: std::collections::HashSet<String>,
    /// Content snippets from results (fact content or message content)
    pub content_snippets: Vec<String>,
    /// Number of results returned
    pub result_count: usize,
    /// Qdrant point UUIDs for facts returned (P20: graph augmentation seeds)
    pub fact_ids: Vec<String>,
}

// ---- Agent Response Types (re-exported from engram-core) ----

pub use engram::llm::{AgentResponse, CompletionResult, ToolCall};

// ---- Payload extraction helpers ----

pub(super) fn get_string_payload(
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

pub(super) fn get_int_payload(
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
