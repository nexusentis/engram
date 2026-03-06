//! P9: Offline Retrieval Recall Harness
//!
//! Measures retrieval quality without LLM answering costs.
//! Two tiers:
//! - SinglePass: one vector+fulltext search per question (matches A8)
//! - AgenticSynthetic: scripted multi-tool policy mimicking agent behavior

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use qdrant_client::qdrant::{Condition, Filter};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Semaphore;

use crate::types::{BenchmarkQuestion, QuestionCategory};
use engram::embedding::{EmbeddingProvider, RemoteEmbeddingProvider};
use engram::storage::QdrantStorage;

use engram::agent::ToolExecutor;

// ---- Types ----

/// Recall measurement mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecallMode {
    /// Single vector+fulltext search per question (cheapest)
    SinglePass,
    /// Scripted multi-tool policy mimicking agent (no LLM)
    AgenticSynthetic,
}

impl std::fmt::Display for RecallMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecallMode::SinglePass => write!(f, "SinglePass"),
            RecallMode::AgenticSynthetic => write!(f, "AgenticSynthetic"),
        }
    }
}

/// Configuration for the recall harness
#[derive(Debug, Clone)]
pub struct RecallConfig {
    /// Which mode to run
    pub mode: RecallMode,
    /// Number of facts to retrieve (default 25)
    pub fact_k: u64,
    /// Number of messages to retrieve (default 20)
    pub msg_k: usize,
    /// Concurrency for parallel question processing (default 20)
    pub concurrency: usize,
    /// Path to write JSONL output
    pub output_path: Option<PathBuf>,
    /// Path to ANSWERS_FILE for cross-referencing benchmark correctness
    pub answers_file: Option<PathBuf>,
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            mode: RecallMode::SinglePass,
            fact_k: 25,
            msg_k: 20,
            concurrency: 20,
            output_path: None,
            answers_file: None,
        }
    }
}

/// A single tool step in the AgenticSynthetic trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStep {
    /// Tool name (e.g. "search_facts")
    pub tool_name: String,
    /// Arguments passed to the tool
    pub args: serde_json::Value,
    /// Execution time in milliseconds
    pub duration_ms: u64,
    /// Session IDs returned by this step
    pub returned_sessions: HashSet<String>,
    /// Target sessions newly discovered in this step
    pub new_target_hits: Vec<String>,
    /// Size of result in characters
    pub result_chars: usize,
}

/// Classification of retrieval failure
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FailureClass {
    /// All target sessions found
    FullRecall,
    /// Some but not all target sessions found
    PartialMiss,
    /// No target sessions found at all
    CompleteMiss,
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailureClass::FullRecall => write!(f, "full"),
            FailureClass::PartialMiss => write!(f, "partial"),
            FailureClass::CompleteMiss => write!(f, "miss"),
        }
    }
}

/// Per-question retrieval recall result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRecall {
    /// Question ID from dataset
    pub question_id: String,
    /// 1-based question index
    pub question_idx: usize,
    /// Question text
    pub question: String,
    /// Question category
    pub category: QuestionCategory,
    /// Expected answer text
    pub expected_answer: String,
    /// Target session IDs (answer_session_ids from dataset)
    pub target_sessions: Vec<String>,
    /// All session IDs found across all tool calls
    pub found_sessions: HashSet<String>,
    /// Sessions found via fact search only
    pub fact_sessions: HashSet<String>,
    /// Sessions found via message search only
    pub msg_sessions: HashSet<String>,
    /// Fraction of target sessions found (0.0 - 1.0)
    pub needle_recall: f32,
    /// Whether all target sessions were found
    pub full_recall: bool,
    /// Whether expected answer keywords appear in retrieved content
    pub content_hit: bool,
    /// Failure classification
    pub failure_class: FailureClass,
    /// Tool call trace (AgenticSynthetic only; empty for SinglePass)
    pub steps: Vec<ToolStep>,
    /// Cross-referenced benchmark correctness (from ANSWERS_FILE)
    pub benchmark_correct: Option<bool>,
}

// ---- Core functions ----

/// Run the recall harness on a set of questions
pub async fn run_recall_harness(
    config: &RecallConfig,
    questions: &[BenchmarkQuestion],
    storage: Arc<QdrantStorage>,
    embedding_provider: Arc<RemoteEmbeddingProvider>,
) -> Vec<QuestionRecall> {
    let semaphore = Arc::new(Semaphore::new(config.concurrency.max(1)));
    let progress = Arc::new(AtomicUsize::new(0));
    let total = questions.len();

    let mut handles = Vec::new();
    for (i, question) in questions.iter().enumerate() {
        if question.answer_session_ids.is_empty() {
            continue;
        }
        let storage = Arc::clone(&storage);
        let embedding_provider = Arc::clone(&embedding_provider);
        let semaphore = Arc::clone(&semaphore);
        let progress = Arc::clone(&progress);
        let question = question.clone();
        let config = config.clone();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let result = match config.mode {
                RecallMode::SinglePass => {
                    run_single_pass(i, &question, &storage, &embedding_provider, &config)
                        .await
                }
                RecallMode::AgenticSynthetic => {
                    run_agentic_synthetic(
                        i,
                        &question,
                        Arc::clone(&storage),
                        Arc::clone(&embedding_provider),
                        &config,
                    )
                    .await
                }
            };

            let done = progress.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 50 == 0 || done == total {
                eprintln!("  Progress: {}/{}", done, total);
            }

            result
        });
        handles.push(handle);
    }

    let mut results: Vec<QuestionRecall> = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Some(r)) => results.push(r),
            Ok(None) => {}
            Err(e) => eprintln!("Task panicked: {}", e),
        }
    }
    results.sort_by_key(|r| r.question_idx);
    results
}

/// SinglePass: one vector+fulltext search over facts and messages
async fn run_single_pass(
    idx: usize,
    question: &BenchmarkQuestion,
    storage: &QdrantStorage,
    embedding_provider: &RemoteEmbeddingProvider,
    config: &RecallConfig,
) -> Option<QuestionRecall> {
    let user_id = format!("user_{}", question.id);

    let embedding = match embedding_provider.embed_query(&question.question).await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("  [{}] embedding error: {}", idx + 1, e);
            return None;
        }
    };

    // Search facts: user_id + is_latest=true
    let fact_filter = Filter {
        must: vec![
            Condition::matches("is_latest", true).into(),
            Condition::matches("user_id", user_id.clone()).into(),
        ],
        ..Default::default()
    };
    let fact_results = storage
        .search_memories_hybrid(
            fact_filter,
            embedding.clone(),
            &question.question,
            config.fact_k,
            None,
        )
        .await
        .unwrap_or_default();

    // Search messages: user_id filter
    let msg_filter = Some(Filter {
        must: vec![Condition::matches("user_id", user_id).into()],
        ..Default::default()
    });
    let msg_results = storage
        .search_messages_hybrid(
            embedding,
            &question.question,
            msg_filter,
            config.msg_k,
        )
        .await
        .unwrap_or_default();

    // Collect session IDs and content
    let mut fact_sessions = HashSet::new();
    let mut all_content = Vec::new();
    for (mem, _) in &fact_results {
        if let Some(ref sid) = mem.session_id {
            fact_sessions.insert(sid.clone());
        }
        all_content.push(mem.content.clone());
    }

    let mut msg_sessions = HashSet::new();
    for point in &msg_results {
        if let Some(val) = point.payload.get("session_id") {
            if let Some(qdrant_client::qdrant::value::Kind::StringValue(sid)) = &val.kind {
                msg_sessions.insert(sid.clone());
            }
        }
        if let Some(val) = point.payload.get("content") {
            if let Some(qdrant_client::qdrant::value::Kind::StringValue(c)) = &val.kind {
                all_content.push(c.clone());
            }
        }
    }

    let found_sessions: HashSet<String> = fact_sessions.union(&msg_sessions).cloned().collect();
    let content_hit = check_content_hit(&question.answer, &all_content);
    let failure_class = classify_failure(&found_sessions, &question.answer_session_ids);
    let found_count = question
        .answer_session_ids
        .iter()
        .filter(|sid| found_sessions.contains(sid.as_str()))
        .count();
    let total_targets = question.answer_session_ids.len();

    Some(QuestionRecall {
        question_id: question.id.clone(),
        question_idx: idx,
        question: question.question.clone(),
        category: question.category,
        expected_answer: question.answer.clone(),
        target_sessions: question.answer_session_ids.clone(),
        found_sessions,
        fact_sessions,
        msg_sessions,
        needle_recall: if total_targets > 0 {
            found_count as f32 / total_targets as f32
        } else {
            1.0
        },
        full_recall: found_count == total_targets,
        content_hit,
        failure_class,
        steps: Vec::new(),
        benchmark_correct: None,
    })
}

/// AgenticSynthetic: scripted multi-tool policy (no LLM)
async fn run_agentic_synthetic(
    idx: usize,
    question: &BenchmarkQuestion,
    storage: Arc<QdrantStorage>,
    embedding_provider: Arc<RemoteEmbeddingProvider>,
    config: &RecallConfig,
) -> Option<QuestionRecall> {
    let user_id = format!("user_{}", question.id);
    let embedder: Arc<dyn engram::embedding::EmbeddingProvider> = embedding_provider;
    let executor = ToolExecutor::new(storage, embedder)
        .with_user_id(&user_id)
        .with_reference_date(question.question_date)
        .with_relative_dates(false);

    let target_set: HashSet<String> = question.answer_session_ids.iter().cloned().collect();
    let mut all_found: HashSet<String> = HashSet::new();
    let mut fact_sessions: HashSet<String> = HashSet::new();
    let mut msg_sessions: HashSet<String> = HashSet::new();
    let mut all_content: Vec<String> = Vec::new();
    let mut steps: Vec<ToolStep> = Vec::new();

    // Helper: process a step result, accumulating sessions/content/steps
    macro_rules! process_step {
        ($sr:expr, $session_set:expr) => {
            if let Some(sr) = $sr {
                $session_set.extend(sr.step.returned_sessions.iter().cloned());
                all_found.extend(sr.step.returned_sessions.iter().cloned());
                all_content.extend(sr.content_snippets);
                steps.push(sr.step);
            }
        };
    }

    // Step 1: search_facts
    let args = json!({"query": question.question, "top_k": config.fact_k});
    let sr = execute_step(&executor, "search_facts", &args, &target_set, &all_found).await;
    process_step!(sr, fact_sessions);

    if has_full_recall(&all_found, &question.answer_session_ids) {
        return Some(build_result(idx, question, all_found, fact_sessions, msg_sessions, &all_content, steps));
    }

    // Step 2: search_messages
    let args = json!({"query": question.question, "top_k": config.msg_k});
    let sr = execute_step(&executor, "search_messages", &args, &target_set, &all_found).await;
    process_step!(sr, msg_sessions);

    if has_full_recall(&all_found, &question.answer_session_ids) {
        return Some(build_result(idx, question, all_found, fact_sessions, msg_sessions, &all_content, steps));
    }

    // Step 3: grep_messages with answer keywords
    let keywords = extract_answer_keywords(&question.answer);
    if !keywords.is_empty() {
        let args = json!({"substring": keywords});
        let sr = execute_step(&executor, "grep_messages", &args, &target_set, &all_found).await;
        process_step!(sr, msg_sessions);
    }

    if has_full_recall(&all_found, &question.answer_session_ids) {
        return Some(build_result(idx, question, all_found, fact_sessions, msg_sessions, &all_content, steps));
    }

    // Step 4 (temporal): get_by_date_range if temporal question
    if question.category == QuestionCategory::Temporal {
        if let Some(qdate) = question.question_date {
            let start = qdate - chrono::Duration::days(180);
            let start_str = start.format("%Y/%m/%d").to_string();
            let end_str = qdate.format("%Y/%m/%d").to_string();
            let args = json!({
                "start_date": start_str,
                "end_date": end_str,
            });
            let sr = execute_step(&executor, "get_by_date_range", &args, &target_set, &all_found).await;
            process_step!(sr, msg_sessions);
        }
    }

    // Step 5 (multi-session): get_session_context for found target sessions
    if question.category == QuestionCategory::MultiSession {
        let found_targets: Vec<String> = question
            .answer_session_ids
            .iter()
            .filter(|sid| all_found.contains(sid.as_str()))
            .cloned()
            .collect();
        for sid in found_targets.iter().take(3) {
            let args = json!({
                "session_id": sid,
                "turn_index": 0,
                "window": 10,
                "include_facts": true,
            });
            let sr = execute_step(&executor, "get_session_context", &args, &target_set, &all_found).await;
            process_step!(sr, all_found);
        }
    }

    Some(build_result(idx, question, all_found, fact_sessions, msg_sessions, &all_content, steps))
}

/// Result from execute_step: ToolStep + content snippets for content_hit checking
struct StepResult {
    step: ToolStep,
    content_snippets: Vec<String>,
}

/// Execute a single tool step and track timing + new target hits
async fn execute_step(
    executor: &ToolExecutor,
    tool_name: &str,
    args: &serde_json::Value,
    target_set: &HashSet<String>,
    already_found: &HashSet<String>,
) -> Option<StepResult> {
    let start = Instant::now();
    match executor.execute_structured(tool_name, args).await {
        Ok(result) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            let new_target_hits: Vec<String> = result
                .sessions
                .iter()
                .filter(|sid| target_set.contains(sid.as_str()) && !already_found.contains(sid.as_str()))
                .cloned()
                .collect();
            Some(StepResult {
                step: ToolStep {
                    tool_name: tool_name.to_string(),
                    args: args.clone(),
                    duration_ms,
                    returned_sessions: result.sessions,
                    new_target_hits,
                    result_chars: result.text.len(),
                },
                content_snippets: result.content_snippets,
            })
        }
        Err(e) => {
            eprintln!("  {} error: {}", tool_name, e);
            None
        }
    }
}

/// Check if all target sessions have been found
fn has_full_recall(found: &HashSet<String>, targets: &[String]) -> bool {
    targets.iter().all(|sid| found.contains(sid))
}

/// Build the final QuestionRecall from accumulated data
fn build_result(
    idx: usize,
    question: &BenchmarkQuestion,
    found_sessions: HashSet<String>,
    fact_sessions: HashSet<String>,
    msg_sessions: HashSet<String>,
    all_content: &[String],
    steps: Vec<ToolStep>,
) -> QuestionRecall {
    let found_count = question
        .answer_session_ids
        .iter()
        .filter(|sid| found_sessions.contains(sid.as_str()))
        .count();
    let total_targets = question.answer_session_ids.len();
    let content_hit = check_content_hit(&question.answer, all_content);
    let failure_class = classify_failure(&found_sessions, &question.answer_session_ids);

    QuestionRecall {
        question_id: question.id.clone(),
        question_idx: idx,
        question: question.question.clone(),
        category: question.category,
        expected_answer: question.answer.clone(),
        target_sessions: question.answer_session_ids.clone(),
        found_sessions,
        fact_sessions,
        msg_sessions,
        needle_recall: if total_targets > 0 {
            found_count as f32 / total_targets as f32
        } else {
            1.0
        },
        full_recall: found_count == total_targets,
        content_hit,
        failure_class,
        steps,
        benchmark_correct: None,
    }
}

// ---- Helpers ----

/// Extract keyword(s) from expected answer for grep search.
/// Takes the longest word (>= 3 chars) from the answer to use as a search term.
fn extract_answer_keywords(answer: &str) -> String {
    let answer_lower = answer.to_lowercase();
    // Remove common filler words and pick the best keyword
    let stop_words: HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "yes", "no", "i", "my", "it", "and", "or",
        "to", "in", "on", "at", "of", "for", "not", "don't", "doesn't", "didn't",
    ]
    .iter()
    .copied()
    .collect();

    let words: Vec<&str> = answer_lower
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| w.len() >= 3 && !stop_words.contains(w))
        .collect();

    // Return the longest word as it's most likely to be distinctive
    words
        .into_iter()
        .max_by_key(|w| w.len())
        .unwrap_or("")
        .to_string()
}

/// Check if expected answer keywords appear in retrieved content
fn check_content_hit(expected_answer: &str, contents: &[String]) -> bool {
    let answer_lower = expected_answer.to_lowercase();
    let keywords: Vec<&str> = answer_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .collect();

    if keywords.is_empty() {
        return false;
    }

    let all_content_lower: String = contents
        .iter()
        .map(|c| c.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    // At least half the keywords must appear in the retrieved content
    let hits = keywords
        .iter()
        .filter(|kw| all_content_lower.contains(**kw))
        .count();
    hits * 2 >= keywords.len()
}

/// Classify retrieval failure
fn classify_failure(found: &HashSet<String>, targets: &[String]) -> FailureClass {
    let hits = targets.iter().filter(|sid| found.contains(sid.as_str())).count();
    if hits == targets.len() {
        FailureClass::FullRecall
    } else if hits > 0 {
        FailureClass::PartialMiss
    } else {
        FailureClass::CompleteMiss
    }
}

// ---- Output ----

/// Print summary table to stdout
pub fn print_summary(results: &[QuestionRecall], mode: RecallMode) {
    println!(
        "\n=== P9 Retrieval Recall [{}] ({} questions) ===\n",
        mode,
        results.len()
    );

    // Per-category table
    println!(
        "{:<15} {:>6} {:>6} {:>8} {:>6} {:>8} {:>10}",
        "Category", "Total", "Full", "Partial", "Miss", "Full%", "Content%"
    );
    println!("{}", "-".repeat(65));

    let categories = [
        QuestionCategory::Abstention,
        QuestionCategory::Extraction,
        QuestionCategory::MultiSession,
        QuestionCategory::Temporal,
        QuestionCategory::Updates,
    ];

    let mut grand_total = 0usize;
    let mut grand_full = 0usize;
    let mut grand_partial = 0usize;
    let mut grand_content = 0usize;

    for cat in &categories {
        let cat_results: Vec<&QuestionRecall> =
            results.iter().filter(|r| r.category == *cat).collect();
        let total = cat_results.len();
        if total == 0 {
            continue;
        }
        let full = cat_results
            .iter()
            .filter(|r| r.failure_class == FailureClass::FullRecall)
            .count();
        let partial = cat_results
            .iter()
            .filter(|r| r.failure_class == FailureClass::PartialMiss)
            .count();
        let miss = total - full - partial;
        let content = cat_results.iter().filter(|r| r.content_hit).count();
        let full_pct = full as f64 / total as f64 * 100.0;
        let content_pct = content as f64 / total as f64 * 100.0;

        grand_total += total;
        grand_full += full;
        grand_partial += partial;
        grand_content += content;

        println!(
            "{:<15} {:>6} {:>6} {:>8} {:>6} {:>7.1}% {:>9.1}%",
            format!("{:?}", cat),
            total,
            full,
            partial,
            miss,
            full_pct,
            content_pct
        );
    }

    let grand_miss = grand_total - grand_full - grand_partial;
    let grand_full_pct = if grand_total > 0 {
        grand_full as f64 / grand_total as f64 * 100.0
    } else {
        0.0
    };
    let grand_content_pct = if grand_total > 0 {
        grand_content as f64 / grand_total as f64 * 100.0
    } else {
        0.0
    };
    println!("{}", "-".repeat(65));
    println!(
        "{:<15} {:>6} {:>6} {:>8} {:>6} {:>7.1}% {:>9.1}%",
        "TOTAL",
        grand_total,
        grand_full,
        grand_partial,
        grand_miss,
        grand_full_pct,
        grand_content_pct
    );

    // Per-tool yield (AgenticSynthetic only)
    if mode == RecallMode::AgenticSynthetic {
        println!("\n=== Per-Tool Yield ===");
        let tool_names = [
            "search_facts",
            "search_messages",
            "grep_messages",
            "get_by_date_range",
            "get_session_context",
        ];
        for tool in &tool_names {
            let questions_with_hit: usize = results
                .iter()
                .filter(|r| {
                    r.steps
                        .iter()
                        .any(|s| s.tool_name == *tool && !s.new_target_hits.is_empty())
                })
                .count();
            let questions_using_tool: usize = results
                .iter()
                .filter(|r| r.steps.iter().any(|s| s.tool_name == *tool))
                .count();
            if questions_using_tool > 0 {
                println!(
                    "{:<25} {:>3}/{} questions hit at least 1 target session",
                    format!("{}:", tool),
                    questions_with_hit,
                    questions_using_tool
                );
            }
        }
    }

    // Cross-reference with answers file
    let has_answers = results.iter().any(|r| r.benchmark_correct.is_some());
    if has_answers {
        let retrieval_ok_correct = results
            .iter()
            .filter(|r| r.full_recall && r.benchmark_correct == Some(true))
            .count();
        let retrieval_ok_wrong = results
            .iter()
            .filter(|r| r.full_recall && r.benchmark_correct == Some(false))
            .count();
        let retrieval_miss_wrong = results
            .iter()
            .filter(|r| !r.full_recall && r.benchmark_correct == Some(false))
            .count();
        let retrieval_miss_correct = results
            .iter()
            .filter(|r| !r.full_recall && r.benchmark_correct == Some(true))
            .count();

        println!("\n=== Failure Buckets (with ANSWERS_FILE) ===");
        println!(
            "retrieval_ok + answer_correct:   {:>3}  (both work)",
            retrieval_ok_correct
        );
        println!(
            "retrieval_ok + answer_wrong:     {:>3}  (reasoning failure)",
            retrieval_ok_wrong
        );
        println!(
            "retrieval_miss + answer_wrong:   {:>3}  (retrieval bottleneck)",
            retrieval_miss_wrong
        );
        println!(
            "retrieval_miss + answer_correct: {:>3}  (got lucky / partial data)",
            retrieval_miss_correct
        );
    }

    // Miss details
    let misses: Vec<&QuestionRecall> = results
        .iter()
        .filter(|r| r.failure_class != FailureClass::FullRecall)
        .collect();
    if !misses.is_empty() {
        println!(
            "\n=== Questions with Incomplete Retrieval ({}) ===",
            misses.len()
        );
        for r in &misses {
            let missing: Vec<&String> = r
                .target_sessions
                .iter()
                .filter(|sid| !r.found_sessions.contains(sid.as_str()))
                .collect();
            let found_count = r.target_sessions.len() - missing.len();
            print!(
                "  Q{} [{}] ({:?}) recall={}/{} missing={:?}",
                r.question_idx + 1,
                &r.question_id[..r.question_id.len().min(12)],
                r.category,
                found_count,
                r.target_sessions.len(),
                missing
            );
            // Show per-tool hits for AgenticSynthetic
            if !r.steps.is_empty() {
                print!("\n    ");
                for step in &r.steps {
                    let hits = step.new_target_hits.len();
                    print!("{}: {} hits | ", step.tool_name, hits);
                }
            }
            println!();
        }
    }
}

/// Write results as JSONL to a file
pub fn write_jsonl(results: &[QuestionRecall], path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    for r in results {
        let line = serde_json::to_string(r).unwrap_or_default();
        writeln!(file, "{}", line)?;
    }
    println!("\nWrote {} records to {}", results.len(), path.display());
    Ok(())
}

/// Cross-reference recall results with saved benchmark answers
pub fn cross_reference_answers(results: &mut [QuestionRecall], answers_path: &Path) {
    let content = match std::fs::read_to_string(answers_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Warning: could not read ANSWERS_FILE {}: {}",
                answers_path.display(),
                e
            );
            return;
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Warning: invalid JSON in ANSWERS_FILE: {}", e);
            return;
        }
    };

    // Build a map of question_id -> is_correct
    let mut correctness: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();

    let records = if let Some(results_arr) = parsed.get("question_results") {
        results_arr.as_array().cloned().unwrap_or_default()
    } else if parsed.is_array() {
        parsed.as_array().cloned().unwrap_or_default()
    } else {
        Vec::new()
    };

    for record in &records {
        if let Some(qid) = record["question_id"].as_str() {
            let correct = record["is_correct"]
                .as_bool()
                .or_else(|| record["original_correct"].as_bool())
                .unwrap_or(false);
            correctness.insert(qid.to_string(), correct);
        }
    }

    let mut matched = 0;
    for r in results.iter_mut() {
        if let Some(&correct) = correctness.get(&r.question_id) {
            r.benchmark_correct = Some(correct);
            matched += 1;
        }
    }
    println!(
        "Cross-referenced {}/{} questions with ANSWERS_FILE",
        matched,
        results.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_answer_keywords() {
        assert_eq!(extract_answer_keywords("Paris"), "paris");
        assert_eq!(extract_answer_keywords("Software Engineer"), "engineer");
        assert_eq!(extract_answer_keywords("the blue car"), "blue");
        assert_eq!(extract_answer_keywords("42"), ""); // too short
        // Picks longest distinctive word
        assert_eq!(extract_answer_keywords("basketball"), "basketball");
    }

    #[test]
    fn test_check_content_hit() {
        let contents = vec![
            "I work as a software engineer at Google".to_string(),
            "My favorite food is pizza".to_string(),
        ];
        assert!(check_content_hit("software engineer", &contents));
        assert!(check_content_hit("pizza", &contents));
        assert!(!check_content_hit("basketball player", &contents));
    }

    #[test]
    fn test_classify_failure() {
        let found: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let targets = vec!["a".to_string(), "b".to_string()];
        assert_eq!(classify_failure(&found, &targets), FailureClass::FullRecall);

        let targets2 = vec!["a".to_string(), "c".to_string()];
        assert_eq!(
            classify_failure(&found, &targets2),
            FailureClass::PartialMiss
        );

        let targets3 = vec!["x".to_string(), "y".to_string()];
        assert_eq!(
            classify_failure(&found, &targets3),
            FailureClass::CompleteMiss
        );
    }
}
