//! Batch mode methods for SessionIngester.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use qdrant_client::qdrant::{value::Kind, PointStruct, Value};
use uuid::Uuid;

use crate::types::BenchmarkSession;
use engram::embedding::EmbeddingProvider;
use crate::error::{BenchmarkError, Result};
use engram::extraction::{
    ApiExtractorConfig, BatchClient, BatchExtractor, BatchStatus, ConversationTurn, Conversation,
    Role,
};
use engram::types::Memory;

use super::config::BatchPollResult;
use super::SessionIngester;

impl SessionIngester {
    /// Generate JSONL batch file for pending sessions (batch mode step 1)
    ///
    /// Returns the number of sessions written to the file.
    pub fn generate_batch_file(
        &self,
        sessions: &[BenchmarkSession],
        output_path: &Path,
        model: &str,
    ) -> Result<usize> {
        let batch_extractor = BatchExtractor::new(ApiExtractorConfig::openai(model));

        let mut file = std::fs::File::create(output_path).map_err(|e| {
            BenchmarkError::Ingestion(format!("Failed to create batch file: {}", e))
        })?;

        let mut count = 0;
        for session in sessions {
            // Convert session to conversation
            let turns: Vec<ConversationTurn> = session
                .messages
                .iter()
                .map(|m| ConversationTurn {
                    role: match m.role.to_lowercase().as_str() {
                        "user" => Role::User,
                        "assistant" => Role::Assistant,
                        _ => Role::System,
                    },
                    content: m.content.clone(),
                    timestamp: Some(m.timestamp),
                })
                .collect();

            let conversation = Conversation::new(&session.user_id, turns);

            // Generate JSONL line
            let jsonl_line = batch_extractor.create_request(&session.session_id, &conversation);
            writeln!(file, "{}", jsonl_line).map_err(|e| {
                BenchmarkError::Ingestion(format!("Failed to write to batch file: {}", e))
            })?;

            count += 1;
        }

        tracing::info!(
            "Generated batch file with {} requests at {:?}",
            count,
            output_path
        );
        Ok(count)
    }

    /// Submit batch file to OpenAI (batch mode step 2)
    ///
    /// Returns the batch_id for polling.
    pub async fn submit_batch(&self, jsonl_path: &Path) -> Result<String> {
        let content = std::fs::read_to_string(jsonl_path)
            .map_err(|e| BenchmarkError::Ingestion(format!("Failed to read batch file: {}", e)))?;

        let client = BatchClient::from_env()
            .ok_or_else(|| BenchmarkError::Ingestion("OPENAI_API_KEY not set".into()))?;

        // Upload file
        tracing::info!("Uploading batch file ({} bytes)...", content.len());
        let file_id = client.upload_file(&content).await.map_err(|e| {
            BenchmarkError::Ingestion(format!("Failed to upload batch file: {}", e))
        })?;
        tracing::info!("Uploaded file: {}", file_id);

        // Submit batch
        tracing::info!("Submitting batch job...");
        let batch_id = client
            .submit_batch(&file_id)
            .await
            .map_err(|e| BenchmarkError::Ingestion(format!("Failed to submit batch: {}", e)))?;
        tracing::info!("Submitted batch: {}", batch_id);

        Ok(batch_id)
    }

    /// Poll batch status and process results when complete (batch mode step 3)
    ///
    /// If the batch is still in progress, returns `BatchPollResult::InProgress`.
    /// If complete, downloads results and processes them into the storage.
    pub async fn poll_and_process_batch(
        &self,
        batch_id: &str,
        session_lookup: &HashMap<String, BenchmarkSession>,
        model: &str,
    ) -> Result<BatchPollResult> {
        let client = BatchClient::from_env()
            .ok_or_else(|| BenchmarkError::Ingestion("OPENAI_API_KEY not set".into()))?;

        // Check batch status
        let poll_result = client
            .get_batch_status(batch_id)
            .await
            .map_err(|e| BenchmarkError::Ingestion(format!("Failed to get batch status: {}", e)))?;

        match poll_result.status {
            BatchStatus::Validating => Ok(BatchPollResult::InProgress {
                completed: 0,
                failed: 0,
                total: 0,
            }),
            BatchStatus::InProgress {
                completed,
                failed,
                total,
            } => Ok(BatchPollResult::InProgress {
                completed,
                failed,
                total,
            }),
            BatchStatus::Completed {
                output_file_id,
                error_file_id,
            } => {
                tracing::info!(
                    "Batch completed! Downloading results from {}...",
                    output_file_id
                );

                // Download and process results
                let results_content = client.get_results(&output_file_id).await.map_err(|e| {
                    BenchmarkError::Ingestion(format!("Failed to download results: {}", e))
                })?;

                // Download error file if present
                if let Some(error_file_id) = error_file_id {
                    match client.get_results(&error_file_id).await {
                        Ok(error_content) => {
                            tracing::warn!("Batch had errors:\n{}", error_content);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to download error file: {}", e);
                        }
                    }
                }

                // Process results
                let process_result = self
                    .process_batch_results(&results_content, session_lookup, model)
                    .await?;
                Ok(process_result)
            }
            BatchStatus::Failed { error } => Ok(BatchPollResult::Failed { error }),
            BatchStatus::Expired => Ok(BatchPollResult::Failed {
                error: "Batch expired (not completed within 24 hours)".to_string(),
            }),
            BatchStatus::Cancelled => Ok(BatchPollResult::Failed {
                error: "Batch was cancelled".to_string(),
            }),
        }
    }

    /// Process batch results and store facts in Qdrant
    async fn process_batch_results(
        &self,
        results_content: &str,
        session_lookup: &HashMap<String, BenchmarkSession>,
        model: &str,
    ) -> Result<BatchPollResult> {
        let batch_extractor = BatchExtractor::new(ApiExtractorConfig::openai(model));

        let embedding_provider = self
            .embedding_provider
            .as_ref()
            .ok_or_else(|| BenchmarkError::Ingestion("Embedding provider not configured".into()))?;

        let storage = self
            .storage
            .as_ref()
            .ok_or_else(|| BenchmarkError::Ingestion("Storage not configured".into()))?;

        let mut sessions_processed = 0;
        let mut facts_extracted = 0;
        let mut errors = Vec::new();

        for line in results_content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            match batch_extractor.parse_result(line) {
                Ok((session_id, facts)) => {
                    // Look up the session
                    let session = match session_lookup.get(&session_id) {
                        Some(s) => s,
                        None => {
                            errors.push(format!("Session {} not found in lookup", session_id));
                            continue;
                        }
                    };

                    // Get session date for t_valid
                    let session_date = session.earliest_timestamp();

                    // Store facts
                    for fact in &facts {
                        // Create memory
                        let mut memory = Memory::new(&session.user_id, &fact.content)
                            .with_epistemic_type(fact.epistemic_type.clone())
                            .with_fact_type(fact.fact_type.clone())
                            .with_session(session.session_id.clone());
                        if let Some(date) = session_date {
                            memory = memory.with_valid_time(date);
                        }

                        // Generate embedding
                        match embedding_provider.embed_document(&fact.content).await {
                            Ok(embedding) => {
                                // Store in Qdrant
                                if let Err(e) = storage.upsert_memory(&memory, embedding).await {
                                    errors.push(format!(
                                        "Failed to store memory for session {}: {}",
                                        session_id, e
                                    ));
                                }
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "Failed to embed fact for session {}: {}",
                                    session_id, e
                                ));
                            }
                        }
                    }

                    facts_extracted += facts.len();

                    // ---- Raw Message Storage (Epic 003) ----
                    // Store raw conversation turns for this session
                    if !self.config.skip_messages {
                    let peer_name = session
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("peer_name"))
                        .and_then(|v| v.as_str());

                    let valid_msg_indices: Vec<usize> = session
                        .messages
                        .iter()
                        .enumerate()
                        .filter(|(_, msg)| !msg.content.trim().is_empty())
                        .map(|(i, _)| i)
                        .collect();
                    let turn_texts: Vec<String> = valid_msg_indices
                        .iter()
                        .map(|&i| session.messages[i].content.clone())
                        .collect();

                    if !turn_texts.is_empty() {
                        const EMBED_BATCH_SIZE: usize = 50;
                        let mut all_embeddings = Vec::with_capacity(turn_texts.len());
                        let mut embed_ok = true;

                        for chunk in turn_texts.chunks(EMBED_BATCH_SIZE) {
                            match embedding_provider.embed_batch(chunk).await {
                                Ok(embs) => all_embeddings.extend(embs),
                                Err(e) => {
                                    errors.push(format!(
                                        "Failed to embed messages for session {}: {}",
                                        session_id, e
                                    ));
                                    embed_ok = false;
                                    break;
                                }
                            }
                        }

                        if embed_ok {
                            let points: Vec<PointStruct> = valid_msg_indices
                                .iter()
                                .zip(all_embeddings.into_iter())
                                .map(|(&msg_idx, embedding)| {
                                    let msg = &session.messages[msg_idx];
                                    let id = Uuid::now_v7().to_string();
                                    let mut payload = std::collections::HashMap::new();
                                    payload.insert(
                                        "content".to_string(),
                                        Value {
                                            kind: Some(Kind::StringValue(msg.content.clone())),
                                        },
                                    );
                                    payload.insert(
                                        "session_id".to_string(),
                                        Value {
                                            kind: Some(Kind::StringValue(
                                                session.session_id.clone(),
                                            )),
                                        },
                                    );
                                    payload.insert(
                                        "turn_index".to_string(),
                                        Value { kind: Some(Kind::IntegerValue(msg_idx as i64)) },
                                    );
                                    payload.insert(
                                        "role".to_string(),
                                        Value { kind: Some(Kind::StringValue(msg.role.clone())) },
                                    );
                                    payload.insert(
                                        "t_valid".to_string(),
                                        Value { kind: Some(Kind::StringValue(msg.timestamp.to_rfc3339())) },
                                    );
                                    payload.insert(
                                        "user_id".to_string(),
                                        Value { kind: Some(Kind::StringValue(session.user_id.clone())) },
                                    );
                                    if let Some(name) = peer_name {
                                        payload.insert(
                                            "peer_name".to_string(),
                                            Value { kind: Some(Kind::StringValue(name.to_string())) },
                                        );
                                    }
                                    PointStruct::new(id, embedding, payload)
                                })
                                .collect();

                            if let Err(e) = storage.upsert_messages_batch(points).await {
                                errors.push(format!(
                                    "Failed to store messages for session {}: {}",
                                    session_id, e
                                ));
                            }
                        }
                    }
                    } // end skip_messages guard

                    sessions_processed += 1;
                }
                Err(e) => {
                    errors.push(format!("Failed to parse result line: {}", e));
                }
            }
        }

        tracing::info!(
            "Processed batch: {} sessions, {} facts, {} errors",
            sessions_processed,
            facts_extracted,
            errors.len()
        );

        Ok(BatchPollResult::Completed {
            sessions_processed,
            facts_extracted,
            errors,
        })
    }

    /// Get session IDs from JSONL batch file
    ///
    /// Useful for creating the session lookup map.
    pub fn get_batch_session_ids(jsonl_path: &Path) -> Result<Vec<String>> {
        let content = std::fs::read_to_string(jsonl_path)
            .map_err(|e| BenchmarkError::Ingestion(format!("Failed to read batch file: {}", e)))?;

        let mut session_ids = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            #[derive(serde::Deserialize)]
            struct BatchLine {
                custom_id: String,
            }

            match serde_json::from_str::<BatchLine>(line) {
                Ok(batch_line) => {
                    session_ids.push(batch_line.custom_id);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse batch line: {}", e);
                }
            }
        }

        Ok(session_ids)
    }
}
