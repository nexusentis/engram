//! MCP Streamable HTTP transport (POST /mcp, DELETE /mcp).
//!
//! Implements the MCP Streamable HTTP transport (2025-03-26 spec).
//! - POST /mcp: JSON-RPC request → JSON-RPC response
//! - DELETE /mcp: Terminate a session by Mcp-Session-Id header
//!
//! Each session gets its own McpHandler instance, keyed by a random session ID
//! returned in the `Mcp-Session-Id` response header. Idle sessions are reaped
//! after SESSION_TTL.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;

use engram_core::api::mcp::{McpHandler, McpRequest, McpResponse};

use crate::state::{AppState, McpSession};

const SESSION_HEADER: &str = "mcp-session-id";

/// POST /mcp — handle a JSON-RPC message within an MCP session.
///
/// If the request does not include an `Mcp-Session-Id` header, a new session is
/// created (the first message MUST be `initialize`). Subsequent messages must
/// include the session ID returned in the first response.
pub async fn mcp_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<McpRequest>,
) -> impl IntoResponse {
    let is_initialize = request.method == "initialize";

    // Resolve or create the session handler.
    let (session_id, handler) = if let Some(hv) = headers.get(SESSION_HEADER) {
        let sid = hv.to_str().unwrap_or("").to_string();
        let mut sessions = state.mcp_sessions.write().await;
        match sessions.get_mut(&sid) {
            Some(session) => {
                session.last_active = Instant::now();
                (sid, session.handler.clone())
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    HeaderMap::new(),
                    Json(McpResponse::error(
                        request.id.clone(),
                        -32600,
                        "Unknown session ID",
                    )),
                );
            }
        }
    } else if is_initialize {
        // Create a new session.
        let sid = uuid::Uuid::now_v7().to_string();
        let rt_handle = tokio::runtime::Handle::current();
        let mut h = McpHandler::with_backend(
            "engram",
            env!("CARGO_PKG_VERSION"),
            state.mcp_backend.clone(),
            rt_handle.clone(),
        );

        // Inject memory_answer tool if the agent is configured.
        if let Some(ref agent) = state.memory_agent {
            let agent = Arc::clone(agent);
            let handle = rt_handle;
            h = h.with_extra_tools(
                vec![AppState::memory_answer_tool_def()],
                move |_name, args| {
                    let user_id = match args.get("user_id").and_then(|v| v.as_str()) {
                        Some(id) if !id.is_empty() => id.to_string(),
                        _ => return engram_core::api::mcp::ToolResult::error(
                            "missing required parameter: user_id",
                        ),
                    };
                    let question = match args.get("question").and_then(|v| v.as_str()) {
                        Some(q) if !q.is_empty() => q.to_string(),
                        _ => return engram_core::api::mcp::ToolResult::error(
                            "missing required parameter: question",
                        ),
                    };
                    let ref_time: Option<chrono::DateTime<chrono::Utc>> = args
                        .get("reference_time")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok());

                    let agent = Arc::clone(&agent);
                    match handle.block_on(agent.answer(&question, &user_id, ref_time)) {
                        Ok(result) => engram_core::api::mcp::ToolResult::text(
                            serde_json::json!({
                                "answer": result.answer,
                                "abstained": result.abstained,
                                "strategy": format!("{:?}", result.strategy),
                                "iterations": result.iterations,
                                "total_time_ms": result.total_time_ms,
                                "fallback_used": result.fallback_used,
                            })
                            .to_string(),
                        ),
                        Err(e) => engram_core::api::mcp::ToolResult::error(
                            format!("memory_answer failed: {e}"),
                        ),
                    }
                },
            );
        }

        let handler = Arc::new(std::sync::Mutex::new(h));
        let session = McpSession {
            handler: handler.clone(),
            last_active: Instant::now(),
        };
        let mut sessions = state.mcp_sessions.write().await;
        // Opportunistically reap expired sessions while we hold the write lock.
        let ttl = Duration::from_secs(state.security.mcp_session_ttl_secs);
        sessions.retain(|_, s| s.last_active.elapsed() < ttl);
        sessions.insert(sid.clone(), session);
        (sid, handler)
    } else {
        return (
            StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            Json(McpResponse::error(
                request.id.clone(),
                -32600,
                "Missing Mcp-Session-Id header; send 'initialize' first",
            )),
        );
    };

    // JSON-RPC notifications have no `id` — fire-and-forget.
    let is_notification = request.id.is_none();

    // Clone request.id before moving request into the blocking task, so the
    // join-error path can still reference the original ID.
    let request_id = request.id.clone();
    let handler_clone = handler.clone();
    let response = tokio::task::spawn_blocking(move || {
        match handler_clone.lock() {
            Ok(mut h) => h.handle(request),
            Err(_) => McpResponse::error(request.id, -32603, "Session handler unavailable"),
        }
    })
    .await
    .unwrap_or_else(|e| {
        McpResponse::error(request_id, -32603, format!("Handler panicked: {e}"))
    });

    let mut resp_headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&session_id) {
        resp_headers.insert(SESSION_HEADER, v);
    }

    // Per JSON-RPC spec, notifications expect no response body.
    if is_notification {
        (StatusCode::ACCEPTED, resp_headers, Json(response))
    } else {
        (StatusCode::OK, resp_headers, Json(response))
    }
}

/// DELETE /mcp — terminate an MCP session.
pub async fn mcp_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let sid = headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if sid.is_empty() {
        return StatusCode::BAD_REQUEST;
    }

    let removed = state.mcp_sessions.write().await.remove(sid).is_some();
    if removed {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}
