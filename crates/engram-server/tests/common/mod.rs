//! Shared test helpers for engram-server integration tests.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use engram_core::api::AuthConfig;
use engram_core::config::SecurityConfig;
use engram_core::extraction::{ApiExtractor, ApiExtractorConfig};
use engram_core::storage::{QdrantConfig, QdrantStorage};
use engram_core::{EmbeddingProvider, MemorySystem};

use engram_server::state::{AppState, McpSession};

// ---------------------------------------------------------------------------
// MockEmbedder
// ---------------------------------------------------------------------------

pub struct MockEmbedder {
    dim: usize,
}

impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> engram_core::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0; self.dim]).collect())
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        "mock"
    }
}

// ---------------------------------------------------------------------------
// AppState factories
// ---------------------------------------------------------------------------

/// Build a dummy `AppState` that compiles and passes validation checks but
/// never talks to Qdrant, OpenAI, or any external service. Suitable for
/// tests that exercise input validation and middleware (which run *before*
/// any storage call).
pub fn dummy_app_state() -> AppState {
    dummy_app_state_with_auth(AuthConfig::disabled())
}

/// Same as [`dummy_app_state`] but with a custom `AuthConfig`.
pub fn dummy_app_state_with_auth(auth_config: AuthConfig) -> AppState {
    let qdrant_config = QdrantConfig::external("http://dummy:6334");
    let qdrant = Arc::new(QdrantStorage::new_unconnected(qdrant_config));

    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbedder::new(1536));

    let extractor_config = ApiExtractorConfig {
        api_key: Some("test-key".to_string()),
        ..ApiExtractorConfig::default()
    };
    let extractor = Arc::new(ApiExtractor::new(extractor_config));

    let memory_system = MemorySystem::new(qdrant.clone(), embedder.clone(), extractor.clone());

    AppState {
        qdrant,
        embedder,
        extractor,
        auth_config,
        security: SecurityConfig::default(),
        mcp_backend: Arc::new(memory_system),
        mcp_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        memory_agent: None,
    }
}

// ---------------------------------------------------------------------------
// Request helpers
// ---------------------------------------------------------------------------

/// Build the full application router from the given state.
pub fn build_router(state: AppState) -> axum::Router {
    engram_server::routes::build_router(state)
}

/// Send a POST request with a JSON body, returning `(status, body_string)`.
#[allow(dead_code)]
pub async fn post_json(
    router: &axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// Send a POST request with a JSON body and extra headers.
#[allow(dead_code)]
pub async fn post_json_with_headers(
    router: &axum::Router,
    uri: &str,
    body: serde_json::Value,
    headers: Vec<(&str, &str)>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let req = builder
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, resp_headers, String::from_utf8_lossy(&bytes).to_string())
}

/// Send a GET request, returning `(status, headers, body_string)`.
#[allow(dead_code)]
pub async fn get(
    router: &axum::Router,
    uri: &str,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

/// Send a GET request with extra headers.
#[allow(dead_code)]
pub async fn get_with_headers(
    router: &axum::Router,
    uri: &str,
    headers: Vec<(&str, &str)>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder().uri(uri);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, resp_headers, String::from_utf8_lossy(&bytes).to_string())
}

/// Send a DELETE request with extra headers.
#[allow(dead_code)]
pub async fn delete_with_headers(
    router: &axum::Router,
    uri: &str,
    headers: Vec<(&str, &str)>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder().method("DELETE").uri(uri);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, resp_headers, String::from_utf8_lossy(&bytes).to_string())
}
