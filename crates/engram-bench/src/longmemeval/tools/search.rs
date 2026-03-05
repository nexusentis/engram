//! Search tool implementations: search_facts, search_messages, grep_messages, search_entity.

use qdrant_client::qdrant::{Condition, Filter};
use serde_json::Value;

use engram::embedding::EmbeddingProvider;
use crate::error::{BenchmarkError, Result};

use super::types::{get_int_payload, get_string_payload, ToolExecutionResult};
use super::ToolExecutor;

impl ToolExecutor {
    pub(super) async fn exec_search_facts(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as u64;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Embedding failed: {}", e)))?;

        // Build filter with user_id scoping + is_latest + optional observation_level
        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("is_latest", true).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        if let Some(level) = args["level"].as_str() {
            must_conditions.push(Condition::matches("observation_level", level.to_string()).into());
        }
        let filter = if must_conditions.is_empty() {
            Filter::default()
        } else {
            Filter {
                must: must_conditions,
                ..Default::default()
            }
        };

        // Hybrid search: vector + full-text with RRF fusion across all 4 fact collections
        let results = self
            .storage
            .search_memories_hybrid(filter, embedding, query, top_k, None)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Search failed: {}", e)))?;

        // Group by date for readability
        let mut by_date: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for (memory, _score) in &results {
            let date_str = memory.t_valid.format("%Y/%m/%d").to_string();
            let sid = memory.session_id.as_deref().unwrap_or("unknown");
            by_date
                .entry(date_str.clone())
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

    pub(super) async fn exec_search_messages(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Embedding failed: {}", e)))?;

        let results = self
            .storage
            .search_messages_hybrid(embedding, query, self.user_id_filter(), top_k)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Message search failed: {}", e)))?;

        // Group by date for readability
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

    pub(super) async fn exec_grep_messages(&self, args: &Value) -> Result<String> {
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
            .map_err(|e| BenchmarkError::Answering(format!("Grep failed: {}", e)))?;

        // Group by date for readability
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

    // ---- Structured variants for recall harness ----

    pub(super) async fn exec_search_facts_structured(
        &self,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as u64;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Embedding failed: {}", e)))?;

        let mut must_conditions: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::matches("is_latest", true).into()];
        if let Some(uid) = &self.user_id {
            must_conditions.push(Condition::matches("user_id", uid.clone()).into());
        }
        if let Some(level) = args["level"].as_str() {
            must_conditions.push(Condition::matches("observation_level", level.to_string()).into());
        }
        let filter = if must_conditions.is_empty() {
            Filter::default()
        } else {
            Filter {
                must: must_conditions,
                ..Default::default()
            }
        };

        let results = self
            .storage
            .search_memories_hybrid(filter, embedding, query, top_k, None)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Search failed: {}", e)))?;

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
                .entry(date_str.clone())
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

    pub(super) async fn exec_search_messages_structured(
        &self,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
        let query = args["query"].as_str().unwrap_or("");
        let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Embedding failed: {}", e)))?;

        let results = self
            .storage
            .search_messages_hybrid(embedding, query, self.user_id_filter(), top_k)
            .await
            .map_err(|e| BenchmarkError::Answering(format!("Message search failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        // Build text + structured data in single pass over results
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

    pub(super) async fn exec_grep_messages_structured(
        &self,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
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
            .map_err(|e| BenchmarkError::Answering(format!("Grep failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        // Build text + structured data in single pass
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

    pub(super) async fn exec_search_entity(&self, args: &Value) -> Result<String> {
        let entity = args["entity"].as_str().unwrap_or("");

        // Search facts by entity_ids keyword field across all fact collections
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
        for collection in engram::storage::COLLECTIONS {
            if let Ok(results) = self
                .storage
                .scroll_collection(collection, entity_filter.clone(), 10)
                .await
            {
                fact_results.extend(results);
            }
        }

        // Search messages by content grep (user-scoped)
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
            .map_err(|e| {
                BenchmarkError::Answering(format!("Entity message search failed: {}", e))
            })?;

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
}
