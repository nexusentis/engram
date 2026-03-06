//! POST /v1/memories

use axum::extract::State;
use axum::Json;
use std::time::Instant;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::{ErrorResponse, IngestRequest, IngestResponse, METRICS};
use engram_core::extraction::{Conversation, ConversationTurn};

#[utoipa::path(
    post,
    path = "/v1/memories",
    request_body = IngestRequest,
    responses(
        (status = 200, description = "Memories ingested", body = IngestResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
    ),
    tag = "memories"
)]
pub async fn ingest_memories(
    State(state): State<AppState>,
    Json(req): Json<IngestRequest>,
) -> AppResult<Json<IngestResponse>> {
    let start = Instant::now();

    if req.user_id.trim().is_empty() {
        return Err(AppError::BadRequest("user_id is required".into()));
    }
    if req.user_id.len() > 256 {
        return Err(AppError::BadRequest("user_id exceeds 256 characters".into()));
    }
    if req.messages.is_empty() {
        return Err(AppError::BadRequest("messages must not be empty".into()));
    }

    // Convert API messages to Conversation, preserving timestamps
    let turns: Vec<ConversationTurn> = req
        .messages
        .iter()
        .map(|m| {
            let turn = match m.role.as_str() {
                "assistant" => ConversationTurn::assistant(&m.content),
                "system" => ConversationTurn::system(&m.content),
                _ => ConversationTurn::user(&m.content),
            };
            match m.timestamp {
                Some(ts) => turn.with_timestamp(ts),
                None => turn,
            }
        })
        .collect();

    let mut conversation = Conversation::new(&req.user_id, turns);
    if let Some(sid) = &req.session_id {
        conversation = conversation.with_session(sid);
    }

    let messages_count = req.messages.len();

    // Delegate to MemorySystem — extracts facts, stores memories + raw messages
    let result = state
        .mcp_backend
        .ingest(conversation)
        .await
        .map_err(|e| AppError::Internal(format!("Ingestion failed: {e}")))?;

    METRICS.record_ingestion(
        result.memory_ids.len() as u64,
        result.facts_extracted as u64,
        start.elapsed().as_secs_f64(),
    );

    tracing::info!(
        messages_count,
        facts_extracted = result.facts_extracted,
        entities_count = result.entities_found.len(),
        messages_stored = result.messages_stored,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "ingest completed"
    );

    Ok(Json(IngestResponse {
        memory_ids: result.memory_ids,
        facts_extracted: result.facts_extracted,
        entities_found: result.entities_found,
        processing_time_ms: start.elapsed().as_millis() as u64,
    }))
}
