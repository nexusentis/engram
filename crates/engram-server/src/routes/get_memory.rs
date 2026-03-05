//! GET /v1/memories/:id

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::{ErrorResponse, MemoryDetail, MemoryResponse};

#[derive(Deserialize, IntoParams)]
pub struct GetMemoryQuery {
    /// User ID to scope the lookup
    pub user_id: String,
}

#[utoipa::path(
    get,
    path = "/v1/memories/{id}",
    params(GetMemoryQuery),
    responses(
        (status = 200, description = "Memory found", body = MemoryResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
    ),
    tag = "memories"
)]
pub async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<GetMemoryQuery>,
) -> AppResult<Json<MemoryResponse>> {
    if query.user_id.trim().is_empty() {
        return Err(AppError::BadRequest("user_id is required".into()));
    }
    if query.user_id.len() > 256 {
        return Err(AppError::BadRequest("user_id exceeds 256 characters".into()));
    }

    tracing::debug!(memory_id = %id, "get_memory");

    let memory = state
        .qdrant
        .get_memory(&query.user_id, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Memory {id} not found")))?;

    let detail = MemoryDetail {
        id: memory.id,
        user_id: memory.user_id,
        content: memory.content,
        confidence: memory.confidence,
        source_type: format!("{:?}", memory.source_type),
        fact_type: format!("{:?}", memory.fact_type),
        epistemic_type: format!("{:?}", memory.epistemic_type),
        entities: memory.entity_ids,
        t_valid: memory.t_valid,
        t_created: memory.t_created,
        t_expired: memory.t_expired,
        supersedes_id: memory.supersedes_id,
        derived_from_ids: memory.derived_from_ids,
        is_latest: memory.is_latest,
    };

    Ok(Json(MemoryResponse {
        memory: detail,
        history: None,
    }))
}
