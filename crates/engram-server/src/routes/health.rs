//! GET /health

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use crate::error::AppResult;
use crate::state::AppState;
use engram_core::api::HealthResponse;

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Healthy", body = HealthResponse),
        (status = 503, description = "Degraded", body = HealthResponse),
    ),
    tag = "operations"
)]
pub async fn health_check(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let qdrant_ok = state
        .qdrant
        .health_check()
        .await
        .unwrap_or(false);

    if qdrant_ok {
        Ok((StatusCode::OK, Json(HealthResponse::healthy())))
    } else {
        Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse::degraded(qdrant_ok)),
        ))
    }
}
