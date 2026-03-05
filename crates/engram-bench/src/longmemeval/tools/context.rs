//! Context retrieval tool implementations: get_session_context, get_by_date_range.

use qdrant_client::qdrant::{Condition, DatetimeRange, Filter};
use serde_json::Value;

use crate::error::{BenchmarkError, Result};

use super::date_parsing::parse_date_expression;
use super::types::{get_int_payload, get_string_payload, ToolExecutionResult};
use super::ToolExecutor;

impl ToolExecutor {
    pub(super) async fn exec_get_session_context(&self, args: &Value) -> Result<String> {
        let session_id = args["session_id"].as_str().unwrap_or("");
        let turn_index = args["turn_index"].as_i64().unwrap_or(0);
        let window = args["window"].as_i64().unwrap_or(5);
        let include_facts = args["include_facts"].as_bool().unwrap_or(false);

        // Part 1: Raw message turns
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
            .map_err(|e| BenchmarkError::Answering(format!("Session context failed: {}", e)))?;

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

        // Date-grouped format
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

        // Part 2: Extracted facts from the same session (agent-controlled)
        if include_facts {
            let fact_filter =
                Filter::must([Condition::matches("session_id", session_id.to_string())]);
            let mut all_facts = Vec::new();
            for collection in engram::storage::COLLECTIONS {
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

    pub(super) async fn exec_get_by_date_range(&self, args: &Value) -> Result<String> {
        let start = args["start_date"].as_str().unwrap_or("");
        let end = args["end_date"].as_str().unwrap_or("");
        let query = args["query"].as_str();

        // Parse date expressions (supports YYYY/MM/DD, Month YYYY, YYYY, Q1 2023)
        let start_ts = parse_date_expression(start);
        let end_ts = parse_date_expression(end);

        let (start_ts, end_ts) = match (start_ts, end_ts) {
            (Some((s, _)), Some((_, e))) => (s, e),
            (Some((s, e)), None) => (s, e), // Single date expression for both
            _ => {
                return Ok(format!(
                    "Could not parse date range: '{}' to '{}'",
                    start, end
                ))
            }
        };

        // Build proper Qdrant datetime range filter on t_valid
        let mut range_must: Vec<qdrant_client::qdrant::Condition> =
            vec![Condition::datetime_range(
                "t_valid",
                DatetimeRange {
                    gte: Some(start_ts),
                    lte: Some(end_ts),
                    ..Default::default()
                },
            )
            .into()];
        if let Some(uid) = &self.user_id {
            range_must.push(Condition::matches("user_id", uid.clone()).into());
        }
        let range_filter = Filter {
            must: range_must,
            ..Default::default()
        };

        let results = if let Some(q) = query {
            // Combine date range with fulltext search
            let mut combined_must: Vec<qdrant_client::qdrant::Condition> = vec![
                Condition::datetime_range(
                    "t_valid",
                    DatetimeRange {
                        gte: Some(start_ts.clone()),
                        lte: Some(end_ts.clone()),
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
                .map_err(|e| {
                    BenchmarkError::Answering(format!("Date range search failed: {}", e))
                })?
        } else {
            // No query — scroll all messages in date range
            self.storage
                .scroll_messages(range_filter, 50)
                .await
                .map_err(|e| {
                    BenchmarkError::Answering(format!("Date range scroll failed: {}", e))
                })?
        };

        // Date-grouped format
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

    // ---- Structured variants for recall harness ----

    pub(super) async fn exec_get_session_context_structured(
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
            .map_err(|e| BenchmarkError::Answering(format!("Session context failed: {}", e)))?;

        let mut sessions = std::collections::HashSet::new();
        let mut content_snippets = Vec::new();
        let result_count = results.len();

        if !session_id.is_empty() && result_count > 0 {
            sessions.insert(session_id.to_string());
        }

        // Build text from single query results
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

    pub(super) async fn exec_get_by_date_range_structured(
        &self,
        args: &Value,
    ) -> Result<ToolExecutionResult> {
        let start = args["start_date"].as_str().unwrap_or("");
        let end = args["end_date"].as_str().unwrap_or("");
        let query = args["query"].as_str();

        let start_ts = parse_date_expression(start);
        let end_ts = parse_date_expression(end);

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
                .map_err(|e| {
                    BenchmarkError::Answering(format!("Date range search failed: {}", e))
                })?
        } else {
            let range_filter = Filter {
                must: range_must,
                ..Default::default()
            };
            self.storage
                .scroll_messages(range_filter, 50)
                .await
                .map_err(|e| {
                    BenchmarkError::Answering(format!("Date range scroll failed: {}", e))
                })?
        };

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
}
