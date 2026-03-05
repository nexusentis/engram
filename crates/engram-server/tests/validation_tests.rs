//! Offline validation, middleware, auth, and MCP transport tests.
//!
//! These tests run without Qdrant, network, or API keys. They exercise
//! input validation (which fires before any storage call) and middleware.

mod common;

use axum::http::StatusCode;
use engram_core::api::AuthConfig;
use serde_json::json;

use common::{
    build_router, delete_with_headers, dummy_app_state, dummy_app_state_with_auth, get,
    get_with_headers, post_json, post_json_with_headers,
};

// ===========================================================================
// Search validation (/v1/memories/search)
// ===========================================================================

#[tokio::test]
async fn search_empty_user_id() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({"user_id": "", "query": "hello", "limit": 10}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_user_id_too_long() {
    let router = build_router(dummy_app_state());
    let long = "a".repeat(257);
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({"user_id": long, "query": "hello", "limit": 10}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_query_too_long() {
    let router = build_router(dummy_app_state());
    let long = "a".repeat(10_001);
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({"user_id": "user1", "query": long, "limit": 10}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_limit_zero() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({"user_id": "user1", "query": "hello", "limit": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_invalid_confidence() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({
            "user_id": "user1",
            "query": "hello",
            "limit": 10,
            "filters": {"min_confidence": 1.5}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_time_range_inverted() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({
            "user_id": "user1",
            "query": "hello",
            "limit": 10,
            "filters": {
                "time_range": {
                    "start": "2025-12-31T00:00:00Z",
                    "end": "2025-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Ingest validation (/v1/memories POST)
// ===========================================================================

#[tokio::test]
async fn ingest_empty_user_id() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories",
        json!({
            "user_id": "",
            "messages": [{"role": "user", "content": "hi"}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ingest_user_id_too_long() {
    let router = build_router(dummy_app_state());
    let long = "a".repeat(257);
    let (status, _) = post_json(
        &router,
        "/v1/memories",
        json!({
            "user_id": long,
            "messages": [{"role": "user", "content": "hi"}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ingest_empty_messages() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories",
        json!({"user_id": "user1", "messages": []}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// List validation (/v1/memories GET)
// ===========================================================================

#[tokio::test]
async fn list_empty_user_id() {
    let router = build_router(dummy_app_state());
    let (status, _, _) = get(&router, "/v1/memories?user_id=").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_user_id_too_long() {
    let router = build_router(dummy_app_state());
    let long = "a".repeat(257);
    let (status, _, _) = get(&router, &format!("/v1/memories?user_id={long}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_limit_zero() {
    let router = build_router(dummy_app_state());
    let (status, _, _) = get(&router, "/v1/memories?user_id=u1&limit=0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Messages search validation (/v1/messages/search)
// ===========================================================================

#[tokio::test]
async fn messages_empty_user_id() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/messages/search",
        json!({"user_id": "", "query": "hello"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn messages_query_too_long() {
    let router = build_router(dummy_app_state());
    let long = "a".repeat(10_001);
    let (status, _) = post_json(
        &router,
        "/v1/messages/search",
        json!({"user_id": "u1", "query": long}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn messages_limit_zero() {
    let router = build_router(dummy_app_state());
    let (status, _) = post_json(
        &router,
        "/v1/messages/search",
        json!({"user_id": "u1", "query": "hello", "limit": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Get memory / Delete memory validation
// ===========================================================================

#[tokio::test]
async fn get_memory_whitespace_user_id() {
    let router = build_router(dummy_app_state());
    let id = uuid::Uuid::new_v4();
    // Whitespace-only user_id is caught by trim().is_empty()
    let (status, _, _) = get(&router, &format!("/v1/memories/{id}?user_id=%20%20")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_whitespace_user_id() {
    let router = build_router(dummy_app_state());
    let id = uuid::Uuid::new_v4();
    let (status, _, _) = delete_with_headers(
        &router,
        &format!("/v1/memories/{id}?user_id=%20%20"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Middleware: request ID
// ===========================================================================

#[tokio::test]
async fn request_id_generated() {
    let router = build_router(dummy_app_state());
    let (_, headers, _) = get(&router, "/openapi.json").await;
    assert!(headers.contains_key("x-request-id"));
}

#[tokio::test]
async fn request_id_propagated() {
    let router = build_router(dummy_app_state());
    let (_, headers, _) =
        get_with_headers(&router, "/openapi.json", vec![("x-request-id", "test-123")]).await;
    assert_eq!(
        headers.get("x-request-id").unwrap().to_str().unwrap(),
        "test-123"
    );
}

#[tokio::test]
async fn request_id_oversized_rejected() {
    let router = build_router(dummy_app_state());
    let long_id = "x".repeat(129);
    let (_, headers, _) =
        get_with_headers(&router, "/openapi.json", vec![("x-request-id", &long_id)]).await;
    let rid = headers.get("x-request-id").unwrap().to_str().unwrap();
    // Should generate a new UUID, not echo back the oversized one
    assert_ne!(rid, long_id);
}

#[tokio::test]
async fn request_id_with_space_rejected() {
    let router = build_router(dummy_app_state());
    // Space is not ASCII graphic, so the middleware generates a new ID
    let bad_id = "bad id";
    let (_, headers, _) =
        get_with_headers(&router, "/openapi.json", vec![("x-request-id", bad_id)]).await;
    let rid = headers.get("x-request-id").unwrap().to_str().unwrap();
    assert_ne!(rid, bad_id);
}

// ===========================================================================
// Middleware: security headers
// ===========================================================================

#[tokio::test]
async fn security_headers_present() {
    let router = build_router(dummy_app_state());
    let (_, headers, _) = get(&router, "/openapi.json").await;
    assert_eq!(
        headers.get("x-content-type-options").unwrap().to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(
        headers.get("x-frame-options").unwrap().to_str().unwrap(),
        "DENY"
    );
}

// ===========================================================================
// OpenAPI spec
// ===========================================================================

#[tokio::test]
async fn openapi_spec_valid() {
    let router = build_router(dummy_app_state());
    let (status, _, body) = get(&router, "/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let spec: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert!(spec.get("openapi").is_some());
}

// ===========================================================================
// Auth middleware
// ===========================================================================

fn auth_enabled_state() -> engram_server::state::AppState {
    let token_hash = engram_core::api::hash_token("test-token");
    dummy_app_state_with_auth(AuthConfig::enabled(vec![token_hash]))
}

#[tokio::test]
async fn auth_required_no_token() {
    let router = build_router(auth_enabled_state());
    let (status, _) = post_json(
        &router,
        "/v1/memories/search",
        json!({"user_id": "u1", "query": "hello", "limit": 10}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_skip_health() {
    let router = build_router(auth_enabled_state());
    let (status, _, _) = get(&router, "/health").await;
    // Health always accessible — may return 503 (no real Qdrant) but NOT 401
    assert_ne!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_skip_metrics() {
    let router = build_router(auth_enabled_state());
    let (status, _, _) = get(&router, "/metrics").await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_skip_openapi() {
    let router = build_router(auth_enabled_state());
    let (status, _, _) = get(&router, "/openapi.json").await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
}

// ===========================================================================
// MCP transport
// ===========================================================================

#[tokio::test]
async fn mcp_missing_session_header() {
    let router = build_router(dummy_app_state());
    // Non-initialize request without session header → 400
    let (status, _, _) = post_json_with_headers(
        &router,
        "/mcp",
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mcp_delete_missing_header() {
    let router = build_router(dummy_app_state());
    let (status, _, _) = delete_with_headers(&router, "/mcp", vec![]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mcp_delete_unknown_session() {
    let router = build_router(dummy_app_state());
    let (status, _, _) = delete_with_headers(
        &router,
        "/mcp",
        vec![("mcp-session-id", "nonexistent-session")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_initialize() {
    let router = build_router(dummy_app_state());
    let (status, headers, _) = post_json_with_headers(
        &router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0.1"}
            }
        }),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.contains_key("mcp-session-id"));
}

#[tokio::test]
async fn mcp_notification_accepted() {
    let router = build_router(dummy_app_state());
    // First initialize to get a session
    let (_, headers, _) = post_json_with_headers(
        &router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0.1"}
            }
        }),
        vec![],
    )
    .await;
    let session_id = headers
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Send a notification (no "id" field) → 202
    let (status, _, _) = post_json_with_headers(
        &router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
        vec![("mcp-session-id", &session_id)],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}
