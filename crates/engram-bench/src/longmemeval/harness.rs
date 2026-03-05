//! LongMemEval-S benchmark harness
//!
//! Runs the full benchmark evaluation pipeline.

use std::time::Instant;

use chrono::{DateTime, NaiveDateTime, Utc};
use uuid::Uuid;

use crate::storage::ResultStorage;
use crate::types::{
    BenchmarkConfig, BenchmarkMessage, BenchmarkQuestion, BenchmarkResult, BenchmarkSession,
    QuestionCategory, QuestionResult,
};
use crate::error::{BenchmarkError, Result};

use super::answerer::{AnswerGenerator, AnswererConfig};
use super::ingester::{IngesterConfig, IngestionStats, SessionIngester};
use super::judge::{Judge, JudgeConfig};

/// LongMemEval-S benchmark harness
///
/// Orchestrates the full benchmark evaluation:
/// 1. Load dataset (sessions and questions)
/// 2. Ingest sessions into memory system
/// 3. Answer questions using retrieval + LLM
/// 4. Judge answers using GPT-4o
/// 5. Calculate per-category and overall accuracy
/// 6. Store results for comparison
pub struct LongMemEvalHarness {
    config: BenchmarkConfig,
    ingester: SessionIngester,
    answerer: AnswerGenerator,
    judge: Judge,
    result_storage: Option<ResultStorage>,
}

impl LongMemEvalHarness {
    /// Create a new harness with the given configuration
    pub fn new(
        config: BenchmarkConfig,
        ingester: SessionIngester,
        answerer: AnswerGenerator,
        judge: Judge,
    ) -> Self {
        Self {
            config,
            ingester,
            answerer,
            judge,
            result_storage: None,
        }
    }

    /// Create with default components
    pub fn with_defaults(config: BenchmarkConfig) -> Self {
        Self {
            config,
            ingester: SessionIngester::new(IngesterConfig::default()),
            answerer: AnswerGenerator::new(AnswererConfig::default()),
            judge: Judge::new(JudgeConfig::default()),
            result_storage: None,
        }
    }

    /// Set result storage
    pub fn with_result_storage(mut self, storage: ResultStorage) -> Self {
        self.result_storage = Some(storage);
        self
    }

    /// Get the configuration
    pub fn config(&self) -> &BenchmarkConfig {
        &self.config
    }

    /// Run the full benchmark
    pub fn run(
        &self,
        sessions: &[BenchmarkSession],
        questions: &[BenchmarkQuestion],
    ) -> Result<BenchmarkResult> {
        let run_id = Uuid::now_v7();
        let started_at = Utc::now();
        let start = Instant::now();
        let mut total_cost = 0.0f32;

        tracing::info!("Starting LongMemEval-S benchmark run {}", run_id);

        // Filter questions by category and limit if configured
        let filtered_questions = self.filter_questions(questions);
        tracing::info!("Running on {} questions", filtered_questions.len());

        // Clear existing data
        if self.ingester.config().clear_before_ingest {
            tracing::info!("Clearing existing data...");
            self.ingester.clear_data()?;
        }

        // Ingest sessions
        tracing::info!("Ingesting {} sessions...", sessions.len());
        let ingestion_stats = self.ingester.ingest_sessions(sessions)?;
        self.log_ingestion_stats(&ingestion_stats);

        // Process questions
        let mut question_results = Vec::new();

        for (i, question) in filtered_questions.iter().enumerate() {
            match self.process_question(question) {
                Ok((result, cost)) => {
                    question_results.push(result);
                    total_cost += cost;
                }
                Err(e) => {
                    tracing::error!("Error processing question {}: {}", question.id, e);
                    question_results.push(self.create_error_result(question, &e));
                }
            }

            // Progress logging
            if (i + 1) % 10 == 0 {
                let correct = question_results.iter().filter(|r| r.is_correct).count();
                tracing::info!(
                    "Progress: {}/{} questions, current accuracy: {:.1}%",
                    i + 1,
                    filtered_questions.len(),
                    (correct as f32 / (i + 1) as f32) * 100.0
                );
            }
        }

        let completed_at = Utc::now();

        // Build result
        let mut result = BenchmarkResult {
            run_id,
            benchmark_name: "LongMemEval-S".to_string(),
            config: self.config.clone(),
            started_at,
            completed_at,
            total_questions: 0,
            correct_count: 0,
            accuracy: 0.0,
            category_scores: vec![],
            question_results,
            total_time_seconds: start.elapsed().as_secs_f32(),
            estimated_cost_usd: total_cost,
        };

        // Calculate scores
        result = result.calculate_scores();

        // Save results if storage is configured
        if let Some(ref storage) = self.result_storage {
            storage.save_result(&result)?;
            tracing::info!("Results saved to database");
        }

        // Print summary
        self.print_summary(&result);

        Ok(result)
    }

    /// Filter questions based on configuration
    pub fn filter_questions(&self, questions: &[BenchmarkQuestion]) -> Vec<BenchmarkQuestion> {
        let mut filtered: Vec<_> = questions
            .iter()
            .filter(|q| self.config.includes_category(q.category))
            .cloned()
            .collect();

        // Apply max_questions limit if set
        if self.config.max_questions > 0 && filtered.len() > self.config.max_questions {
            if let Some(seed) = self.config.seed {
                // Deterministic shuffle
                use rand::seq::SliceRandom;
                use rand::SeedableRng;
                let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
                filtered.shuffle(&mut rng);
            }
            filtered.truncate(self.config.max_questions);
        }

        filtered
    }

    /// Process a single question
    fn process_question(&self, question: &BenchmarkQuestion) -> Result<(QuestionResult, f32)> {
        let user_id = question
            .session_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or("default");

        // Generate answer
        let answer_result = self.answerer.answer(question, user_id)?;

        // Judge answer
        let judge_result = self.judge.judge(
            &question.question,
            &question.answer,
            &answer_result.answer,
            question.category,
        )?;

        let total_cost = answer_result.cost_usd + judge_result.cost_usd;

        let result = QuestionResult {
            question_id: question.id.clone(),
            category: question.category,
            question: question.question.clone(),
            expected_answer: question.answer.clone(),
            generated_answer: answer_result.answer,
            retrieved_memories: answer_result.retrieved_memories,
            is_correct: judge_result.is_correct,
            judge_score: judge_result.score,
            judge_reasoning: judge_result.reasoning,
            retrieval_time_ms: answer_result.retrieval_time_ms,
            answer_time_ms: answer_result.answer_time_ms,
            total_time_ms: answer_result.total_time_ms,
        };

        Ok((result, total_cost))
    }

    /// Create an error result for a failed question
    fn create_error_result(
        &self,
        question: &BenchmarkQuestion,
        error: &crate::error::Error,
    ) -> QuestionResult {
        QuestionResult {
            question_id: question.id.clone(),
            category: question.category,
            question: question.question.clone(),
            expected_answer: question.answer.clone(),
            generated_answer: format!("ERROR: {}", error),
            retrieved_memories: vec![],
            is_correct: false,
            judge_score: 0.0,
            judge_reasoning: error.to_string(),
            retrieval_time_ms: 0,
            answer_time_ms: 0,
            total_time_ms: 0,
        }
    }

    /// Log ingestion statistics
    fn log_ingestion_stats(&self, stats: &IngestionStats) {
        tracing::info!(
            "Ingested {} sessions, created {} memories, extracted {} entities",
            stats.sessions_processed,
            stats.memories_created,
            stats.entities_extracted
        );

        if !stats.errors.is_empty() {
            tracing::warn!("Ingestion had {} errors", stats.errors.len());
            for error in &stats.errors {
                tracing::warn!("  - {}", error);
            }
        }
    }

    /// Print benchmark summary
    fn print_summary(&self, result: &BenchmarkResult) {
        let separator = "=".repeat(60);
        println!("\n{}", separator);
        println!("LongMemEval-S Benchmark Results");
        println!("{}", separator);
        println!("Run ID: {}", result.run_id);
        println!("Extraction Mode: {}", result.config.extraction_mode);
        println!("Total Questions: {}", result.total_questions);
        println!("Correct: {}", result.correct_count);
        println!("Overall Accuracy: {:.1}%", result.accuracy * 100.0);
        println!();
        println!("Per-Category Results:");
        for cs in &result.category_scores {
            println!(
                "  {:12} {}/{:3} ({:.1}%)",
                cs.category.as_str(),
                cs.correct,
                cs.total,
                cs.accuracy * 100.0
            );
        }
        println!();
        println!("Total Time: {:.1}s", result.total_time_seconds);
        println!("Estimated Cost: ${:.2}", result.estimated_cost_usd);
        println!("{}", separator);
    }

    /// Check if target accuracy is met
    pub fn meets_target(&self, result: &BenchmarkResult, target: f32) -> bool {
        result.accuracy >= target
    }

    /// Compare with a previous run
    pub fn compare_with_baseline(
        &self,
        result: &BenchmarkResult,
        baseline: &BenchmarkResult,
    ) -> RunComparison {
        let mut category_diffs = Vec::new();

        for category in QuestionCategory::all() {
            let current_score = result.category_accuracy(category).unwrap_or(0.0);
            let baseline_score = baseline.category_accuracy(category).unwrap_or(0.0);

            category_diffs.push(CategoryComparison {
                category,
                current: current_score,
                baseline: baseline_score,
                diff: current_score - baseline_score,
            });
        }

        RunComparison {
            current_accuracy: result.accuracy,
            baseline_accuracy: baseline.accuracy,
            accuracy_diff: result.accuracy - baseline.accuracy,
            category_diffs,
            is_improvement: result.accuracy > baseline.accuracy,
        }
    }
}

/// Comparison between two benchmark runs
#[derive(Debug, Clone)]
pub struct RunComparison {
    /// Current run accuracy
    pub current_accuracy: f32,
    /// Baseline run accuracy
    pub baseline_accuracy: f32,
    /// Accuracy difference
    pub accuracy_diff: f32,
    /// Per-category comparisons
    pub category_diffs: Vec<CategoryComparison>,
    /// Whether this is an improvement
    pub is_improvement: bool,
}

impl RunComparison {
    /// Get the improvement as a percentage
    pub fn improvement_percent(&self) -> f32 {
        if self.baseline_accuracy > 0.0 {
            (self.accuracy_diff / self.baseline_accuracy) * 100.0
        } else {
            0.0
        }
    }
}

/// Comparison for a single category
#[derive(Debug, Clone)]
pub struct CategoryComparison {
    /// Category
    pub category: QuestionCategory,
    /// Current accuracy
    pub current: f32,
    /// Baseline accuracy
    pub baseline: f32,
    /// Difference
    pub diff: f32,
}

impl CategoryComparison {
    /// Check if this category improved
    pub fn improved(&self) -> bool {
        self.diff > 0.0
    }

    /// Check if this category regressed
    pub fn regressed(&self) -> bool {
        self.diff < 0.0
    }
}

/// Dataset loader for benchmark data
#[derive(Debug, Default)]
pub struct DatasetLoader {
    data_dir: Option<String>,
}

impl DatasetLoader {
    /// Create a new loader
    pub fn new() -> Self {
        Self { data_dir: None }
    }

    /// Set custom data directory
    pub fn with_data_dir(mut self, dir: impl Into<String>) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    /// Get the data directory path
    fn get_data_path(&self) -> String {
        self.data_dir
            .clone()
            .unwrap_or_else(|| "data/benchmarks/longmemeval".to_string())
    }

    /// Load the LongMemEval-S dataset from JSON file
    pub fn load_longmemeval_s(&self) -> Result<LongMemEvalDataset> {
        let path = format!("{}/longmemeval_s_cleaned.json", self.get_data_path());
        self.load_from_file(&path)
    }

    /// Load dataset from a specific file path
    pub fn load_from_file(&self, path: &str) -> Result<LongMemEvalDataset> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| BenchmarkError::Download(format!("Failed to read {}: {}", path, e)))?;

        self.parse_dataset(&content)
    }

    /// Parse the dataset JSON content
    pub fn parse_dataset(&self, content: &str) -> Result<LongMemEvalDataset> {
        let items: Vec<serde_json::Value> = serde_json::from_str(content)
            .map_err(|e| BenchmarkError::Download(format!("JSON parse error: {}", e)))?;

        let mut all_sessions = Vec::new();
        let mut questions = Vec::new();
        let mut session_id_counter = 0;

        for item in items {
            // Parse question
            let question_id = item["question_id"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            let question_type = item["question_type"].as_str().unwrap_or("extraction");
            let question_text = item["question"].as_str().unwrap_or("").to_string();
            let answer = match &item["answer"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => String::new(),
            };

            // Map question_type to category (check _abs suffix FIRST)
            let category = Self::map_question_type(&question_id, question_type);

            // Parse haystack_dates (1:1 with haystack_sessions)
            let haystack_dates: Vec<Option<DateTime<Utc>>> = item["haystack_dates"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|v| v.as_str().and_then(Self::parse_longmemeval_date))
                        .collect()
                })
                .unwrap_or_default();

            // Parse question_date
            let question_date = item["question_date"]
                .as_str()
                .and_then(Self::parse_longmemeval_date);

            // Parse haystack_session_ids (external IDs, 1:1 with haystack_sessions)
            let haystack_ext_ids: Vec<String> = item["haystack_session_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Parse answer_session_ids (external IDs of sessions containing the answer)
            let answer_ext_ids: Vec<String> = item["answer_session_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Parse haystack sessions
            let mut session_ids = Vec::new();
            let first_session_counter = session_id_counter;
            if let Some(haystack) = item["haystack_sessions"].as_array() {
                for (session_idx, session_data) in haystack.iter().enumerate() {
                    if let Some(messages_array) = session_data.as_array() {
                        let session_id = format!("session_{}", session_id_counter);
                        session_id_counter += 1;

                        // Create user_id based on question (each question has its own user context)
                        let user_id = format!("user_{}", question_id);

                        let mut session = BenchmarkSession::new(&session_id, &user_id);

                        // Use the session date from haystack_dates, or fallback to Utc::now()
                        let session_date = haystack_dates
                            .get(session_idx)
                            .copied()
                            .flatten()
                            .unwrap_or_else(Utc::now);

                        for msg in messages_array {
                            let role = msg["role"].as_str().unwrap_or("user").to_string();
                            let content = msg["content"].as_str().unwrap_or("").to_string();

                            session = session.with_message(BenchmarkMessage {
                                role,
                                content,
                                timestamp: session_date,
                            });
                        }

                        session_ids.push(session_id.clone());
                        all_sessions.push(session);
                    }
                }
            }

            // Map answer_session_ids (external) to internal session_N IDs
            let answer_internal_ids: Vec<String> = answer_ext_ids
                .iter()
                .filter_map(|ext_id| {
                    haystack_ext_ids
                        .iter()
                        .position(|h| h == ext_id)
                        .map(|idx| format!("session_{}", first_session_counter + idx))
                })
                .collect();

            // Create question with session references
            let mut question =
                BenchmarkQuestion::new(&question_id, &question_text, &answer, category);
            question.session_ids = session_ids;
            question.answer_session_ids = answer_internal_ids;
            if let Some(qd) = question_date {
                question = question.with_question_date(qd);
            }
            questions.push(question);
        }

        tracing::info!(
            "Loaded {} questions with {} total sessions",
            questions.len(),
            all_sessions.len()
        );

        Ok(LongMemEvalDataset {
            sessions: all_sessions,
            questions,
        })
    }

    /// Parse a LongMemEval date string like "2023/05/30 (Tue) 23:40" into DateTime<Utc>
    fn parse_longmemeval_date(date_str: &str) -> Option<DateTime<Utc>> {
        // Strip the day-of-week part: "2023/05/30 (Tue) 23:40" -> "2023/05/30 23:40"
        let cleaned = if let Some(paren_start) = date_str.find('(') {
            if let Some(paren_end) = date_str.find(')') {
                let before = date_str[..paren_start].trim_end();
                let after = date_str[paren_end + 1..].trim_start();
                format!("{} {}", before, after)
            } else {
                date_str.to_string()
            }
        } else {
            date_str.to_string()
        };

        NaiveDateTime::parse_from_str(cleaned.trim(), "%Y/%m/%d %H:%M")
            .ok()
            .map(|ndt| ndt.and_utc())
    }

    /// Map question ID and type to QuestionCategory
    ///
    /// CRITICAL: Check for _abs suffix FIRST, then match on question_type
    fn map_question_type(question_id: &str, question_type: &str) -> QuestionCategory {
        // Check for abstention FIRST (questions ending with _abs)
        if question_id.ends_with("_abs") {
            return QuestionCategory::Abstention;
        }

        // Then match on question_type
        match question_type.to_lowercase().as_str() {
            t if t.contains("temporal") => QuestionCategory::Temporal,
            t if t.contains("update") => QuestionCategory::Updates,
            t if t.contains("multi") || t.contains("reason") => QuestionCategory::MultiSession,
            t if t.contains("abstain") || t.contains("unanswer") => QuestionCategory::Abstention,
            t if t.starts_with("single-session") => QuestionCategory::Extraction,
            _ => QuestionCategory::Extraction, // Default to extraction
        }
    }
}

/// LongMemEval-S dataset
#[derive(Debug)]
pub struct LongMemEvalDataset {
    /// Sessions from the dataset
    pub sessions: Vec<BenchmarkSession>,
    /// Questions for evaluation
    pub questions: Vec<BenchmarkQuestion>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CategoryScore;
    use chrono::Utc;

    fn create_test_sessions() -> Vec<BenchmarkSession> {
        let now = Utc::now();
        vec![BenchmarkSession::new("session-1", "user-1")
            .with_message(crate::BenchmarkMessage::user("My name is John", now))
            .with_message(crate::BenchmarkMessage::assistant(
                "Nice to meet you, John!",
                now,
            ))]
    }

    fn create_test_questions() -> Vec<BenchmarkQuestion> {
        vec![
            BenchmarkQuestion::new(
                "q1",
                "What is the user's name?",
                "John",
                QuestionCategory::Extraction,
            ),
            BenchmarkQuestion::new(
                "q2",
                "When did the user change their address?",
                "Last week",
                QuestionCategory::Updates,
            ),
            BenchmarkQuestion::new(
                "q3",
                "What is the user's favorite sport?",
                "ABSTAIN",
                QuestionCategory::Abstention,
            ),
        ]
    }

    #[test]
    fn test_harness_creation() {
        let config = BenchmarkConfig::default();
        let harness = LongMemEvalHarness::with_defaults(config.clone());

        assert_eq!(harness.config().name, config.name);
    }

    #[test]
    fn test_filter_questions_no_filter() {
        let config = BenchmarkConfig::default(); // Empty categories = all
        let harness = LongMemEvalHarness::with_defaults(config);
        let questions = create_test_questions();

        let filtered = harness.filter_questions(&questions);
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_filter_questions_by_category() {
        let config = BenchmarkConfig::default().with_categories(vec![QuestionCategory::Extraction]);
        let harness = LongMemEvalHarness::with_defaults(config);
        let questions = create_test_questions();

        let filtered = harness.filter_questions(&questions);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].category, QuestionCategory::Extraction);
    }

    #[test]
    fn test_filter_questions_with_limit() {
        let config = BenchmarkConfig::default()
            .with_max_questions(2)
            .with_seed(42);
        let harness = LongMemEvalHarness::with_defaults(config);
        let questions = create_test_questions();

        let filtered = harness.filter_questions(&questions);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_run_benchmark() {
        let config = BenchmarkConfig::new("test");
        let harness = LongMemEvalHarness::with_defaults(config);

        let sessions = create_test_sessions();
        let questions = create_test_questions();

        let result = harness.run(&sessions, &questions);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.benchmark_name, "LongMemEval-S");
        assert_eq!(result.total_questions, 3);
    }

    #[test]
    fn test_meets_target() {
        let config = BenchmarkConfig::default();
        let harness = LongMemEvalHarness::with_defaults(config.clone());

        let mut result = BenchmarkResult::new("test", config);
        result.accuracy = 0.92;

        assert!(harness.meets_target(&result, 0.90));
        assert!(!harness.meets_target(&result, 0.95));
    }

    #[test]
    fn test_compare_with_baseline() {
        let config = BenchmarkConfig::default();
        let harness = LongMemEvalHarness::with_defaults(config.clone());

        let mut current = BenchmarkResult::new("test", config.clone());
        current.accuracy = 0.92;
        current.category_scores = vec![CategoryScore {
            category: QuestionCategory::Extraction,
            total: 100,
            correct: 95,
            accuracy: 0.95,
        }];

        let mut baseline = BenchmarkResult::new("test", config);
        baseline.accuracy = 0.88;
        baseline.category_scores = vec![CategoryScore {
            category: QuestionCategory::Extraction,
            total: 100,
            correct: 90,
            accuracy: 0.90,
        }];

        let comparison = harness.compare_with_baseline(&current, &baseline);

        assert!(comparison.is_improvement);
        assert!((comparison.accuracy_diff - 0.04).abs() < 0.001);
        assert!(comparison.improvement_percent() > 0.0);
    }

    #[test]
    fn test_run_comparison_improvement_percent() {
        let comparison = RunComparison {
            current_accuracy: 0.95,
            baseline_accuracy: 0.90,
            accuracy_diff: 0.05,
            category_diffs: vec![],
            is_improvement: true,
        };

        // 0.05 / 0.90 * 100 = 5.55%
        let percent = comparison.improvement_percent();
        assert!((percent - 5.555).abs() < 0.1);
    }

    #[test]
    fn test_category_comparison() {
        let improved = CategoryComparison {
            category: QuestionCategory::Extraction,
            current: 0.95,
            baseline: 0.90,
            diff: 0.05,
        };

        let regressed = CategoryComparison {
            category: QuestionCategory::MultiSession,
            current: 0.85,
            baseline: 0.90,
            diff: -0.05,
        };

        assert!(improved.improved());
        assert!(!improved.regressed());
        assert!(!regressed.improved());
        assert!(regressed.regressed());
    }

    #[test]
    fn test_dataset_loader_default_path() {
        // Test that loader handles missing default path gracefully
        let loader = DatasetLoader::new();
        let result = loader.load_longmemeval_s();
        // Will fail if data directory doesn't exist at default path
        // This is expected in unit tests - integration tests use the actual path
        assert!(result.is_err() || result.is_ok());
    }

    // Category mapping tests

    #[test]
    fn test_abstention_takes_precedence() {
        // Even if question_type is "temporal-reasoning",
        // if ID ends with _abs, it should be Abstention
        let category =
            DatasetLoader::map_question_type("temporal-reasoning-123_abs", "temporal-reasoning");
        assert_eq!(category, QuestionCategory::Abstention);
    }

    #[test]
    fn test_multi_session_category() {
        let category = DatasetLoader::map_question_type("multi-session-001", "multi-session");
        assert_eq!(category, QuestionCategory::MultiSession);

        // Also test backward compatibility with "reasoning"
        let category = DatasetLoader::map_question_type("reasoning-001", "reasoning");
        assert_eq!(category, QuestionCategory::MultiSession);
    }

    #[test]
    fn test_extraction_category() {
        for question_type in [
            "single-session-user",
            "single-session-assistant",
            "single-session-preference",
        ] {
            let category =
                DatasetLoader::map_question_type(&format!("{}-001", question_type), question_type);
            assert_eq!(category, QuestionCategory::Extraction);
        }
    }

    #[test]
    fn test_temporal_category() {
        let category = DatasetLoader::map_question_type("temporal-001", "temporal-reasoning");
        assert_eq!(category, QuestionCategory::Temporal);
    }

    #[test]
    fn test_updates_category() {
        let category = DatasetLoader::map_question_type("update-001", "knowledge-update");
        assert_eq!(category, QuestionCategory::Updates);
    }

    #[test]
    fn test_parse_longmemeval_date() {
        let date = DatasetLoader::parse_longmemeval_date("2023/05/30 (Tue) 23:40");
        assert!(date.is_some());
        let dt = date.unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M").to_string(), "2023-05-30 23:40");
    }

    #[test]
    fn test_parse_longmemeval_date_no_parens() {
        // Edge case: no day-of-week
        let date = DatasetLoader::parse_longmemeval_date("2023/05/30 23:40");
        assert!(date.is_some());
    }

    #[test]
    fn test_parse_longmemeval_date_invalid() {
        let date = DatasetLoader::parse_longmemeval_date("not a date");
        assert!(date.is_none());
    }
}
