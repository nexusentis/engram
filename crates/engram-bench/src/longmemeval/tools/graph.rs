//! Graph-based tool implementations: graph_lookup, graph_relationships,
//! graph_disambiguate, graph_enumerate, date_diff.

use chrono::Datelike;
use serde_json::Value;

use crate::error::Result;

use super::types::get_string_payload;
use super::ToolExecutor;

impl ToolExecutor {
    /// graph_lookup: Entity profile from SurrealDB knowledge graph
    pub(super) async fn exec_graph_lookup(&self, args: &Value) -> Result<String> {
        let entity_name = args["entity"].as_str().unwrap_or("");
        let user_id = match &self.user_id {
            Some(uid) => uid.as_str(),
            None => return Ok("Graph lookup requires a user context.".to_string()),
        };

        let graph = match &self.graph_store {
            Some(g) => g,
            None => return Ok("Knowledge graph is not enabled.".to_string()),
        };

        let exact_entities = match graph.get_entities_by_name(user_id, entity_name).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Graph lookup failed for '{}': {}", entity_name, e);
                return Ok(format!("Graph lookup error for '{}'.", entity_name));
            }
        };

        // Fuzzy fallback: if exact match fails, try substring search
        const LOOKUP_FUZZY_LIMIT: usize = 5;
        let (entities, is_fuzzy) = if exact_entities.is_empty() {
            match graph
                .search_entities_fuzzy(user_id, entity_name, LOOKUP_FUZZY_LIMIT)
                .await
            {
                Ok(fuzzy) if !fuzzy.is_empty() => (fuzzy, true),
                Ok(_) => {
                    return Ok(format!(
                        "Entity '{}' not found in the knowledge graph.",
                        entity_name
                    ));
                }
                Err(e) => {
                    tracing::warn!("Graph fuzzy lookup failed for '{}': {}", entity_name, e);
                    return Ok(format!(
                        "Graph lookup error for '{}' (database unavailable).",
                        entity_name
                    ));
                }
            }
        } else {
            (exact_entities, false)
        };

        let mut output = if is_fuzzy {
            format!(
                "No exact match for '{}'. Showing {} similar entities (substring match):\n",
                entity_name,
                entities.len()
            )
        } else {
            String::new()
        };

        for (i, entity) in entities.iter().enumerate() {
            let entity_id = match &entity.id {
                Some(id) => id,
                None => continue,
            };

            // Get profile (relationships + sessions)
            match graph.entity_profile(user_id, entity_id).await {
                Err(e) => {
                    tracing::warn!("Graph profile failed for entity {:?}: {}", entity_id, e);
                    continue;
                }
                Ok(None) => continue,
                Ok(Some(profile)) => {
                if entities.len() > 1 {
                    output.push_str(&format!(
                        "\n=== Match {} ===\n",
                        i + 1,
                    ));
                }
                output.push_str(&format!(
                    "Entity '{}' (type: {}): {} mentions, {} sessions\n",
                    entity.name,
                    entity.entity_type,
                    profile.mention_count,
                    profile.session_count,
                ));

                if !entity.aliases.is_empty() {
                    output.push_str(&format!("Aliases: {}\n", entity.aliases.join(", ")));
                }

                if !profile.relationships.is_empty() {
                    output.push_str("\n--- Relationships ---\n");
                    for (rel, other, is_outgoing) in &profile.relationships {
                        if *is_outgoing {
                            output.push_str(&format!(
                                "- {} -> {} (type: {}, confidence: {:.1})\n",
                                rel.relation_type, other.name, other.entity_type, rel.confidence
                            ));
                        } else {
                            output.push_str(&format!(
                                "- {} <- {} (type: {}, confidence: {:.1})\n",
                                rel.relation_type, other.name, other.entity_type, rel.confidence
                            ));
                        }
                    }
                }

                // Show associated facts (via mentions → Qdrant lookup)
                if let Ok(fact_ids) = graph.facts_for_entity(user_id, entity_id).await {
                    if !fact_ids.is_empty() {
                        output.push_str(&format!("\n--- Facts ({}) ---\n", fact_ids.len()));
                        let mut facts_found = 0;
                        for fact_id in &fact_ids {
                            if facts_found >= 20 {
                                output.push_str("(truncated)\n");
                                break;
                            }
                            match self.storage.get_point_by_id_any_collection(fact_id).await {
                                Ok(Some(point)) => {
                                    // Defense-in-depth: verify fact belongs to this user
                                    let fact_user = get_string_payload(&point.payload, "user_id");
                                    if !fact_user.is_empty() && fact_user != user_id {
                                        tracing::warn!(
                                            "Graph mention points to fact {} owned by '{}', expected '{}'",
                                            fact_id, fact_user, user_id
                                        );
                                        continue;
                                    }
                                    let content = get_string_payload(&point.payload, "content");
                                    let t_valid = get_string_payload(&point.payload, "t_valid");
                                    let session_id =
                                        get_string_payload(&point.payload, "session_id");
                                    let date = t_valid.split('T').next().unwrap_or(&t_valid);
                                    output.push_str(&format!(
                                        "- [{}] (session: {}) {}\n",
                                        date, session_id, content
                                    ));
                                    facts_found += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                } // Ok(Some(profile))
            } // match
        }

        if output.is_empty() || (!is_fuzzy && output.trim().is_empty()) {
            return Ok(format!(
                "Entity '{}' was found but profile data is unavailable.",
                entity_name
            ));
        }

        Ok(output)
    }

    /// graph_relationships: Find entities by relationship type
    pub(super) async fn exec_graph_relationships(&self, args: &Value) -> Result<String> {
        let user_id = match &self.user_id {
            Some(uid) => uid.as_str(),
            None => return Ok("Graph relationships requires a user context.".to_string()),
        };

        let graph = match &self.graph_store {
            Some(g) => g,
            None => return Ok("Knowledge graph is not enabled.".to_string()),
        };

        let entity_name = args["entity"].as_str().unwrap_or("");
        let relation = args["relation"].as_str();

        // Get all entities with this name
        let entities = match graph.get_entities_by_name(user_id, entity_name).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Graph relationships lookup failed for '{}': {}", entity_name, e);
                return Ok(format!("Graph lookup error for '{}'.", entity_name));
            }
        };

        if entities.is_empty() {
            return Ok(format!(
                "Entity '{}' not found in the knowledge graph.",
                entity_name
            ));
        }

        let mut output = String::new();

        for entity in &entities {
            let entity_id = match &entity.id {
                Some(id) => id,
                None => continue,
            };

            let rels = match graph.get_relationships_for(user_id, entity_id).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Graph relationships query failed for {:?}: {}", entity_id, e);
                    continue;
                }
            };

            // Filter by relation type if specified
            let filtered: Vec<_> = if let Some(rel_type) = relation {
                if rel_type == "all" {
                    rels
                } else {
                    rels.into_iter()
                        .filter(|(r, _, _)| {
                            r.relation_type.to_lowercase() == rel_type.to_lowercase()
                        })
                        .collect()
                }
            } else {
                rels
            };

            output.push_str(&format!(
                "Entity '{}' ({}): {} relationships\n",
                entity.name,
                entity.entity_type,
                filtered.len()
            ));

            for (rel, other, is_outgoing) in &filtered {
                if *is_outgoing {
                    output.push_str(&format!(
                        "- {} -> {} (type: {}, confidence: {:.1})\n",
                        rel.relation_type, other.name, other.entity_type, rel.confidence
                    ));
                } else {
                    output.push_str(&format!(
                        "- {} <- {} (type: {}, confidence: {:.1})\n",
                        rel.relation_type, other.name, other.entity_type, rel.confidence
                    ));
                }
            }
        }

        if output.trim().is_empty() {
            return Ok(format!(
                "Entity '{}' was found but relationship data is unavailable.",
                entity_name
            ));
        }

        Ok(output)
    }

    /// graph_disambiguate: Explicit entity disambiguation
    pub(super) async fn exec_graph_disambiguate(&self, args: &Value) -> Result<String> {
        let entity_name = args["name"].as_str().unwrap_or("");
        let user_id = match &self.user_id {
            Some(uid) => uid.as_str(),
            None => return Ok("Graph disambiguate requires a user context.".to_string()),
        };

        let graph = match &self.graph_store {
            Some(g) => g,
            None => return Ok("Knowledge graph is not enabled.".to_string()),
        };

        // Parse context keywords
        let context: Vec<String> = match &args["context"] {
            Value::Array(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            Value::String(s) => s.split(',').map(|s| s.trim().to_string()).collect(),
            _ => vec![],
        };

        let candidates = match graph.disambiguate(user_id, entity_name, &context).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Graph disambiguate failed for '{}': {}", entity_name, e);
                return Ok(format!("Graph disambiguation error for '{}'.", entity_name));
            }
        };

        // Fuzzy fallback: if exact match found nothing, try substring search
        const DISAMBIG_FUZZY_LIMIT: usize = 10;
        let (candidates, is_fuzzy) = if candidates.is_empty() {
            let fuzzy_entities = match graph
                .search_entities_fuzzy(user_id, entity_name, DISAMBIG_FUZZY_LIMIT)
                .await
            {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(
                        "Graph fuzzy disambiguate failed for '{}': {}",
                        entity_name,
                        e
                    );
                    return Ok(format!(
                        "Graph disambiguation error for '{}' (database unavailable).",
                        entity_name
                    ));
                }
            };
            if fuzzy_entities.is_empty() {
                return Ok(format!(
                    "No entities named '{}' found in the knowledge graph.",
                    entity_name
                ));
            }
            // Wrap fuzzy results as disambiguation candidates with basic scoring
            let fuzzy_candidates: Vec<engram::storage::DisambiguationCandidate> = fuzzy_entities
                .into_iter()
                .map(|e| {
                    let score = (e.mention_count as f32).ln().max(0.0) * 0.5;
                    engram::storage::DisambiguationCandidate {
                        entity: e,
                        relationships: vec![],
                        score,
                    }
                })
                .collect();
            (fuzzy_candidates, true)
        } else {
            (candidates, false)
        };

        let mut output = if is_fuzzy {
            format!(
                "No exact match for '{}'. Showing {} similar entities (substring match):\n",
                entity_name,
                candidates.len()
            )
        } else {
            format!(
                "Disambiguation for '{}' ({} candidates):\n",
                entity_name,
                candidates.len()
            )
        };

        for (i, candidate) in candidates.iter().enumerate() {
            output.push_str(&format!(
                "\n{}. {} (type: {}, score: {:.2})\n",
                i + 1,
                candidate.entity.name,
                candidate.entity.entity_type,
                candidate.score
            ));
            output.push_str(&format!(
                "   Mentions: {}\n",
                candidate.entity.mention_count,
            ));
            // Show distinguishing relationships
            for (rel_type, target_name) in &candidate.relationships {
                output.push_str(&format!(
                    "   - {} {}\n",
                    rel_type, target_name
                ));
            }
        }

        Ok(output)
    }

    /// graph_enumerate: List entities by type with count and optional keyword filter
    pub(super) async fn exec_graph_enumerate(&self, args: &Value) -> Result<String> {
        let entity_type = args["entity_type"].as_str().unwrap_or("");
        let keyword = args["keyword"].as_str().map(|s| s.to_lowercase());
        let limit = args["limit"]
            .as_i64()
            .unwrap_or(30)
            .min(100)
            .max(1) as usize;

        let user_id = match &self.user_id {
            Some(uid) => uid.as_str(),
            None => return Ok("Graph enumerate requires a user context.".to_string()),
        };

        let graph = match &self.graph_store {
            Some(g) => g,
            None => return Ok("Knowledge graph is not enabled.".to_string()),
        };

        let entities = match graph.find_entities_by_type(user_id, entity_type).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    "Graph enumerate failed for type '{}': {}",
                    entity_type,
                    e
                );
                return Ok(format!(
                    "Graph enumerate error for type '{}'.",
                    entity_type
                ));
            }
        };

        // Already sorted by mention_count DESC from the query
        let total_unfiltered = entities.len();

        // Apply keyword filter if provided
        let filtered: Vec<_> = if let Some(ref kw) = keyword {
            entities
                .into_iter()
                .filter(|e| e.name.to_lowercase().contains(kw))
                .collect()
        } else {
            entities
        };

        let total_matches = filtered.len();
        let displayed = total_matches.min(limit);

        let mut output = if keyword.is_some() {
            format!(
                "Found {} entities of type '{}' matching keyword '{}' (out of {} total, showing top {}):\n",
                total_matches,
                entity_type,
                keyword.as_deref().unwrap_or(""),
                total_unfiltered,
                displayed,
            )
        } else {
            format!(
                "Found {} entities of type '{}' (showing top {}):\n",
                total_matches, entity_type, displayed,
            )
        };

        for entity in filtered.iter().take(limit) {
            let last_seen = entity
                .last_seen
                .0
                .format("%Y/%m/%d")
                .to_string();
            output.push_str(&format!(
                "- {} ({} mentions, last seen {})\n",
                entity.name, entity.mention_count, last_seen,
            ));
        }

        if total_matches > displayed {
            output.push_str(&format!(
                "... and {} more. Use keyword filter to narrow.\n",
                total_matches - displayed,
            ));
        }

        Ok(output)
    }

    /// Deterministic date arithmetic — compute exact difference between two dates
    pub(super) fn exec_date_diff(&self, args: &Value) -> Result<String> {
        let start = args["start_date"].as_str().unwrap_or("");
        let end = args["end_date"].as_str().unwrap_or("");
        let unit = args["unit"].as_str().unwrap_or("days");
        let inclusive = args["inclusive"].as_bool().unwrap_or(false);

        // Parse dates (support YYYY/MM/DD and YYYY-MM-DD)
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
                // Approximate month calculation using actual calendar months
                let mut months = (end_date.year() - start_date.year()) * 12
                    + (end_date.month() as i32 - start_date.month() as i32);
                let mut remaining_days = end_date.day() as i32 - start_date.day() as i32;
                if remaining_days < 0 {
                    months -= 1;
                    remaining_days += 30; // approximate
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
                // Use total-months arithmetic to handle month borrowing correctly
                // e.g. 2014/05 → 2019/02 = 57 months = 4 years 9 months (not 5y 3m)
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
