//! REST route definitions.

mod delete;
mod get_memory;
mod health;
mod ingest;
mod list;
mod messages;
mod metrics;
pub mod mcp_http;
mod search;

use axum::extract::Request;
use axum::http::{header, HeaderValue, Method};
use axum::middleware as axum_mw;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::time::Duration;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;

use axum::extract::DefaultBodyLimit;

use crate::middleware::auth::AuthLayer;
use crate::middleware::request_metrics::request_metrics;
use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    paths(
        health::health_check,
        ingest::ingest_memories,
        search::search_memories,
        get_memory::get_memory,
        delete::delete_memory,
        list::list_memories,
        messages::search_messages,
    ),
    components(schemas(
        engram_core::api::IngestRequest,
        engram_core::api::MessageInput,
        engram_core::api::IngestResponse,
        engram_core::api::SearchRequest,
        engram_core::api::SearchFilters,
        engram_core::api::TimeRange,
        engram_core::api::SearchResponse,
        engram_core::api::MemoryResult,
        engram_core::api::MemoryResponse,
        engram_core::api::MemoryDetail,
        engram_core::api::MemoryVersion,
        engram_core::api::DeleteResponse,
        engram_core::api::UserMemoriesRequest,
        engram_core::api::UserMemoriesResponse,
        engram_core::api::HealthResponse,
        engram_core::api::ErrorResponse,
        messages::MessagesSearchRequest,
        messages::MessagesSearchResponse,
        messages::MessageHit,
    )),
    tags(
        (name = "memories", description = "Memory CRUD and search"),
        (name = "messages", description = "Raw message search"),
        (name = "operations", description = "Health and metrics"),
    ),
    info(
        title = "Engram Memory API",
        version = "0.1.0",
        description = "REST API for the Engram AI agent memory system",
    )
)]
struct ApiDoc;

async fn openapi_spec() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

/// Build the application router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    let auth_layer = AuthLayer::new(state.auth_config.clone());
    let body_limit = state.security.body_limit_bytes;
    let timeout_secs = state.security.request_timeout_secs;
    let cors_layer = build_cors_layer(&state.security.cors_origins);

    // Layer ordering (last added = outermost):
    // Request → request_id → security_headers → cors → trace → metrics → auth → timeout → body_limit → compression → handler
    Router::new()
        // Unauthenticated
        .route("/health", get(health::health_check))
        .route("/metrics", get(metrics::metrics))
        .route("/openapi.json", get(openapi_spec))
        .route("/mcp", post(mcp_http::mcp_post).delete(mcp_http::mcp_delete))
        // Authenticated
        .route(
            "/v1/memories",
            get(list::list_memories).post(ingest::ingest_memories),
        )
        .route("/v1/memories/search", post(search::search_memories))
        .route(
            "/v1/memories/:id",
            get(get_memory::get_memory).delete(delete::delete_memory),
        )
        .route("/v1/messages/search", post(messages::search_messages))
        .layer(CompressionLayer::new())
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TimeoutLayer::new(Duration::from_secs(timeout_secs)))
        .layer(auth_layer)
        .layer(axum_mw::from_fn(request_metrics))
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer)
        .layer(axum_mw::from_fn(security_headers))
        .layer(axum_mw::from_fn(crate::middleware::request_id::request_id))
        .with_state(state)
}

async fn security_headers(req: Request, next: axum_mw::Next) -> Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    resp
}

fn build_cors_layer(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        CorsLayer::new() // same-origin only
    } else if origins.iter().any(|o| o == "*") {
        CorsLayer::permissive()
    } else {
        let mcp_session_id: axum::http::HeaderName = "mcp-session-id".parse().unwrap();
        let x_request_id: axum::http::HeaderName = "x-request-id".parse().unwrap();
        let parsed: Vec<_> = origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(parsed)
            .allow_methods([Method::GET, Method::POST, Method::DELETE])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, mcp_session_id.clone(), x_request_id.clone()])
            .expose_headers([mcp_session_id, x_request_id])
    }
}
