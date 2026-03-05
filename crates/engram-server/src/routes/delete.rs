//! DELETE /v1/memories/:id

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::{DeleteResponse, ErrorResponse};

#[derive(Deserialize, IntoParams)]
pub struct DeleteQuery {
    /// User ID for tenant isolation
    pub user_id: String,
}

#[utoipa::path(
    delete,
    path = "/v1/memories/{id}",
    params(DeleteQuery),
    responses(
        (status = 200, description = "Memory deleted", body = DeleteResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
    ),
    tag = "memories"
)]
pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<DeleteQuery>,
) -> AppResult<Json<DeleteResponse>> {
    if query.user_id.trim().is_empty() {
        return Err(AppError::BadRequest("user_id is required".into()));
    }
    if query.user_id.len() > 256 {
        return Err(AppError::BadRequest("user_id exceeds 256 characters".into()));
    }

    let deleted = state.qdrant.delete_memory(&query.user_id, id).await?;

    if !deleted {
        return Err(AppError::NotFound(format!("Memory {id} not found")));
    }

    tracing::info!(memory_id = %id, "memory deleted");

    Ok(Json(DeleteResponse {
        id,
        deleted: true,
        deleted_at: chrono::Utc::now(),
    }))
}
