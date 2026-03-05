//! Integration tests requiring a live Qdrant instance.
//!
//! Gated by `ENGRAM_TEST_QDRANT_URL`. Set it to run these tests:
//!
//!     ENGRAM_TEST_QDRANT_URL=http://localhost:6334 cargo test --package engram-server
//!
//! In CI, Qdrant is provided via a service container.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;

use engram_core::api::AuthConfig;
use engram_core::config::SecurityConfig;
use engram_core::extraction::{ApiExtractor, ApiExtractorConfig};
use engram_core::storage::QdrantStorage;
use engram_core::{EmbeddingProvider, MemorySystem};

use engram_server::state::AppState;

use common::{
    delete_with_headers, get, post_json, post_json_with_headers, MockEmbedder,
};

/// Return the Qdrant URL if the env var is set, or skip the test.
macro_rules! require_qdrant {
    () => {
        match std::env::var("ENGRAM_TEST_QDRANT_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("Skipping: ENGRAM_TEST_QDRANT_URL not set");
                return;
            }
        }
    };
}

/// Build an AppState backed by real Qdrant + MockEmbedder.
async fn live_app_state(qdrant_url: &str) -> AppState {
    let qdrant_config = engram_core::QdrantConfig::external(qdrant_url);
    let qdrant = Arc::new(
        QdrantStorage::new(qdrant_config)
            .await
            .expect("Qdrant should connect"),
    );

    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbedder::new(1536));

    let extractor_config = ApiExtractorConfig {
        api_key: Some("dummy-key".to_string()),
        ..ApiExtractorConfig::default()
    };
    let extractor = Arc::new(ApiExtractor::new(extractor_config));

    let memory_system = MemorySystem::new(qdrant.clone(), embedder.clone(), extractor.clone());

    AppState {
        qdrant,
        embedder,
        extractor,
        auth_config: AuthConfig::disabled(),
        security: SecurityConfig::default(),
        mcp_backend: Arc::new(memory_system),
        mcp_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
    }
}

// ===========================================================================
// Health
// ===========================================================================

#[tokio::test]
async fn health_check_ok() {
    let url = require_qdrant!();
    let state = live_app_state(&url).await;
    let router = common::build_router(state);
    let (status, _, body) = get(&router, "/health").await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["status"], "healthy");
}

// ===========================================================================
// Search: empty results
// ===========================================================================

#[tokio::test]
async fn search_empty_results() {
    let url = require_qdrant!();
    let state = live_app_state(&url).await;
    let router = common::build_router(state);
    let (status, body) = post_json(
        &router,
        "/v1/memories/search",
        json!({
            "user_id": "nonexistent-user-12345",
            "query": "something that does not exist",
            "limit": 10
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp["total_found"], 0);
}

// ===========================================================================
// Get/Delete memory not found
// ===========================================================================

#[tokio::test]
async fn get_memory_not_found() {
    let url = require_qdrant!();
    let state = live_app_state(&url).await;
    let router = common::build_router(state);
    let random_id = uuid::Uuid::new_v4();
    let (status, _, _) = get(
        &router,
        &format!("/v1/memories/{random_id}?user_id=test-user"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_memory_not_found() {
    let url = require_qdrant!();
    let state = live_app_state(&url).await;
    let router = common::build_router(state);
    let random_id = uuid::Uuid::new_v4();
    let (status, _, _) = delete_with_headers(
        &router,
        &format!("/v1/memories/{random_id}?user_id=test-user"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// MCP session lifecycle
// ===========================================================================

#[tokio::test]
async fn mcp_initialize_and_tools_list() {
    let url = require_qdrant!();
    let state = live_app_state(&url).await;
    let router = common::build_router(state);

    // Initialize
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
    let session_id = headers
        .get("mcp-session-id")
        .expect("session ID in response")
        .to_str()
        .unwrap()
        .to_string();

    // tools/list
    let (status, _, body) = post_json_with_headers(
        &router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
        vec![("mcp-session-id", &session_id)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(resp.get("result").is_some(), "Expected result in tools/list response");
}

#[tokio::test]
async fn mcp_session_lifecycle() {
    let url = require_qdrant!();
    let state = live_app_state(&url).await;
    let router = common::build_router(state);

    // 1. Initialize
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
    let session_id = headers
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // 2. Use the session (tools/list)
    let (status, _, _) = post_json_with_headers(
        &router,
        "/mcp",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        vec![("mcp-session-id", &session_id)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 3. Delete the session
    let (status, _, _) = delete_with_headers(
        &router,
        "/mcp",
        vec![("mcp-session-id", &session_id)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 4. Session is gone → 404
    let (status, _, _) = delete_with_headers(
        &router,
        "/mcp",
        vec![("mcp-session-id", &session_id)],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
