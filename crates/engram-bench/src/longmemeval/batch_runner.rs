//! Batch runner for LongMemEval benchmark
//!
//! Allows running the benchmark in chunks that can be resumed across sessions.
//! Progress is saved to disk after each batch.
//!
//! # Ingestion Modes
//!
//! - `Async`: Uses concurrent API calls (faster, full price)
//! - `BatchApi`: Uses OpenAI Batch API (slower, 50% cheaper)

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

/// Ingestion mode for the benchmark
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestionApiMode {
    /// Concurrent async API calls (faster, full price)
    #[default]
    Async,
    /// OpenAI Batch API (slower, 50% cheaper)
    BatchApi,
}

use crate::longmemeval::answerer::AnswerGenerator;
use crate::longmemeval::ingester::SessionIngester;
use crate::longmemeval::judge::Judge;
use crate::types::{BenchmarkQuestion, BenchmarkSession, QuestionCategory};
use crate::error::{BenchmarkError, Result};

/// Progress checkpoint for resumable benchmark runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCheckpoint {
    /// Unique run identifier
    pub run_id: String,
    /// When this run started
    pub started_at: DateTime<Utc>,
    /// Last update time
    pub updated_at: DateTime<Utc>,
    /// Session IDs that have been successfully ingested
    pub ingested_sessions: HashSet<String>,
    /// Question IDs that have been answered
    pub answered_questions: HashSet<String>,
    /// Results for answered questions
    pub question_results: Vec<QuestionResult>,
    /// Total sessions in dataset
    pub total_sessions: usize,
    /// Total questions in dataset
    pub total_questions: usize,
    /// Current phase
    pub phase: BenchmarkPhase,

    // Batch API tracking fields
    /// Currently pending batch job ID (if any)
    #[serde(default)]
    pub pending_batch_id: Option<String>,
    /// Session IDs included in the pending batch
    #[serde(default)]
    pub pending_batch_sessions: Vec<String>,
    /// Output file ID when batch completes (for downloading results)
    #[serde(default)]
    pub batch_output_file_id: Option<String>,
    /// Path to the generated JSONL batch file
    #[serde(default)]
    pub batch_jsonl_path: Option<String>,
}

/// Current phase of the benchmark
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BenchmarkPhase {
    /// Ingesting sessions (real-time mode)
    Ingestion,
    /// Batch ingestion: generating JSONL
    BatchGenerate,
    /// Batch ingestion: waiting for OpenAI to process
    BatchPending,
    /// Batch ingestion: processing results
    BatchProcessing,
    /// Answering questions
    Answering,
    /// Complete
    Complete,
}

/// Result for a single question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResult {
    pub question_id: String,
    pub question: String,
    pub expected: String,
    pub generated: String,
    pub is_correct: bool,
    pub score: f32,
    pub category: QuestionCategory,
    pub answered_at: DateTime<Utc>,
    /// Tool call trace from answering agent (P18.1 telemetry)
    #[serde(default)]
    pub tool_trace: Vec<super::answerer::ToolTraceEntry>,
    /// P22: Whether ensemble fallback was used
    #[serde(default)]
    pub fallback_used: bool,
    /// P22: Why fallback was triggered
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    /// P22: Model that ran first
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_model: Option<String>,
    /// P22: Model that produced the final answer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_model: Option<String>,
}

impl BenchmarkCheckpoint {
    /// Create a new checkpoint for a fresh run
    pub fn new(run_id: impl Into<String>, total_sessions: usize, total_questions: usize) -> Self {
        let now = Utc::now();
        Self {
            run_id: run_id.into(),
            started_at: now,
            updated_at: now,
            ingested_sessions: HashSet::new(),
            answered_questions: HashSet::new(),
            question_results: Vec::new(),
            total_sessions,
            total_questions,
            phase: BenchmarkPhase::Ingestion,
            // Batch API fields
            pending_batch_id: None,
            pending_batch_sessions: Vec::new(),
            batch_output_file_id: None,
            batch_jsonl_path: None,
        }
    }

    /// Load checkpoint from file, or create new if not exists
    pub fn load_or_create(
        path: &Path,
        total_sessions: usize,
        total_questions: usize,
    ) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| {
                BenchmarkError::Storage(format!("Failed to read checkpoint: {}", e))
            })?;
            let checkpoint: Self = serde_json::from_str(&content).map_err(|e| {
                BenchmarkError::Storage(format!("Failed to parse checkpoint: {}", e))
            })?;
            Ok(checkpoint)
        } else {
            let run_id = format!("run_{}", Utc::now().format("%Y%m%d_%H%M%S"));
            Ok(Self::new(run_id, total_sessions, total_questions))
        }
    }

    /// Save checkpoint to file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            BenchmarkError::Storage(format!("Failed to serialize checkpoint: {}", e))
        })?;
        fs::write(path, content)
            .map_err(|e| BenchmarkError::Storage(format!("Failed to write checkpoint: {}", e)))?;
        Ok(())
    }

    /// Mark a session as ingested
    pub fn mark_session_ingested(&mut self, session_id: &str) {
        self.ingested_sessions.insert(session_id.to_string());
        self.updated_at = Utc::now();
    }

    /// Mark a question as answered
    pub fn mark_question_answered(&mut self, result: QuestionResult) {
        self.answered_questions.insert(result.question_id.clone());
        self.question_results.push(result);
        self.updated_at = Utc::now();
    }

    /// Check if a session has been ingested
    pub fn is_session_ingested(&self, session_id: &str) -> bool {
        self.ingested_sessions.contains(session_id)
    }

    /// Check if a question has been answered
    pub fn is_question_answered(&self, question_id: &str) -> bool {
        self.answered_questions.contains(question_id)
    }

    /// Get ingestion progress as percentage
    pub fn ingestion_progress(&self) -> f32 {
        if self.total_sessions == 0 {
            100.0
        } else {
            (self.ingested_sessions.len() as f32 / self.total_sessions as f32) * 100.0
        }
    }

    /// Get answering progress as percentage
    pub fn answering_progress(&self) -> f32 {
        if self.total_questions == 0 {
            100.0
        } else {
            (self.answered_questions.len() as f32 / self.total_questions as f32) * 100.0
        }
    }

    /// Calculate current accuracy
    pub fn accuracy(&self) -> f32 {
        if self.question_results.is_empty() {
            0.0
        } else {
            let correct = self
                .question_results
                .iter()
                .filter(|r| r.is_correct)
                .count();
            correct as f32 / self.question_results.len() as f32
        }
    }

    /// Get remaining sessions to ingest
    pub fn remaining_sessions<'a>(
        &self,
        all_sessions: &'a [BenchmarkSession],
    ) -> Vec<&'a BenchmarkSession> {
        all_sessions
            .iter()
            .filter(|s| !self.is_session_ingested(&s.session_id))
            .collect()
    }

    /// Get remaining questions to answer
    pub fn remaining_questions<'a>(
        &self,
        all_questions: &'a [BenchmarkQuestion],
    ) -> Vec<&'a BenchmarkQuestion> {
        all_questions
            .iter()
            .filter(|q| !self.is_question_answered(&q.id))
            .collect()
    }

    // ==================== Batch API Methods ====================

    /// Start tracking a new batch submission
    pub fn start_batch(&mut self, batch_id: &str, session_ids: Vec<String>, jsonl_path: &str) {
        self.pending_batch_id = Some(batch_id.to_string());
        self.pending_batch_sessions = session_ids;
        self.batch_jsonl_path = Some(jsonl_path.to_string());
        self.batch_output_file_id = None;
        self.phase = BenchmarkPhase::BatchPending;
        self.updated_at = Utc::now();
    }

    /// Mark batch as completed with output file
    pub fn batch_completed(&mut self, output_file_id: &str) {
        self.batch_output_file_id = Some(output_file_id.to_string());
        self.phase = BenchmarkPhase::BatchProcessing;
        self.updated_at = Utc::now();
    }

    /// Mark batch results as processed and clear batch state
    pub fn batch_processed(&mut self, processed_session_ids: &[String]) {
        for session_id in processed_session_ids {
            self.ingested_sessions.insert(session_id.clone());
        }

        // Clear batch state
        self.pending_batch_id = None;
        self.pending_batch_sessions.clear();
        self.batch_output_file_id = None;
        self.batch_jsonl_path = None;

        // Move to next phase
        if self.ingested_sessions.len() >= self.total_sessions {
            self.phase = BenchmarkPhase::Answering;
        } else {
            // More sessions to process - go back to generate
            self.phase = BenchmarkPhase::BatchGenerate;
        }

        self.updated_at = Utc::now();
    }

    /// Check if there's a pending batch
    pub fn has_pending_batch(&self) -> bool {
        self.pending_batch_id.is_some()
    }

    /// Get the pending batch ID
    pub fn get_pending_batch_id(&self) -> Option<&str> {
        self.pending_batch_id.as_deref()
    }

    /// Get the batch JSONL path
    pub fn get_batch_jsonl_path(&self) -> Option<&str> {
        self.batch_jsonl_path.as_deref()
    }

    /// Clear batch state (e.g., if batch failed)
    pub fn clear_batch(&mut self) {
        self.pending_batch_id = None;
        self.pending_batch_sessions.clear();
        self.batch_output_file_id = None;
        self.batch_jsonl_path = None;
        self.phase = BenchmarkPhase::BatchGenerate;
        self.updated_at = Utc::now();
    }
}

/// Configuration for batch benchmark runs
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Number of sessions to ingest per batch
    pub sessions_per_batch: usize,
    /// Number of questions to answer per batch
    pub questions_per_batch: usize,
    /// Path to checkpoint file
    pub checkpoint_path: String,
    /// Whether to print verbose progress
    pub verbose: bool,
    /// Ingestion mode (async API or batch API)
    pub ingestion_mode: IngestionApiMode,
    /// Directory for batch JSONL files (only used in BatchApi mode)
    pub batch_files_dir: String,
    /// Number of questions to answer concurrently (1 = sequential)
    pub answer_concurrency: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            sessions_per_batch: 25000, // all sessions in one batch for parallel ingestion
            questions_per_batch: 100, // ~10 min
            checkpoint_path: "benchmark_checkpoint.json".to_string(),
            verbose: true,
            ingestion_mode: IngestionApiMode::Async,
            batch_files_dir: "batch_files".to_string(),
            answer_concurrency: 1, // sequential by default
        }
    }
}

impl BatchConfig {
    /// Create config for ~1 hour batches
    /// Based on: 148 sessions in 6 min = ~1500 sessions/hour
    pub fn one_hour_batches() -> Self {
        Self {
            sessions_per_batch: 1500,
            questions_per_batch: 100,
            ..Default::default()
        }
    }

    /// Create config for ~2.5 hour batches (5 batches total for full benchmark)
    /// 18,464 sessions / 5 = ~3700 sessions per batch
    pub fn five_batch_run() -> Self {
        Self {
            sessions_per_batch: 3700,
            questions_per_batch: 100,
            ..Default::default()
        }
    }

    /// Create config for ~30 min batches
    pub fn thirty_min_batches() -> Self {
        Self {
            sessions_per_batch: 750,
            questions_per_batch: 50,
            ..Default::default()
        }
    }

    /// Set checkpoint path
    pub fn with_checkpoint_path(mut self, path: impl Into<String>) -> Self {
        self.checkpoint_path = path.into();
        self
    }

    /// Set ingestion mode
    pub fn with_ingestion_mode(mut self, mode: IngestionApiMode) -> Self {
        self.ingestion_mode = mode;
        self
    }

    /// Use async API mode (faster, full price)
    pub fn async_mode(mut self) -> Self {
        self.ingestion_mode = IngestionApiMode::Async;
        self
    }

    /// Use batch API mode (slower, 50% cheaper)
    pub fn batch_api_mode(mut self) -> Self {
        self.ingestion_mode = IngestionApiMode::BatchApi;
        self
    }

    /// Set batch files directory (for BatchApi mode)
    pub fn with_batch_files_dir(mut self, dir: impl Into<String>) -> Self {
        self.batch_files_dir = dir.into();
        self
    }

    /// Set answer concurrency (number of questions answered in parallel)
    pub fn with_answer_concurrency(mut self, concurrency: usize) -> Self {
        self.answer_concurrency = concurrency.max(1);
        self
    }
}

/// Batch runner for the benchmark
pub struct BatchRunner {
    config: BatchConfig,
    checkpoint: BenchmarkCheckpoint,
}

impl BatchRunner {
    /// Create a new batch runner, loading existing checkpoint if available
    pub fn new(config: BatchConfig, total_sessions: usize, total_questions: usize) -> Result<Self> {
        let checkpoint_path = Path::new(&config.checkpoint_path);
        let checkpoint =
            BenchmarkCheckpoint::load_or_create(checkpoint_path, total_sessions, total_questions)?;

        Ok(Self { config, checkpoint })
    }

    /// Get current progress summary
    pub fn progress_summary(&self) -> String {
        format!(
            "Run: {}\n\
             Phase: {:?}\n\
             Ingestion: {}/{} ({:.1}%)\n\
             Questions: {}/{} ({:.1}%)\n\
             Current Accuracy: {:.1}%",
            self.checkpoint.run_id,
            self.checkpoint.phase,
            self.checkpoint.ingested_sessions.len(),
            self.checkpoint.total_sessions,
            self.checkpoint.ingestion_progress(),
            self.checkpoint.answered_questions.len(),
            self.checkpoint.total_questions,
            self.checkpoint.answering_progress(),
            self.checkpoint.accuracy() * 100.0,
        )
    }

    /// Run one batch of ingestion
    ///
    /// Behavior depends on `ingestion_mode`:
    /// - `Async`: Runs concurrent API calls immediately
    /// - `BatchApi`: Submits to OpenAI Batch API (50% cheaper, async processing)
    pub async fn run_ingestion_batch(
        &mut self,
        sessions: &[BenchmarkSession],
        ingester: &SessionIngester,
    ) -> Result<usize> {
        match self.config.ingestion_mode {
            IngestionApiMode::Async => self.run_ingestion_batch_async(sessions, ingester).await,
            IngestionApiMode::BatchApi => self.run_ingestion_batch_api(sessions, ingester).await,
        }
    }

    /// Run ingestion using concurrent async API calls (faster, full price)
    async fn run_ingestion_batch_async(
        &mut self,
        sessions: &[BenchmarkSession],
        ingester: &SessionIngester,
    ) -> Result<usize> {
        let remaining: Vec<_> = self.checkpoint.remaining_sessions(sessions);

        if remaining.is_empty() {
            self.checkpoint.phase = BenchmarkPhase::Answering;
            self.save_checkpoint()?;
            return Ok(0);
        }

        let batch: Vec<_> = remaining
            .into_iter()
            .take(self.config.sessions_per_batch)
            .cloned()
            .collect();

        let batch_size = batch.len();

        if self.config.verbose {
            println!(
                "[Async Mode] Ingesting batch of {} sessions ({}/{} total)...",
                batch_size,
                self.checkpoint.ingested_sessions.len(),
                self.checkpoint.total_sessions
            );
        }

        // Ingest the batch
        let stats = ingester.ingest_sessions_async(&batch).await?;

        // Mark successful sessions (those not in errors)
        let failed_sessions: HashSet<_> = stats
            .errors
            .iter()
            .filter_map(|e| {
                // Extract session ID from error message "Session X: ..."
                e.split(':')
                    .next()
                    .map(|s| s.replace("Session ", "").trim().to_string())
            })
            .collect();

        for session in &batch {
            if !failed_sessions.contains(&session.session_id) {
                self.checkpoint.mark_session_ingested(&session.session_id);
            }
        }

        // Check if ingestion is complete
        if self.checkpoint.ingested_sessions.len() >= self.checkpoint.total_sessions {
            self.checkpoint.phase = BenchmarkPhase::Answering;
        }

        self.save_checkpoint()?;

        if self.config.verbose {
            println!(
                "Batch complete. Progress: {:.1}% ({} errors in batch)",
                self.checkpoint.ingestion_progress(),
                stats.errors.len()
            );

            // Print first few errors for debugging
            if !stats.errors.is_empty() {
                println!("Sample errors:");
                for error in stats.errors.iter().take(5) {
                    println!("  - {}", error);
                }
                if stats.errors.len() > 5 {
                    println!("  ... and {} more", stats.errors.len() - 5);
                }
            }
        }

        Ok(batch_size - stats.errors.len())
    }

    /// Run ingestion using OpenAI Batch API (slower, 50% cheaper)
    ///
    /// This is a multi-step process:
    /// 1. If no pending batch: generate JSONL file and submit to OpenAI
    /// 2. If pending batch: poll status and process results when complete
    async fn run_ingestion_batch_api(
        &mut self,
        sessions: &[BenchmarkSession],
        ingester: &SessionIngester,
    ) -> Result<usize> {
        // Check if we have a pending batch to poll
        if self.checkpoint.has_pending_batch() {
            return self.poll_and_process_batch_api(sessions, ingester).await;
        }

        // No pending batch - start a new one
        let remaining: Vec<_> = self.checkpoint.remaining_sessions(sessions);

        if remaining.is_empty() {
            self.checkpoint.phase = BenchmarkPhase::Answering;
            self.save_checkpoint()?;
            return Ok(0);
        }

        let batch: Vec<_> = remaining
            .into_iter()
            .take(self.config.sessions_per_batch)
            .cloned()
            .collect();

        let batch_size = batch.len();

        if self.config.verbose {
            println!(
                "[Batch API Mode] Preparing batch of {} sessions ({}/{} total)...",
                batch_size,
                self.checkpoint.ingested_sessions.len(),
                self.checkpoint.total_sessions
            );
        }

        // Ensure batch files directory exists
        fs::create_dir_all(&self.config.batch_files_dir).map_err(|e| {
            BenchmarkError::Ingestion(format!("Failed to create batch files dir: {}", e))
        })?;

        // Generate JSONL file
        let jsonl_filename = format!("batch_{}.jsonl", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
        let jsonl_path = Path::new(&self.config.batch_files_dir).join(&jsonl_filename);

        let model = ingester.extraction_model().unwrap_or("gpt-4o-mini");
        let count = ingester.generate_batch_file(&batch, &jsonl_path, model)?;

        if self.config.verbose {
            println!(
                "Generated JSONL file: {} ({} requests)",
                jsonl_path.display(),
                count
            );
        }

        // Submit to OpenAI Batch API
        let batch_id = ingester.submit_batch(&jsonl_path).await?;

        if self.config.verbose {
            println!("Submitted batch job: {}", batch_id);
            println!("Batch API jobs typically complete within 24 hours.");
            println!("Run this command again to check status and process results.");
        }

        // Save pending batch state
        let session_ids: Vec<String> = batch.iter().map(|s| s.session_id.clone()).collect();
        self.checkpoint.start_batch(
            &batch_id,
            session_ids,
            jsonl_path.to_string_lossy().as_ref(),
        );
        self.save_checkpoint()?;

        // Return 0 since processing happens later
        Ok(0)
    }

    /// Poll pending batch and process results when complete
    async fn poll_and_process_batch_api(
        &mut self,
        sessions: &[BenchmarkSession],
        ingester: &SessionIngester,
    ) -> Result<usize> {
        use crate::longmemeval::ingester::BatchPollResult;

        let batch_id = self
            .checkpoint
            .get_pending_batch_id()
            .ok_or_else(|| BenchmarkError::Ingestion("No pending batch to poll".into()))?
            .to_string();

        if self.config.verbose {
            println!("[Batch API Mode] Polling batch job: {}", batch_id);
        }

        // Build session lookup map (owned, as required by poll_and_process_batch)
        let session_lookup: HashMap<String, BenchmarkSession> = sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.clone()))
            .collect();

        let model = ingester.extraction_model().unwrap_or("gpt-4o-mini");
        let poll_result = ingester
            .poll_and_process_batch(&batch_id, &session_lookup, model)
            .await?;

        match poll_result {
            BatchPollResult::InProgress {
                completed,
                failed,
                total,
            } => {
                if self.config.verbose {
                    println!(
                        "Batch still in progress: {}/{} completed, {} failed",
                        completed, total, failed
                    );
                    println!("Run this command again later to check status.");
                }
                Ok(0)
            }
            BatchPollResult::Completed {
                sessions_processed,
                facts_extracted,
                errors,
            } => {
                if self.config.verbose {
                    println!(
                        "Batch complete! Processed {} sessions, extracted {} facts, {} errors",
                        sessions_processed,
                        facts_extracted,
                        errors.len()
                    );
                    if !errors.is_empty() {
                        println!("Sample errors:");
                        for error in errors.iter().take(3) {
                            println!("  - {}", error);
                        }
                    }
                }

                // Mark sessions as ingested
                let pending_sessions = self.checkpoint.pending_batch_sessions.clone();
                self.checkpoint.batch_processed(&pending_sessions);

                // Check if ingestion is complete
                if self.checkpoint.ingested_sessions.len() >= self.checkpoint.total_sessions {
                    self.checkpoint.phase = BenchmarkPhase::Answering;
                }

                self.save_checkpoint()?;

                if self.config.verbose {
                    println!("Progress: {:.1}%", self.checkpoint.ingestion_progress());
                }

                Ok(sessions_processed)
            }
            BatchPollResult::Failed { error } => {
                if self.config.verbose {
                    println!("Batch failed: {}", error);
                    println!("Clearing batch state. You can retry by running again.");
                }
                self.checkpoint.clear_batch();
                self.save_checkpoint()?;
                Err(BenchmarkError::Ingestion(format!("Batch failed: {}", error)).into())
            }
        }
    }

    /// Run one batch of question answering (supports concurrent processing)
    pub async fn run_answering_batch(
        &mut self,
        questions: &[BenchmarkQuestion],
        answerer: &AnswerGenerator,
        judge: &Judge,
    ) -> Result<usize> {
        let remaining: Vec<_> = self.checkpoint.remaining_questions(questions);

        if remaining.is_empty() {
            self.checkpoint.phase = BenchmarkPhase::Complete;
            self.save_checkpoint()?;
            return Ok(0);
        }

        let batch: Vec<_> = remaining
            .into_iter()
            .take(self.config.questions_per_batch)
            .collect();

        let batch_size = batch.len();
        let concurrency = self.config.answer_concurrency.max(1);

        if self.config.verbose {
            println!(
                "Answering batch of {} questions ({}/{} total, concurrency={})...",
                batch_size,
                self.checkpoint.answered_questions.len(),
                self.checkpoint.total_questions,
                concurrency,
            );
        }

        if concurrency <= 1 {
            // Sequential path (original behavior)
            let mut answered = 0;
            for question in batch {
                let user_id = format!("user_{}", question.id);

                match answerer.answer_async(question, &user_id).await {
                    Ok(answer_result) => {
                        let judge_result = judge
                            .judge_async(
                                &question.question,
                                &question.answer,
                                &answer_result.answer,
                                question.category,
                            )
                            .await?;

                        let result = QuestionResult {
                            question_id: question.id.clone(),
                            question: question.question.clone(),
                            expected: question.answer.clone(),
                            generated: answer_result.answer,
                            is_correct: judge_result.is_correct,
                            score: judge_result.score,
                            category: question.category,
                            answered_at: Utc::now(),
                            tool_trace: answer_result.tool_trace,
                            fallback_used: answer_result.fallback_used,
                            fallback_reason: answer_result.fallback_reason,
                            primary_model: answer_result.primary_model,
                            final_model: answer_result.final_model,
                        };

                        if self.config.verbose {
                            let mark = if result.is_correct { "✓" } else { "✗" };
                            let qnum = self.checkpoint.answered_questions.len() + 1;
                            if !result.is_correct {
                                println!(
                                    "  Q{}: {} ({}) [{}] Q: {} | Expected: {} | Got: {}",
                                    qnum,
                                    mark,
                                    question.category,
                                    question.id,
                                    question.question,
                                    result.expected,
                                    result.generated.chars().take(200).collect::<String>()
                                );
                            } else {
                                println!("  Q{}: {} ({})", qnum, mark, question.category);
                            }
                        }

                        self.checkpoint.mark_question_answered(result);
                        answered += 1;
                    }
                    Err(e) => {
                        if self.config.verbose {
                            println!("  Q{}: Failed - {}", question.id, e);
                        }
                    }
                }

                // Save after each question for safety
                self.save_checkpoint()?;
            }

            if self.checkpoint.answered_questions.len() >= self.checkpoint.total_questions {
                self.checkpoint.phase = BenchmarkPhase::Complete;
                self.save_checkpoint()?;
            }

            if self.config.verbose {
                println!(
                    "Batch complete. Progress: {:.1}%, Accuracy: {:.1}%",
                    self.checkpoint.answering_progress(),
                    self.checkpoint.accuracy() * 100.0
                );
            }

            Ok(answered)
        } else {
            // Concurrent path
            let answered = std::sync::Arc::new(AtomicUsize::new(0));
            let completed = std::sync::Arc::new(AtomicUsize::new(0));
            let verbose = self.config.verbose;
            let total_answered_before = self.checkpoint.answered_questions.len();

            let results: Vec<Option<QuestionResult>> = stream::iter(batch.into_iter())
                .map(|question| {
                    let user_id = format!("user_{}", question.id);
                    let answered = answered.clone();
                    let completed = completed.clone();
                    async move {
                        match answerer.answer_async(question, &user_id).await {
                            Ok(answer_result) => {
                                match judge.judge_async(
                                    &question.question,
                                    &question.answer,
                                    &answer_result.answer,
                                    question.category,
                                ).await {
                                    Ok(judge_result) => {
                                        let result = QuestionResult {
                                            question_id: question.id.clone(),
                                            question: question.question.clone(),
                                            expected: question.answer.clone(),
                                            generated: answer_result.answer,
                                            is_correct: judge_result.is_correct,
                                            score: judge_result.score,
                                            category: question.category,
                                            answered_at: Utc::now(),
                                            tool_trace: answer_result.tool_trace,
                                            fallback_used: answer_result.fallback_used,
                                            fallback_reason: answer_result.fallback_reason,
                                            primary_model: answer_result.primary_model,
                                            final_model: answer_result.final_model,
                                        };

                                        let count = completed.fetch_add(1, Ordering::SeqCst) + 1;
                                        let qnum = total_answered_before + count;
                                        if verbose {
                                            let mark = if result.is_correct { "✓" } else { "✗" };
                                            if !result.is_correct {
                                                println!(
                                                    "  Q{}: {} ({}) [{}] Q: {} | Expected: {} | Got: {}",
                                                    qnum, mark, question.category, question.id,
                                                    question.question, result.expected,
                                                    result.generated.chars().take(200).collect::<String>()
                                                );
                                            } else {
                                                println!(
                                                    "  Q{}: {} ({})",
                                                    qnum, mark, question.category
                                                );
                                            }
                                        }

                                        answered.fetch_add(1, Ordering::SeqCst);
                                        Some(result)
                                    }
                                    Err(e) => {
                                        completed.fetch_add(1, Ordering::SeqCst);
                                        if verbose {
                                            eprintln!("  Q{}: Judge failed - {}", question.id, e);
                                        }
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                completed.fetch_add(1, Ordering::SeqCst);
                                if verbose {
                                    eprintln!("  Q{}: Failed - {}", question.id, e);
                                }
                                None
                            }
                        }
                    }
                })
                .buffer_unordered(concurrency)
                .collect()
                .await;

            // Apply results to checkpoint (sequential — checkpoint isn't concurrent-safe)
            for result in results.into_iter().flatten() {
                self.checkpoint.mark_question_answered(result);
            }
            self.save_checkpoint()?;

            if self.checkpoint.answered_questions.len() >= self.checkpoint.total_questions {
                self.checkpoint.phase = BenchmarkPhase::Complete;
                self.save_checkpoint()?;
            }

            let total_answered = answered.load(Ordering::SeqCst);
            if self.config.verbose {
                println!(
                    "Batch complete. Progress: {:.1}%, Accuracy: {:.1}%",
                    self.checkpoint.answering_progress(),
                    self.checkpoint.accuracy() * 100.0
                );
            }

            Ok(total_answered)
        }
    }

    /// Save checkpoint to disk
    fn save_checkpoint(&self) -> Result<()> {
        let path = Path::new(&self.config.checkpoint_path);
        self.checkpoint.save(path)
    }

    /// Get the current phase
    pub fn phase(&self) -> BenchmarkPhase {
        self.checkpoint.phase
    }

    /// Force transition to answering phase (e.g., after partial ingestion due to rate limits)
    pub fn force_answering_phase(&mut self) {
        if self.checkpoint.phase == BenchmarkPhase::Ingestion {
            self.checkpoint.phase = BenchmarkPhase::Answering;
            self.save_checkpoint().ok();
        }
    }

    /// Get reference to checkpoint (for checking batch status)
    pub fn checkpoint(&self) -> &BenchmarkCheckpoint {
        &self.checkpoint
    }

    /// Check if benchmark is complete
    pub fn is_complete(&self) -> bool {
        self.checkpoint.phase == BenchmarkPhase::Complete
    }

    /// Get final results (only valid when complete)
    pub fn final_results(&self) -> Option<FinalResults> {
        if !self.is_complete() {
            return None;
        }

        let correct = self
            .checkpoint
            .question_results
            .iter()
            .filter(|r| r.is_correct)
            .count();

        Some(FinalResults {
            total_questions: self.checkpoint.total_questions,
            correct_answers: correct,
            accuracy: self.checkpoint.accuracy(),
            results_by_category: self.results_by_category(),
        })
    }

    fn results_by_category(&self) -> Vec<(QuestionCategory, usize, usize, f32)> {
        use std::collections::HashMap;

        let mut by_category: HashMap<QuestionCategory, (usize, usize)> = HashMap::new();

        for result in &self.checkpoint.question_results {
            let entry = by_category.entry(result.category).or_insert((0, 0));
            entry.0 += 1; // total
            if result.is_correct {
                entry.1 += 1; // correct
            }
        }

        by_category
            .into_iter()
            .map(|(cat, (total, correct))| {
                let acc = if total > 0 {
                    correct as f32 / total as f32
                } else {
                    0.0
                };
                (cat, total, correct, acc)
            })
            .collect()
    }
}

/// Final benchmark results
#[derive(Debug, Clone)]
pub struct FinalResults {
    pub total_questions: usize,
    pub correct_answers: usize,
    pub accuracy: f32,
    pub results_by_category: Vec<(QuestionCategory, usize, usize, f32)>,
}

impl std::fmt::Display for FinalResults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Final Results ===")?;
        writeln!(
            f,
            "Total: {}/{} ({:.1}%)",
            self.correct_answers,
            self.total_questions,
            self.accuracy * 100.0
        )?;
        writeln!(f, "\nBy Category:")?;
        for (cat, total, correct, acc) in &self.results_by_category {
            writeln!(
                f,
                "  {:?}: {}/{} ({:.1}%)",
                cat,
                correct,
                total,
                acc * 100.0
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_checkpoint_new() {
        let checkpoint = BenchmarkCheckpoint::new("test_run", 100, 50);
        assert_eq!(checkpoint.run_id, "test_run");
        assert_eq!(checkpoint.total_sessions, 100);
        assert_eq!(checkpoint.total_questions, 50);
        assert_eq!(checkpoint.phase, BenchmarkPhase::Ingestion);
    }

    #[test]
    fn test_checkpoint_save_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");

        let mut checkpoint = BenchmarkCheckpoint::new("test", 10, 5);
        checkpoint.mark_session_ingested("session_1");
        checkpoint.save(&path).unwrap();

        let loaded = BenchmarkCheckpoint::load_or_create(&path, 10, 5).unwrap();
        assert_eq!(loaded.run_id, "test");
        assert!(loaded.is_session_ingested("session_1"));
    }

    #[test]
    fn test_checkpoint_progress() {
        let mut checkpoint = BenchmarkCheckpoint::new("test", 10, 5);
        assert_eq!(checkpoint.ingestion_progress(), 0.0);

        checkpoint.mark_session_ingested("s1");
        checkpoint.mark_session_ingested("s2");
        assert_eq!(checkpoint.ingestion_progress(), 20.0);
    }

    #[test]
    fn test_batch_config_presets() {
        let one_hour = BatchConfig::one_hour_batches();
        assert_eq!(one_hour.sessions_per_batch, 1500);

        let thirty_min = BatchConfig::thirty_min_batches();
        assert_eq!(thirty_min.sessions_per_batch, 750);
    }

    #[test]
    fn test_checkpoint_batch_tracking() {
        let mut checkpoint = BenchmarkCheckpoint::new("test", 100, 50);

        // Start a batch
        checkpoint.start_batch(
            "batch_123",
            vec!["s1".to_string(), "s2".to_string()],
            "/tmp/batch.jsonl",
        );

        assert!(checkpoint.has_pending_batch());
        assert_eq!(checkpoint.get_pending_batch_id(), Some("batch_123"));
        assert_eq!(checkpoint.phase, BenchmarkPhase::BatchPending);

        // Mark completed
        checkpoint.batch_completed("file_456");
        assert_eq!(
            checkpoint.batch_output_file_id,
            Some("file_456".to_string())
        );
        assert_eq!(checkpoint.phase, BenchmarkPhase::BatchProcessing);

        // Process results
        checkpoint.batch_processed(&["s1".to_string(), "s2".to_string()]);
        assert!(!checkpoint.has_pending_batch());
        assert!(checkpoint.is_session_ingested("s1"));
        assert!(checkpoint.is_session_ingested("s2"));
        assert_eq!(checkpoint.phase, BenchmarkPhase::BatchGenerate);
    }
}
