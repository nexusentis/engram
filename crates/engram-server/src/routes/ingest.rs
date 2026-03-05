//! POST /v1/memories

use axum::extract::State;
use axum::Json;
use std::time::Instant;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use engram_core::api::{ErrorResponse, IngestRequest, IngestResponse, METRICS};
use engram_core::extraction::{Conversation, ConversationTurn};
use engram_core::Memory;

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

    // Extract facts
    let (facts, _registry) = state
        .extractor
        .extract_with_context(&conversation)
        .await
        .map_err(|e| AppError::Internal(format!("Extraction failed: {e}")))?;

    let mut memory_ids = Vec::with_capacity(facts.len());
    let mut entities_found = Vec::new();

    for fact in &facts {
        let entity_ids: Vec<String> =
            fact.entities.iter().map(|e| e.normalized_id.clone()).collect();
        let entity_names: Vec<String> = fact.entities.iter().map(|e| e.name.clone()).collect();
        let entity_types: Vec<String> =
            fact.entities.iter().map(|e| e.entity_type.clone()).collect();

        // Build Memory from ExtractedFact
        let mut memory = Memory::new(&req.user_id, &fact.content);
        memory.confidence = fact.confidence;
        memory.fact_type = fact.fact_type;
        memory.epistemic_type = fact.epistemic_type;
        memory.source_type = fact.source_type;
        memory.entity_ids = entity_ids;
        memory.entity_names = entity_names.clone();
        memory.entity_types = entity_types;
        memory.observation_level = fact.observation_level.clone();
        if let Some(t_valid) = fact.t_valid {
            memory.t_valid = t_valid;
        }
        if let Some(sid) = &req.session_id {
            memory.session_id = Some(sid.clone());
        }

        // Embed and store
        let vector = state
            .embedder
            .embed_document(&fact.content)
            .await
            .map_err(|e| AppError::Internal(format!("Embedding failed: {e}")))?;

        state.qdrant.upsert_memory(&memory, vector).await?;

        memory_ids.push(memory.id);
        entities_found.extend(entity_names);
    }

    entities_found.sort();
    entities_found.dedup();

    let facts_extracted = facts.len();

    METRICS.record_ingestion(
        memory_ids.len() as u64,
        facts_extracted as u64,
        start.elapsed().as_secs_f64(),
    );

    tracing::info!(
        messages_count = req.messages.len(),
        facts_extracted,
        entities_count = entities_found.len(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        "ingest completed"
    );

    Ok(Json(IngestResponse {
        memory_ids,
        facts_extracted,
        entities_found,
        processing_time_ms: start.elapsed().as_millis() as u64,
    }))
}
