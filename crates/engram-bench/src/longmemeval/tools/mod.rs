//! Tool definitions for the agentic answering loop (Epic 003, Feature 002)
//!
//! Defines 6 tools as OpenAI function-calling JSON schemas:
//! - search_facts: Semantic search over extracted facts
//! - search_messages: Semantic search over raw conversation messages
//! - grep_messages: Exact text search over raw messages
//! - get_session_context: Get surrounding turns from a specific session
//! - get_by_date_range: Get facts/messages within a date range
//! - done: Signal completion with final answer

mod context;
pub(crate) mod date_parsing;
mod graph;
pub(crate) mod schemas;
mod search;
#[cfg(test)]
mod tests;
pub(crate) mod types;

use std::sync::Arc;

use qdrant_client::qdrant::{Condition, Filter};
use serde_json::Value;

use engram::embedding::RemoteEmbeddingProvider;
use crate::error::{BenchmarkError, Result};
use engram::storage::QdrantStorage;

pub use date_parsing::format_date_header;
pub use schemas::*;
pub use types::*;

// ---- ToolExecutor ----

/// Executes agentic tools against Qdrant storage
pub struct ToolExecutor {
    storage: Arc<QdrantStorage>,
    embedding_provider: Arc<RemoteEmbeddingProvider>,
    /// Reference date for relative date calculations
    pub reference_date: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether to show relative dates in headers
    pub relative_dates: bool,
    /// Optional SurrealDB knowledge graph for graph-based retrieval
    graph_store: Option<Arc<engram::storage::GraphStore>>,
    /// User ID for scoping message retrieval
    user_id: Option<String>,
}

impl ToolExecutor {
    pub fn new(
        storage: Arc<QdrantStorage>,
        embedding_provider: Arc<RemoteEmbeddingProvider>,
    ) -> Self {
        Self {
            storage,
            embedding_provider,
            reference_date: None,
            relative_dates: true,
            graph_store: None,
            user_id: None,
        }
    }

    pub fn with_user_id(mut self, user_id: &str) -> Self {
        self.user_id = Some(user_id.to_string());
        self
    }

    pub fn with_reference_date(mut self, date: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        self.reference_date = date;
        self
    }

    pub fn with_relative_dates(mut self, enabled: bool) -> Self {
        self.relative_dates = enabled;
        self
    }

    pub fn with_graph_store(mut self, store: Arc<engram::storage::GraphStore>) -> Self {
        self.graph_store = Some(store);
        self
    }

    fn date_header(&self, date_str: &str) -> String {
        format_date_header(date_str, self.reference_date, self.relative_dates)
    }

    /// Build a user_id filter condition for message scoping
    fn user_id_filter(&self) -> Option<Filter> {
        self.user_id
            .as_ref()
            .map(|uid| Filter::must([Condition::matches("user_id", uid.clone())]))
    }

    /// Execute a tool by name with the given arguments
    pub async fn execute(&self, tool_name: &str, args: &Value) -> Result<String> {
        match tool_name {
            "search_facts" => self.exec_search_facts(args).await,
            "search_messages" => self.exec_search_messages(args).await,
            "grep_messages" => self.exec_grep_messages(args).await,
            "get_session_context" => self.exec_get_session_context(args).await,
            "get_by_date_range" => self.exec_get_by_date_range(args).await,
            "search_entity" => self.exec_search_entity(args).await,
            "graph_lookup" => self.exec_graph_lookup(args).await,
            "graph_relationships" => self.exec_graph_relationships(args).await,
            "graph_disambiguate" => self.exec_graph_disambiguate(args).await,
            "graph_enumerate" => self.exec_graph_enumerate(args).await,
            "date_diff" => self.exec_date_diff(args),
            "done" => Ok(args["answer"].as_str().unwrap_or("").to_string()),
            _ => Err(BenchmarkError::Answering(format!("Unknown tool: {}", tool_name)).into()),
        }
    }

    /// Execute tool and return structured result (for recall harness).
    /// Returns session IDs, content snippets, and result count alongside text output.
    pub async fn execute_structured(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
        match tool_name {
            "search_facts" => self.exec_search_facts_structured(args).await,
            "search_messages" => self.exec_search_messages_structured(args).await,
            "grep_messages" => self.exec_grep_messages_structured(args).await,
            "get_session_context" => self.exec_get_session_context_structured(args).await,
            "get_by_date_range" => self.exec_get_by_date_range_structured(args).await,
            _ => {
                // For tools that don't return session data, wrap the text result
                let text = self.execute(tool_name, args).await?;
                Ok(ToolExecutionResult {
                    text,
                    sessions: std::collections::HashSet::new(),
                    content_snippets: Vec::new(),
                    result_count: 0,
                    fact_ids: Vec::new(),
                })
            }
        }
    }
}
