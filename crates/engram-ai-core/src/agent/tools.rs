//! ToolExecutor: executes memory agent tools against Qdrant storage.
//!
//! Provides 8 production tools: search_facts, search_messages, grep_messages,
//! get_session_context, get_by_date_range, search_entity, date_diff, done.

use std::sync::Arc;

use chrono::Datelike;
use qdrant_client::qdrant::{Condition, DatetimeRange, Filter};
use serde_json::Value;

use crate::embedding::EmbeddingProvider;
use crate::error::{Error, Result};
use crate::storage::{QdrantStorage, COLLECTIONS};

use super::date_parsing::{format_date_header, parse_date_expression};
use super::tool_types::{get_int_payload, get_string_payload, ToolExecutionResult};

/// Executes memory agent tools against Qdrant storage.
pub struct ToolExecutor {
    storage: Arc<QdrantStorage>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
    /// Reference date for relative date calculations
    pub reference_date: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether to show relative dates in headers
    pub relative_dates: bool,
    /// User ID for scoping retrieval
    user_id: Option<String>,
}

impl ToolExecutor {
    pub fn new(
        storage: Arc<QdrantStorage>,
        embedding_provider: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self {
            storage,
            embedding_provider,
            reference_date: None,
            relative_dates: true,
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

    /// Access the underlying storage.
    pub fn storage(&self) -> &Arc<QdrantStorage> {
        &self.storage
    }

    fn date_header(&self, date_str: &str) -> String {
        format_date_header(date_str, self.reference_date, self.relative_dates)
    }

    /// Build a user_id filter condition for message scoping.
    fn user_id_filter(&self) -> Option<Filter> {
        self.user_id
            .as_ref()
            .map(|uid| Filter::must([Condition::matches("user_id", uid.clone())]))
    }

    /// Reference year for date parsing (from reference_date or current year).
    fn reference_year(&self) -> Option<i32> {
        self.reference_date.map(|d| d.date_naive().year())
    }

    /// Execute a tool by name with the given arguments.
    pub async fn execute(&self, tool_name: &str, args: &Value) -> Result<String> {
        match tool_name {
            "search_facts" => self.exec_search_facts(args).await,
            "search_messages" => self.exec_search_messages(args).await,
            "grep_messages" => self.exec_grep_messages(args).await,
            "get_session_context" => self.exec_get_session_context(args).await,
            "get_by_date_range" => self.exec_get_by_date_range(args).await,
            "search_entity" => self.exec_search_entity(args).await,
            "date_diff" => self.exec_date_diff(args),
            "done" => Ok(args["answer"].as_str().unwrap_or("").to_string()),
            _ => Err(Error::Agent(format!("Unknown tool: {}", tool_name))),
        }
    }

    /// Execute tool and return structured result with session IDs and metadata.
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

    // ── search_facts ────────────────────────────────────────────────────

    async fn exec_search_facts(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as u64;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| Error::Agent(format!("Embedding failed: {}", e)))?;

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("is_latest", true).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        if let Some(level) = args["level"].as_str() {
            must_conditions
                .push(Condition::matches("observation_level", level.to_string()).into());
        }
        let filter = Filter {
            must: must_conditions,
            ..Default::default()
        };

        let results = self
            .storage
            .search_memories_hybrid(filter, embedding, query, top_k, None)
            .await
            .map_err(|e| Error::Agent(format!("Search failed: {}", e)))?;

        let mut by_date: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for (memory, _score) in &results {
            let date_str = memory.t_valid.format("%Y/%m/%d").to_string();
            let sid = memory.session_id.as_deref().unwrap_or("unknown");
            by_date
                .entry(date_str)
                .or_default()
                .push(format!("(session: {}) {}", sid, memory.content));
        }

        let mut output = format!("Found {} facts:\n", results.len());
        for (date, entries) in &by_date {
            output.push_str(&format!("{}\n", self.date_header(date)));
            for (i, entry) in entries.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, entry));
            }
        }
        Ok(output)
    }

    async fn exec_search_facts_structured(&self, args: &Value) -> Result<ToolExecutionResult> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as u64;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| Error::Agent(format!("Embedding failed: {}", e)))?;

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("is_latest", true).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        if let Some(level) = args["level"].as_str() {
            must_conditions
                .push(Condition::matches("observation_level", level.to_string()).into());
        }
        let filter = Filter {
            must: must_conditions,
            ..Default::default()
        };

        let results = self
            .storage
            .search_memories_hybrid(filter, embedding, query, top_k, None)
            .await
            .map_err(|e| Error::Agent(format!("Search failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let mut fact_ids = Vec::new();
        let mut by_date: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        let result_count = results.len();

        for (memory, _score) in &results {
            let sid = memory.session_id.as_deref().unwrap_or("unknown");
            sessions.insert(sid.to_string());
            content_snippets.push(memory.content.clone());
            fact_ids.push(memory.id.to_string());
            let date_str = memory.t_valid.format("%Y/%m/%d").to_string();
            by_date
                .entry(date_str)
                .or_default()
                .push(format!("(session: {}) {}", sid, memory.content));
        }

        let mut output = format!("Found {} facts:\n", result_count);
        for (date, entries) in &by_date {
            output.push_str(&format!("{}\n", self.date_header(date)));
            for (i, entry) in entries.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, entry));
            }
        }

        Ok(ToolExecutionResult {
            text: output,
            sessions,
            content_snippets,
            result_count,
            fact_ids,
        })
    }

    // ── search_messages ─────────────────────────────────────────────────

    async fn exec_search_messages(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| Error::Agent(format!("Embedding failed: {}", e)))?;

        let results = self
            .storage
            .search_messages_hybrid(embedding, query, self.user_id_filter(), top_k)
            .await
            .map_err(|e| Error::Agent(format!("Message search failed: {}", e)))?;

        let mut by_date: std::collections::BTreeMap<String, Vec<(String, String, i64, String)>> =
            std::collections::BTreeMap::new();
        for r in &results {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let turn_idx = get_int_payload(&r.payload, "turn_index");
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((session_id, role, turn_idx, content));
        }

        let mut output = format!("Found {} messages:\n", results.len());
        for (date, entries) in &by_date {
            output.push_str(&format!("{}\n", self.date_header(date)));
            for (session_id, role, turn_idx, content) in entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                output.push_str(&format!(
                    "  [session: {}, turn: {}] {}: {}\n",
                    session_id, turn_idx, role_label, content
                ));
            }
        }
        Ok(output)
    }

    async fn exec_search_messages_structured(&self, args: &Value) -> Result<ToolExecutionResult> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| Error::Agent(format!("Embedding failed: {}", e)))?;

        let results = self
            .storage
            .search_messages_hybrid(embedding, query, self.user_id_filter(), top_k)
            .await
            .map_err(|e| Error::Agent(format!("Message search failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        let mut by_date: std::collections::BTreeMap<String, Vec<(String, String, i64, String)>> =
            std::collections::BTreeMap::new();
        for r in &results {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let turn_idx = get_int_payload(&r.payload, "turn_index");
            if !session_id.is_empty() {
                sessions.insert(session_id.clone());
            }
            content_snippets.push(content.clone());
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((session_id, role, turn_idx, content));
        }

        let mut text = format!("Found {} messages:\n", result_count);
        for (date, entries) in &by_date {
            text.push_str(&format!("{}\n", self.date_header(date)));
            for (session_id, role, turn_idx, content) in entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                text.push_str(&format!(
                    "  [session: {}, turn: {}] {}: {}\n",
                    session_id, turn_idx, role_label, content
                ));
            }
        }

        Ok(ToolExecutionResult {
            text,
            sessions,
            content_snippets,
            result_count,
            fact_ids: Vec::new(),
        })
    }

    // ── grep_messages ───────────────────────────────────────────────────

    async fn exec_grep_messages(&self, args: &Value) -> Result<String> {
        let substring = args["substring"].as_str().unwrap_or("");

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches_text("content", substring).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        let filter = Filter {
            must: must_conditions,
            ..Default::default()
        };
        let results = self
            .storage
            .scroll_messages(filter, 20)
            .await
            .map_err(|e| Error::Agent(format!("Grep failed: {}", e)))?;

        let mut by_date: std::collections::BTreeMap<String, Vec<(String, String, i64, String)>> =
            std::collections::BTreeMap::new();
        for r in &results {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let turn_idx = get_int_payload(&r.payload, "turn_index");
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((session_id, role, turn_idx, content));
        }

        let mut output = format!("Found {} matches for '{}':\n", results.len(), substring);
        for (date, entries) in &by_date {
            output.push_str(&format!("{}\n", self.date_header(date)));
            for (session_id, role, turn_idx, content) in entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                output.push_str(&format!(
                    "  [session: {}, turn: {}] {}: {}\n",
                    session_id, turn_idx, role_label, content
                ));
            }
        }
        Ok(output)
    }

    async fn exec_grep_messages_structured(&self, args: &Value) -> Result<ToolExecutionResult> {
        let substring = args["substring"].as_str().unwrap_or("");

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches_text("content", substring).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        let filter = Filter {
            must: must_conditions,
            ..Default::default()
        };
        let results = self
            .storage
            .scroll_messages(filter, 20)
            .await
            .map_err(|e| Error::Agent(format!("Grep failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        let mut by_date: std::collections::BTreeMap<String, Vec<(String, String, i64, String)>> =
            std::collections::BTreeMap::new();
        for r in &results {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let turn_idx = get_int_payload(&r.payload, "turn_index");
            if !session_id.is_empty() {
                sessions.insert(session_id.clone());
            }
            content_snippets.push(content.clone());
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((session_id, role, turn_idx, content));
        }

        let mut text = format!("Found {} matches for '{}':\n", result_count, substring);
        for (date, entries) in &by_date {
            text.push_str(&format!("{}\n", self.date_header(date)));
            for (session_id, role, turn_idx, content) in entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                text.push_str(&format!(
                    "  [session: {}, turn: {}] {}: {}\n",
                    session_id, turn_idx, role_label, content
                ));
            }
        }

        Ok(ToolExecutionResult {
            text,
            sessions,
            content_snippets,
            result_count,
            fact_ids: Vec::new(),
        })
    }

    // ── get_session_context ─────────────────────────────────────────────

    async fn exec_get_session_context(&self, args: &Value) -> Result<String> {
        let session_id = args["session_id"].as_str().unwrap_or("");
        let turn_index = args["turn_index"].as_i64().unwrap_or(0);
        let window = args["window"].as_i64().unwrap_or(5);
        let include_facts = args["include_facts"].as_bool().unwrap_or(false);

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("session_id", session_id.to_string()).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        let filter = Filter {
            must: must_conditions,
            ..Default::default()
        };
        let results = self
            .storage
            .scroll_messages(filter, 100)
            .await
            .map_err(|e| Error::Agent(format!("Session context failed: {}", e)))?;

        let mut turns: Vec<_> = results
            .iter()
            .map(|r| {
                let idx = get_int_payload(&r.payload, "turn_index");
                let content = get_string_payload(&r.payload, "content");
                let role = get_string_payload(&r.payload, "role");
                let t_valid = get_string_payload(&r.payload, "t_valid");
                (idx, role, content, t_valid)
            })
            .filter(|(idx, _, _, _)| *idx >= turn_index - window && *idx <= turn_index + window)
            .collect();
        turns.sort_by_key(|(idx, _, _, _)| *idx);

        let mut by_date: std::collections::BTreeMap<String, Vec<(i64, String, String)>> =
            std::collections::BTreeMap::new();
        for (idx, role, content, t_valid) in &turns {
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((*idx, role.clone(), content.clone()));
        }

        let mut output = String::new();
        for (date, entries) in &by_date {
            output.push_str(&format!("{}\n", self.date_header(date)));
            output.push_str(&format!("--- Session {} ---\n", session_id));
            for (idx, role, content) in entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                output.push_str(&format!("[turn {}] {}: {}\n", idx, role_label, content));
            }
        }

        if include_facts {
            let mut conditions: Vec<Condition> = vec![
                Condition::matches("session_id", session_id.to_string()),
            ];
            if let Some(ref uid) = self.user_id {
                conditions.push(Condition::matches("user_id", uid.clone()));
            }
            let fact_filter = Filter::must(conditions);
            let mut all_facts = Vec::new();
            for collection in COLLECTIONS {
                if let Ok(facts) = self
                    .storage
                    .scroll_collection(collection, fact_filter.clone(), 10)
                    .await
                {
                    all_facts.extend(facts);
                }
            }

            if !all_facts.is_empty() {
                output.push_str(&format!(
                    "\n=== Extracted Facts from Session {} ({} facts) ===\n",
                    session_id,
                    all_facts.len().min(10)
                ));
                for (i, f) in all_facts.iter().take(10).enumerate() {
                    let content = get_string_payload(&f.payload, "content");
                    let t_valid = get_string_payload(&f.payload, "t_valid");
                    output.push_str(&format!("{}. [{}] {}\n", i + 1, t_valid, content));
                }
            }
        }

        Ok(output)
    }

    async fn exec_get_session_context_structured(
        &self,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
        let session_id = args["session_id"].as_str().unwrap_or("");
        let turn_index = args["turn_index"].as_i64().unwrap_or(0);
        let window = args["window"].as_i64().unwrap_or(5);

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("session_id", session_id.to_string()).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        let filter = Filter {
            must: must_conditions,
            ..Default::default()
        };
        let results = self
            .storage
            .scroll_messages(filter, 100)
            .await
            .map_err(|e| Error::Agent(format!("Session context failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        if !session_id.is_empty() && result_count > 0 {
            sessions.insert(session_id.to_string());
        }

        let mut turns: Vec<_> = results
            .iter()
            .map(|r| {
                let idx = get_int_payload(&r.payload, "turn_index");
                let content = get_string_payload(&r.payload, "content");
                let role = get_string_payload(&r.payload, "role");
                let t_valid = get_string_payload(&r.payload, "t_valid");
                content_snippets.push(content.clone());
                (idx, role, content, t_valid)
            })
            .filter(|(idx, _, _, _)| *idx >= turn_index - window && *idx <= turn_index + window)
            .collect();
        turns.sort_by_key(|(idx, _, _, _)| *idx);

        let mut text = String::new();
        let mut by_date: std::collections::BTreeMap<String, Vec<(i64, String, String)>> =
            std::collections::BTreeMap::new();
        for (idx, role, content, t_valid) in &turns {
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((*idx, role.clone(), content.clone()));
        }
        for (date, entries) in &by_date {
            text.push_str(&format!("{}\n", self.date_header(date)));
            text.push_str(&format!("--- Session {} ---\n", session_id));
            for (idx, role, content) in entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                text.push_str(&format!("[turn {}] {}: {}\n", idx, role_label, content));
            }
        }

        Ok(ToolExecutionResult {
            text,
            sessions,
            content_snippets,
            result_count,
            fact_ids: Vec::new(),
        })
    }

    // ── get_by_date_range ───────────────────────────────────────────────

    async fn exec_get_by_date_range(&self, args: &Value) -> Result<String> {
        let start = args["start_date"].as_str().unwrap_or("");
        let end = args["end_date"].as_str().unwrap_or("");
        let query = args["query"].as_str();

        let ref_year = self.reference_year();
        let start_ts = parse_date_expression(start, ref_year);
        let end_ts = parse_date_expression(end, ref_year);

        let (start_ts, end_ts) = match (start_ts, end_ts) {
            (Some((s, _)), Some((_, e))) => (s, e),
            (Some((s, e)), None) => (s, e),
            _ => {
                return Ok(format!(
                    "Could not parse date range: '{}' to '{}'",
                    start, end
                ))
            }
        };

        let mut range_must: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::datetime_range(
                "t_valid",
                DatetimeRange {
                    gte: Some(start_ts.clone()),
                    lte: Some(end_ts.clone()),
                    ..Default::default()
                },
            )
            .into()];
        if let Some(uid) = &self.user_id {
            range_must.push(Condition::matches("user_id", uid.clone()).into());
        }

        let results = if let Some(q) = query {
            let mut combined_must: Vec<qdrant_client::qdrant::Condition> = vec![
                Condition::datetime_range(
                    "t_valid",
                    DatetimeRange {
                        gte: Some(start_ts),
                        lte: Some(end_ts),
                        ..Default::default()
                    },
                )
                .into(),
                Condition::matches_text("content", q).into(),
            ];
            if let Some(uid) = &self.user_id {
                combined_must.push(Condition::matches("user_id", uid.clone()).into());
            }
            let combined_filter = Filter {
                must: combined_must,
                ..Default::default()
            };
            self.storage
                .scroll_messages(combined_filter, 30)
                .await
                .map_err(|e| Error::Agent(format!("Date range search failed: {}", e)))?
        } else {
            let range_filter = Filter {
                must: range_must,
                ..Default::default()
            };
            self.storage
                .scroll_messages(range_filter, 50)
                .await
                .map_err(|e| Error::Agent(format!("Date range scroll failed: {}", e)))?
        };

        let mut by_date: std::collections::BTreeMap<String, Vec<(String, String, i64, String)>> =
            std::collections::BTreeMap::new();
        for r in &results {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let turn_idx = get_int_payload(&r.payload, "turn_index");
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((session_id, role, turn_idx, content));
        }

        let mut output = format!(
            "Found {} results from {} to {}:\n",
            results.len(),
            start,
            end
        );
        for (date, entries) in &by_date {
            output.push_str(&format!("{}\n", self.date_header(date)));
            let mut sorted_entries: Vec<_> = entries.iter().collect();
            sorted_entries.sort_by_key(|(_, _, turn_idx, _)| *turn_idx);
            for (session_id, role, turn_idx, content) in sorted_entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                output.push_str(&format!(
                    "  [session: {}, turn: {}] {}: {}\n",
                    session_id, turn_idx, role_label, content
                ));
            }
        }
        Ok(output)
    }

    async fn exec_get_by_date_range_structured(
        &self,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
        let start = args["start_date"].as_str().unwrap_or("");
        let end = args["end_date"].as_str().unwrap_or("");
        let query = args["query"].as_str();

        let ref_year = self.reference_year();
        let start_ts = parse_date_expression(start, ref_year);
        let end_ts = parse_date_expression(end, ref_year);

        let (start_ts, end_ts) = match (start_ts, end_ts) {
            (Some((s, _)), Some((_, e))) => (s, e),
            (Some((s, e)), None) => (s, e),
            _ => {
                return Ok(ToolExecutionResult {
                    text: format!("Could not parse date range: '{}' to '{}'", start, end),
                    sessions: std::collections::HashSet::new(),
                    content_snippets: Vec::new(),
                    result_count: 0,
                    fact_ids: Vec::new(),
                })
            }
        };

        let mut range_must: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::datetime_range(
                "t_valid",
                DatetimeRange {
                    gte: Some(start_ts.clone()),
                    lte: Some(end_ts.clone()),
                    ..Default::default()
                },
            )
            .into()];
        if let Some(uid) = &self.user_id {
            range_must.push(Condition::matches("user_id", uid.clone()).into());
        }

        let results = if let Some(q) = query {
            let mut combined_must: Vec<qdrant_client::qdrant::Condition> = vec![
                Condition::datetime_range(
                    "t_valid",
                    DatetimeRange {
                        gte: Some(start_ts),
                        lte: Some(end_ts),
                        ..Default::default()
                    },
                )
                .into(),
                Condition::matches_text("content", q).into(),
            ];
            if let Some(uid) = &self.user_id {
                combined_must.push(Condition::matches("user_id", uid.clone()).into());
            }
            let combined_filter = Filter {
                must: combined_must,
                ..Default::default()
            };
            self.storage
                .scroll_messages(combined_filter, 30)
                .await
                .map_err(|e| Error::Agent(format!("Date range search failed: {}", e)))?
        } else {
            let range_filter = Filter {
                must: range_must,
                ..Default::default()
            };
            self.storage
                .scroll_messages(range_filter, 50)
                .await
                .map_err(|e| Error::Agent(format!("Date range scroll failed: {}", e)))?
        };

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        let mut by_date: std::collections::BTreeMap<String, Vec<(String, String, i64, String)>> =
            std::collections::BTreeMap::new();
        for r in &results {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let turn_idx = get_int_payload(&r.payload, "turn_index");
            if !session_id.is_empty() {
                sessions.insert(session_id.clone());
            }
            content_snippets.push(content.clone());
            let date_key = if t_valid.len() >= 10 {
                t_valid[..10].to_string()
            } else {
                "unknown".to_string()
            };
            by_date
                .entry(date_key)
                .or_default()
                .push((session_id, role, turn_idx, content));
        }

        let mut text = format!(
            "Found {} results from {} to {}:\n",
            result_count, start, end
        );
        for (date, entries) in &by_date {
            text.push_str(&format!("{}\n", self.date_header(date)));
            let mut sorted_entries: Vec<_> = entries.iter().collect();
            sorted_entries.sort_by_key(|(_, _, turn_idx, _)| *turn_idx);
            for (session_id, role, turn_idx, content) in sorted_entries {
                let role_label = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    r => r,
                };
                text.push_str(&format!(
                    "  [session: {}, turn: {}] {}: {}\n",
                    session_id, turn_idx, role_label, content
                ));
            }
        }

        Ok(ToolExecutionResult {
            text,
            sessions,
            content_snippets,
            result_count,
            fact_ids: Vec::new(),
        })
    }

    // ── search_entity ───────────────────────────────────────────────────

    async fn exec_search_entity(&self, args: &Value) -> Result<String> {
        let entity = args["entity"].as_str().unwrap_or("");

        let mut entity_must: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("entity_ids", entity.to_lowercase()).into()];
        if let Some(uid) = &self.user_id {
            entity_must.push(Condition::matches("user_id", uid.clone()).into());
        }
        let entity_filter = Filter {
            must: entity_must,
            ..Default::default()
        };
        let mut fact_results = Vec::new();
        for collection in COLLECTIONS {
            if let Ok(results) = self
                .storage
                .scroll_collection(collection, entity_filter.clone(), 10)
                .await
            {
                fact_results.extend(results);
            }
        }

        let mut msg_must: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches_text("content", entity).into()];
        if let Some(uid) = &self.user_id {
            msg_must.push(Condition::matches("user_id", uid.clone()).into());
        }
        let msg_filter = Filter {
            must: msg_must,
            ..Default::default()
        };
        let msg_results = self
            .storage
            .scroll_messages(msg_filter, 20)
            .await
            .map_err(|e| Error::Agent(format!("Entity message search failed: {}", e)))?;

        let mut output = format!(
            "Entity '{}': {} facts, {} messages\n",
            entity,
            fact_results.len(),
            msg_results.len()
        );

        output.push_str("\n--- Facts ---\n");
        for (i, r) in fact_results.iter().enumerate() {
            let content = get_string_payload(&r.payload, "content");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            let session_id = get_string_payload(&r.payload, "session_id");
            output.push_str(&format!(
                "{}. [{}] (session: {}) {}\n",
                i + 1,
                t_valid,
                session_id,
                content
            ));
        }

        output.push_str("\n--- Messages ---\n");
        for (i, r) in msg_results.iter().enumerate() {
            let content = get_string_payload(&r.payload, "content");
            let session_id = get_string_payload(&r.payload, "session_id");
            let role = get_string_payload(&r.payload, "role");
            let t_valid = get_string_payload(&r.payload, "t_valid");
            output.push_str(&format!(
                "{}. [{}] ({}, session: {}) {}\n",
                i + 1,
                t_valid,
                role,
                session_id,
                content
            ));
        }

        Ok(output)
    }

    // ── date_diff ───────────────────────────────────────────────────────

    /// Deterministic date arithmetic — compute exact difference between two dates.
    fn exec_date_diff(&self, args: &Value) -> Result<String> {
        let start = args["start_date"].as_str().unwrap_or("");
        let end = args["end_date"].as_str().unwrap_or("");
        let unit = args["unit"].as_str().unwrap_or("days");
        let inclusive = args["inclusive"].as_bool().unwrap_or(false);

        let parse_date = |s: &str| -> Option<chrono::NaiveDate> {
            let s = s.trim().replace('/', "-");
            chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()
        };

        let start_date = match parse_date(start) {
            Some(d) => d,
            None => {
                return Ok(format!(
                    "Error: Could not parse start_date '{}'. Use YYYY/MM/DD or YYYY-MM-DD format.",
                    start
                ))
            }
        };
        let end_date = match parse_date(end) {
            Some(d) => d,
            None => {
                return Ok(format!(
                    "Error: Could not parse end_date '{}'. Use YYYY/MM/DD or YYYY-MM-DD format.",
                    end
                ))
            }
        };

        let days = (end_date - start_date).num_days();
        let days_adj = if inclusive {
            days.abs() + 1
        } else {
            days.abs()
        };

        let result = match unit {
            "days" => format!("{} days", days_adj),
            "weeks" => {
                let weeks = days_adj / 7;
                let remainder = days_adj % 7;
                if remainder == 0 {
                    format!("{} weeks", weeks)
                } else {
                    format!(
                        "{} weeks and {} days ({} days total)",
                        weeks, remainder, days_adj
                    )
                }
            }
            "months" => {
                let mut months = (end_date.year() - start_date.year()) * 12
                    + (end_date.month() as i32 - start_date.month() as i32);
                let mut remaining_days = end_date.day() as i32 - start_date.day() as i32;
                if remaining_days < 0 {
                    months -= 1;
                    remaining_days += 30;
                }
                months = months.abs();
                remaining_days = remaining_days.abs();
                if remaining_days == 0 {
                    format!("{} months", months)
                } else {
                    format!("{} months and {} days", months, remaining_days)
                }
            }
            "years" => {
                let total_months = ((end_date.year() - start_date.year()) * 12
                    + (end_date.month() as i32 - start_date.month() as i32))
                    .abs();
                let years = total_months / 12;
                let months = total_months % 12;
                if months == 0 {
                    format!("{} years", years)
                } else {
                    format!("{} years and {} months", years, months)
                }
            }
            _ => format!("{} days", days_adj),
        };

        let direction = if days >= 0 { "after" } else { "before" };
        Ok(format!(
            "{} is {} {} {} (start: {}, end: {}{})",
            end,
            result,
            direction,
            start,
            start_date.format("%Y/%m/%d"),
            end_date.format("%Y/%m/%d"),
            if inclusive { ", inclusive" } else { "" }
        ))
    }
}
