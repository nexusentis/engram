//! POST /v1/memories/search

use axum::extract::State;
use axum::Json;
use qdrant_client::qdrant::{Condition, DatetimeRange, Filter, Timestamp};
use std::time::Instant;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::{ErrorResponse, MemoryResult, SearchFilters, SearchRequest, SearchResponse, METRICS};

const MAX_LIMIT: usize = 100;

/// Build a Qdrant `Filter` from base conditions (user_id + is_latest) plus
/// any active `SearchFilters` (fact_types, entity_ids, time_range).
fn build_qdrant_filter(user_id: &str, filters: Option<&SearchFilters>) -> Filter {
    let mut must: Vec<Condition> = vec![
        Condition::matches("user_id", user_id.to_string()),
        Condition::matches("is_latest", true),
    ];

    if let Some(f) = filters {
        // fact_types → OR over requested types (nested should)
        if let Some(types) = &f.fact_types {
            if !types.is_empty() {
                let should: Vec<Condition> = types
                    .iter()
                    .map(|t| Condition::matches("fact_type", t.clone()))
                    .collect();
                must.push(Condition::from(Filter {
                    should,
                    must: vec![],
                    must_not: vec![],
                    min_should: None,
                }));
            }
        }

        // entity_ids → OR over requested entities (nested should)
        if let Some(ids) = &f.entity_ids {
            if !ids.is_empty() {
                let should: Vec<Condition> = ids
                    .iter()
                    .map(|id| Condition::matches("entity_ids", id.clone()))
                    .collect();
                must.push(Condition::from(Filter {
                    should,
                    must: vec![],
                    must_not: vec![],
                    min_should: None,
                }));
            }
        }

        // time_range → datetime_range on t_valid (start inclusive, end exclusive)
        if let Some(range) = &f.time_range {
            let gte = range.start.map(|s| Timestamp {
                seconds: s.timestamp(),
                nanos: 0,
            });
            let lt = range.end.map(|e| Timestamp {
                seconds: e.timestamp(),
                nanos: 0,
            });
            if gte.is_some() || lt.is_some() {
                must.push(Condition::datetime_range(
                    "t_valid",
                    DatetimeRange {
                        gte,
                        lt,
                        ..Default::default()
                    },
                ));
            }
        }
    }

    Filter {
        must,
        should: vec![],
        must_not: vec![],
        min_should: None,
    }
}

#[utoipa::path(
    post,
    path = "/v1/memories/search",
    request_body = SearchRequest,
    responses(
        (status = 200, description = "Search results", body = SearchResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
    ),
    tag = "memories"
)]
pub async fn search_memories(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> AppResult<Json<SearchResponse>> {
    let start = Instant::now();

    if req.user_id.trim().is_empty() {
        return Err(AppError::BadRequest("user_id is required".into()));
    }
    if req.user_id.len() > 256 {
        return Err(AppError::BadRequest("user_id exceeds 256 characters".into()));
    }
    if req.query.trim().is_empty() {
        return Err(AppError::BadRequest("query must not be empty".into()));
    }
    if req.query.len() > 10_000 {
        return Err(AppError::BadRequest("query exceeds 10000 characters".into()));
    }
    if req.limit == 0 {
        return Err(AppError::BadRequest("limit must be >= 1".into()));
    }

    if let Some(ref f) = req.filters {
        if let Some(mc) = f.min_confidence {
            if !(0.0..=1.0).contains(&mc) {
                return Err(AppError::BadRequest("min_confidence must be between 0.0 and 1.0".into()));
            }
        }
        if let Some(ref tr) = f.time_range {
            if let (Some(s), Some(e)) = (tr.start, tr.end) {
                if s >= e {
                    return Err(AppError::BadRequest("time_range start must be before end".into()));
                }
            }
        }
    }

    let limit = req.limit.min(MAX_LIMIT);

    let query_vector = state
        .embedder
        .embed_query(&req.query)
        .await
        .map_err(|e| AppError::Internal(format!("Embedding failed: {e}")))?;

    let filter = build_qdrant_filter(&req.user_id, req.filters.as_ref());

    let results = state
        .qdrant
        .search_memories_with_filter(filter, query_vector, limit as u64, None)
        .await?;

    let min_confidence = req
        .filters
        .as_ref()
        .and_then(|f| f.min_confidence)
        .unwrap_or(0.0);

    let memories: Vec<MemoryResult> = results
        .into_iter()
        .filter(|(m, _)| m.confidence >= min_confidence)
        .map(|(m, score)| MemoryResult {
            id: m.id,
            content: m.content,
            confidence: m.confidence,
            score,
            fact_type: format!("{:?}", m.fact_type),
            entities: m.entity_ids,
            t_valid: m.t_valid,
            t_created: m.t_created,
            supersedes_id: m.supersedes_id,
        })
        .collect();

    let total_found = memories.len();

    METRICS.record_retrieval(total_found, false, start.elapsed().as_secs_f64());

    tracing::debug!(
        query_len = req.query.len(),
        results = total_found,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "search completed"
    );

    Ok(Json(SearchResponse {
        memories,
        total_found,
        search_time_ms: start.elapsed().as_millis() as u64,
        abstained: false,
        abstention_reason: None,
    }))
}
