//! POST /v1/messages/search

use axum::extract::State;
use axum::Json;
use qdrant_client::qdrant::{Condition, Filter};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use std::time::Instant;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::ErrorResponse;

const MAX_LIMIT: usize = 100;

#[derive(Debug, Deserialize, ToSchema)]
pub struct MessagesSearchRequest {
    /// User ID to scope the search (required for tenant isolation)
    pub user_id: String,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessagesSearchResponse {
    pub results: Vec<MessageHit>,
    pub total_found: usize,
    pub search_time_ms: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageHit {
    pub score: f32,
    pub payload: serde_json::Value,
}

#[utoipa::path(
    post,
    path = "/v1/messages/search",
    request_body = MessagesSearchRequest,
    responses(
        (status = 200, description = "Search results", body = MessagesSearchResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
    ),
    tag = "messages"
)]
pub async fn search_messages(
    State(state): State<AppState>,
    Json(req): Json<MessagesSearchRequest>,
) -> AppResult<Json<MessagesSearchResponse>> {
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

    let limit = req.limit.min(MAX_LIMIT);

    let query_vector = state
        .embedder
        .embed_query(&req.query)
        .await
        .map_err(|e| AppError::Internal(format!("Embedding failed: {e}")))?;

    // Scope search to the requesting user
    let user_filter = Filter::must([Condition::matches("user_id", req.user_id)]);

    let scored_points = state
        .qdrant
        .search_messages_hybrid(query_vector, &req.query, Some(user_filter), limit)
        .await?;

    let results: Vec<MessageHit> = scored_points
        .into_iter()
        .map(|sp| {
            let payload: serde_json::Value = sp
                .payload
                .into_iter()
                .map(|(k, v)| (k, serde_json::to_value(v).unwrap_or_default()))
                .collect::<serde_json::Map<_, _>>()
                .into();
            MessageHit {
                score: sp.score,
                payload,
            }
        })
        .collect();

    let total_found = results.len();

    tracing::debug!(
        query_len = req.query.len(),
        results = total_found,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "messages search completed"
    );

    Ok(Json(MessagesSearchResponse {
        results,
        total_found,
        search_time_ms: start.elapsed().as_millis() as u64,
    }))
}
