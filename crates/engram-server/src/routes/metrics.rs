//! GET /metrics — Prometheus scrape endpoint.

use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use engram_core::api::METRICS;

pub async fn metrics() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        METRICS.encode(),
    )
}
