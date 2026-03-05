//! GET /v1/memories — list user memories with pagination

use axum::extract::{Query, State};
use axum::Json;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::{ErrorResponse, MemoryResult, UserMemoriesRequest, UserMemoriesResponse};

const MAX_LIMIT: usize = 100;
const FETCH_CAP: u64 = 10_000;

#[utoipa::path(
    get,
    path = "/v1/memories",
    params(UserMemoriesRequest),
    responses(
        (status = 200, description = "User memories", body = UserMemoriesResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
    ),
    tag = "memories"
)]
pub async fn list_memories(
    State(state): State<AppState>,
    Query(req): Query<UserMemoriesRequest>,
) -> AppResult<Json<UserMemoriesResponse>> {
    if req.user_id.trim().is_empty() {
        return Err(AppError::BadRequest("user_id is required".into()));
    }
    if req.user_id.len() > 256 {
        return Err(AppError::BadRequest("user_id exceeds 256 characters".into()));
    }
    if req.limit == 0 {
        return Err(AppError::BadRequest("limit must be >= 1".into()));
    }

    let limit = req.limit.min(MAX_LIMIT);

    // Fetch all user memories (capped) — paginate in-memory.
    // list_user_memories applies limit per-collection, so pass FETCH_CAP.
    let memories = state
        .qdrant
        .list_user_memories(&req.user_id, None, FETCH_CAP)
        .await?;

    let total = memories.len();
    let paged: Vec<MemoryResult> = memories
        .into_iter()
        .skip(req.offset)
        .take(limit)
        .map(|m| MemoryResult {
            id: m.id,
            content: m.content,
            confidence: m.confidence,
            score: 1.0,
            fact_type: format!("{:?}", m.fact_type),
            entities: m.entity_ids,
            t_valid: m.t_valid,
            t_created: m.t_created,
            supersedes_id: m.supersedes_id,
        })
        .collect();

    tracing::debug!(total, limit, offset = req.offset, "list completed");

    Ok(Json(UserMemoriesResponse {
        memories: paged,
        total,
        limit,
        offset: req.offset,
    }))
}
