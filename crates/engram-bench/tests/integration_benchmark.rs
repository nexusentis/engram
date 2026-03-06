//! End-to-end benchmark integration test
//!
//! Run with: OPENAI_API_KEY=... cargo test --test integration_benchmark -- --ignored --nocapture

use engram_bench::longmemeval::{
    AnswerGenerator, AnswererConfig, BenchmarkConfig, IngesterConfig, Judge, JudgeConfig, LlmClient,
    SessionIngester,
};
use engram_bench::{BenchmarkMessage, BenchmarkQuestion, BenchmarkSession, QuestionCategory};
use std::sync::Arc;

/// Stratified sample: select questions proportionally from each category
fn stratified_sample(
    questions: &[BenchmarkQuestion],
    target_total: usize,
    seed: u64,
) -> Vec<BenchmarkQuestion> {
    use rand::seq::SliceRandom;
    use rand::SeedableRng;

    // Group by category
    let mut by_category: std::collections::BTreeMap<QuestionCategory, Vec<&BenchmarkQuestion>> =
        std::collections::BTreeMap::new();
    for q in questions {
        by_category.entry(q.category).or_default().push(q);
    }

    let total = questions.len();
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut selected = Vec::new();

    for (_, cat_questions) in &mut by_category {
        // Proportional allocation, at least 1 per category
        let count = ((cat_questions.len() as f64 / total as f64) * target_total as f64)
            .round()
            .max(1.0) as usize;
        let count = count.min(cat_questions.len());

        cat_questions.shuffle(&mut rng);
        selected.extend(cat_questions.iter().take(count).map(|q| (*q).clone()));
    }

    // Shuffle the final selection for random ordering
    selected.shuffle(&mut rng);
    selected
}
use chrono::Utc;

/// Create a test session with some facts
fn create_test_session() -> BenchmarkSession {
    let now = Utc::now();
    BenchmarkSession {
        session_id: "test-session-1".to_string(),
        user_id: "test-user-benchmark".to_string(),
        messages: vec![
            BenchmarkMessage {
                role: "user".to_string(),
                content: "My name is Alice and I work at Google as a software engineer."
                    .to_string(),
                timestamp: now,
            },
            BenchmarkMessage {
                role: "assistant".to_string(),
                content: "Nice to meet you Alice! That's great that you work at Google."
                    .to_string(),
                timestamp: now,
            },
            BenchmarkMessage {
                role: "user".to_string(),
                content:
                    "I love hiking in the mountains on weekends. My favorite trail is in Yosemite."
                        .to_string(),
                timestamp: now,
            },
            BenchmarkMessage {
                role: "assistant".to_string(),
                content: "Yosemite has beautiful trails! Do you go often?".to_string(),
                timestamp: now,
            },
            BenchmarkMessage {
                role: "user".to_string(),
                content: "Yes, about twice a month. I also have a dog named Max who comes with me."
                    .to_string(),
                timestamp: now,
            },
        ],
        metadata: None,
    }
}

/// Create test questions about the session
fn create_test_questions() -> Vec<BenchmarkQuestion> {
    vec![
        BenchmarkQuestion::new(
            "q1",
            "What is the user's name?",
            "Alice",
            QuestionCategory::Extraction,
        ),
        BenchmarkQuestion::new(
            "q2",
            "Where does the user work?",
            "Google",
            QuestionCategory::Extraction,
        ),
        BenchmarkQuestion::new(
            "q3",
            "What is the user's hobby?",
            "Hiking",
            QuestionCategory::Extraction,
        ),
        BenchmarkQuestion::new(
            "q4",
            "What is the name of the user's dog?",
            "Max",
            QuestionCategory::Extraction,
        ),
    ]
}

/// Test the full ingestion pipeline
#[tokio::test]
#[ignore]
async fn test_ingestion_pipeline() {
    let session = create_test_session();
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");

    // Create ingester with real components
    let ingester = SessionIngester::from_benchmark_config(
        IngesterConfig::default().with_extraction_mode("api".to_string()),
        &bench_config,
    )
    .await
    .expect("Failed to create ingester");

    assert!(
        ingester.is_configured(),
        "Ingester should have all components"
    );

    // Ingest the session
    let stats = ingester
        .ingest_session_async(&session)
        .await
        .expect("Ingestion should succeed");

    println!("Ingestion stats:");
    println!("  Facts extracted: {}", stats.facts_extracted);
    println!("  Memories created: {}", stats.memories_created);
    println!("  Entities extracted: {}", stats.entities_extracted);

    assert!(
        stats.facts_extracted > 0,
        "Should extract at least one fact"
    );
    assert!(
        stats.memories_created > 0,
        "Should create at least one memory"
    );
}

/// Test the answer generation pipeline
#[tokio::test]
#[ignore]
async fn test_answer_pipeline() {
    // First ingest some data
    let session = create_test_session();
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");
    let ingester = SessionIngester::from_benchmark_config(
        IngesterConfig::default().with_extraction_mode("api".to_string()),
        &bench_config,
    )
    .await
    .expect("Failed to create ingester");

    ingester
        .ingest_session_async(&session)
        .await
        .expect("Ingestion should succeed");

    // Create answerer with real components
    let answerer = AnswerGenerator::from_benchmark_config(AnswererConfig::default(), &bench_config)
        .await
        .expect("Failed to create answerer");

    assert!(
        answerer.is_configured(),
        "Answerer should have all components"
    );

    // Test answering questions
    let questions = create_test_questions();
    for question in &questions {
        let result = answerer
            .answer_async(question, &session.user_id)
            .await
            .expect("Answer should succeed");

        println!("Question: {}", question.question);
        println!("Expected: {}", question.answer);
        println!("Generated: {}", result.answer);
        println!("Retrieved {} memories", result.retrieved_memories.len());
        println!("Cost: ${:.6}", result.cost_usd);
        println!();
    }
}

/// Test the judge with real LLM
#[tokio::test]
#[ignore]
async fn test_judge_with_llm() {
    let llm_client = LlmClient::from_env().expect("OPENAI_API_KEY must be set");
    let judge =
        Judge::new(JudgeConfig::default().with_model("gpt-4o-mini")).with_llm_client(llm_client);

    // Test judging a correct answer
    let result = judge
        .judge(
            "What is the user's name?",
            "Alice",
            "The user's name is Alice.",
            QuestionCategory::Extraction,
        )
        .expect("Judge should succeed");

    println!("Judge result for correct answer:");
    println!("  Is correct: {}", result.is_correct);
    println!("  Score: {}", result.score);
    println!("  Reasoning: {}", result.reasoning);
    println!("  Cost: ${:.6}", result.cost_usd);

    // Test judging an incorrect answer
    let result = judge
        .judge(
            "What is the user's name?",
            "Alice",
            "The user's name is Bob.",
            QuestionCategory::Extraction,
        )
        .expect("Judge should succeed");

    println!("\nJudge result for incorrect answer:");
    println!("  Is correct: {}", result.is_correct);
    println!("  Score: {}", result.score);
    println!("  Reasoning: {}", result.reasoning);
}

/// Full end-to-end benchmark test
#[tokio::test]
#[ignore]
async fn test_full_benchmark_flow() {
    println!("=== Full Benchmark Flow Test ===\n");

    // 1. Create test data
    let session = create_test_session();
    let questions = create_test_questions();

    println!(
        "1. Ingesting session with {} messages...",
        session.messages.len()
    );

    // 2. Ingest
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");
    let ingester = SessionIngester::from_benchmark_config(
        IngesterConfig::default().with_extraction_mode("api".to_string()),
        &bench_config,
    )
    .await
    .expect("Failed to create ingester");

    let stats = ingester
        .ingest_session_async(&session)
        .await
        .expect("Ingestion should succeed");

    println!(
        "   Extracted {} facts, created {} memories\n",
        stats.facts_extracted, stats.memories_created
    );

    // 3. Answer and judge
    let answerer = AnswerGenerator::from_benchmark_config(AnswererConfig::default(), &bench_config)
        .await
        .expect("Failed to create answerer");

    // Judge uses heuristics by default (no nested runtime issue)
    let judge = Judge::new(JudgeConfig::default().with_model("gpt-4o-mini"));

    println!("2. Processing {} questions...\n", questions.len());

    let mut correct = 0;
    let mut total_cost = 0.0f32;

    for (i, question) in questions.iter().enumerate() {
        // Answer using async method
        let answer_result = answerer
            .answer_async(question, &session.user_id)
            .await
            .expect("Answer should succeed");

        let judge_result = judge
            .judge_async(
                &question.question,
                &question.answer,
                &answer_result.answer,
                question.category,
            )
            .await
            .expect("Judge should succeed");

        if judge_result.is_correct {
            correct += 1;
        }
        total_cost += answer_result.cost_usd + judge_result.cost_usd;

        println!("   Q{}: {}", i + 1, question.question);
        println!("      Expected: {}", question.answer);
        println!("      Generated: {}", answer_result.answer);
        println!(
            "      Correct: {} (score: {:.2})",
            if judge_result.is_correct {
                "✓"
            } else {
                "✗"
            },
            judge_result.score
        );
        println!();
    }

    // 4. Summary
    println!("=== Results ===");
    println!(
        "Accuracy: {}/{} ({:.1}%)",
        correct,
        questions.len(),
        (correct as f32 / questions.len() as f32) * 100.0
    );
    println!("Total cost: ${:.4}", total_cost);
}

/// Mini benchmark with real dataset (3 questions) to verify full pipeline
/// Run with: OPENAI_API_KEY=... cargo test --test integration_benchmark test_mini_benchmark_real_dataset -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_mini_benchmark_real_dataset() {
    use engram_bench::longmemeval::DatasetLoader;

    println!("=== Mini Benchmark with Real Dataset ===\n");

    // Load the real dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);

    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    // Take first 3 questions and their sessions
    let questions: Vec<_> = dataset.questions.into_iter().take(3).collect();

    // Collect session IDs needed for these questions
    let needed_session_ids: std::collections::HashSet<_> = questions
        .iter()
        .flat_map(|q| q.session_ids.iter().cloned())
        .collect();

    // Filter sessions to only those needed
    let sessions: Vec<_> = dataset
        .sessions
        .into_iter()
        .filter(|s| needed_session_ids.contains(&s.session_id))
        .collect();

    println!(
        "Testing with {} questions, {} sessions\n",
        questions.len(),
        sessions.len()
    );

    // Create components
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");
    let ingester = SessionIngester::from_benchmark_config(
        IngesterConfig::default().with_extraction_mode("api".to_string()),
        &bench_config,
    )
    .await
    .expect("Failed to create ingester");

    let answerer = AnswerGenerator::from_benchmark_config(AnswererConfig::default(), &bench_config)
        .await
        .expect("Failed to create answerer");

    let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

    // Ingest sessions using parallel ingestion
    println!(
        "1. Ingesting {} sessions (parallel, concurrency={})...",
        sessions.len(),
        ingester.config().concurrency
    );

    let ingestion_stats = ingester
        .ingest_sessions_async(&sessions)
        .await
        .expect("Ingestion should succeed");

    let total_memories = ingestion_stats.memories_created;
    let total_facts = ingestion_stats.facts_extracted;

    // Log any errors
    for error in &ingestion_stats.errors {
        println!("   Warning: {}", error);
    }

    println!(
        "   Extracted {} facts, created {} memories\n",
        total_facts, total_memories
    );

    // Process questions
    println!("2. Processing {} questions...\n", questions.len());
    let mut correct = 0;
    let mut total_cost = 0.0f32;

    for (i, question) in questions.iter().enumerate() {
        let user_id = format!("user_{}", question.id);

        match answerer.answer_async(&question, &user_id).await {
            Ok(answer_result) => {
                let judge_result = judge
                    .judge_async(
                        &question.question,
                        &question.answer,
                        &answer_result.answer,
                        question.category,
                    )
                    .await
                    .expect("Judge should succeed");

                if judge_result.is_correct {
                    correct += 1;
                }
                total_cost += answer_result.cost_usd;

                println!("   Q{}: {}", i + 1, question.question);
                println!("      Category: {:?}", question.category);
                println!("      Expected: {}", question.answer);
                println!("      Generated: {}", answer_result.answer);
                println!(
                    "      Retrieved memories: {}",
                    answer_result.retrieved_memories.len()
                );
                println!(
                    "      Correct: {} (score: {:.2})",
                    if judge_result.is_correct {
                        "✓"
                    } else {
                        "✗"
                    },
                    judge_result.score
                );
                println!();
            }
            Err(e) => {
                println!("   Q{}: ERROR - {}", i + 1, e);
            }
        }
    }

    println!("=== Results ===");
    println!(
        "Accuracy: {}/{} ({:.1}%)",
        correct,
        questions.len(),
        (correct as f32 / questions.len() as f32) * 100.0
    );
    println!("Total cost: ${:.4}", total_cost);
}

/// Test the dataset loader with real data (no API calls, just parsing)
#[test]
fn test_load_longmemeval_s_dataset() {
    use engram_bench::longmemeval::DatasetLoader;

    // Get the project root directory
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);

    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    println!("=== Dataset Statistics ===");
    println!("Questions: {}", dataset.questions.len());
    println!("Sessions: {}", dataset.sessions.len());

    // Check basic validity
    assert!(!dataset.questions.is_empty(), "Should have questions");
    assert!(!dataset.sessions.is_empty(), "Should have sessions");

    // Print category distribution
    let mut category_counts = std::collections::HashMap::new();
    for q in &dataset.questions {
        *category_counts.entry(q.category).or_insert(0) += 1;
    }
    println!("\nCategory distribution:");
    for (category, count) in category_counts {
        println!("  {:?}: {}", category, count);
    }

    // Sample a question
    if let Some(q) = dataset.questions.first() {
        println!("\nSample question:");
        println!("  ID: {}", q.id);
        println!("  Category: {:?}", q.category);
        println!("  Question: {}", q.question);
        println!("  Answer: {}", q.answer);
        println!("  Session IDs: {:?}", q.session_ids);
    }

    // Sample a session
    if let Some(s) = dataset.sessions.first() {
        println!("\nSample session:");
        println!("  ID: {}", s.session_id);
        println!("  User ID: {}", s.user_id);
        println!("  Messages: {}", s.messages.len());
        if let Some(m) = s.messages.first() {
            println!("  First message role: {}", m.role);
            println!(
                "  First message preview: {}...",
                &m.content.chars().take(100).collect::<String>()
            );
        }
    }
}

/// Run one batch of the full benchmark (for incremental runs)
///
/// Usage: cargo test --test integration_benchmark test_run_batch -- --ignored --nocapture
///
/// This test:
/// 1. Loads/creates a checkpoint file
/// 2. Runs one batch of ingestion OR answering (depending on phase)
/// 3. Saves progress to checkpoint
/// 4. Can be run repeatedly until complete
#[tokio::test]
#[ignore]
async fn test_run_batch() {
    use engram_bench::longmemeval::{BatchConfig, BatchRunner, BenchmarkPhase, DatasetLoader};

    println!("=== Batch Benchmark Runner ===\n");

    // Load dataset
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader.load_longmemeval_s().expect("Should load dataset");

    println!(
        "Dataset: {} sessions, {} questions",
        dataset.sessions.len(),
        dataset.questions.len()
    );

    // Configure for 5 work sessions of ~2.5 hours each
    // Full benchmark: 18,464 sessions / 5 = ~3700 sessions per batch
    let answer_concurrency: usize = std::env::var("ANSWER_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let config = BatchConfig::five_batch_run()
        .with_checkpoint_path("benchmark_checkpoint.json")
        .with_answer_concurrency(answer_concurrency);

    // Create or resume batch runner
    let mut runner = BatchRunner::new(config, dataset.sessions.len(), dataset.questions.len())
        .expect("Should create runner");

    println!("\n{}\n", runner.progress_summary());

    // Check if already complete
    if runner.is_complete() {
        println!("Benchmark already complete!");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
        return;
    }

    // Create components
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");
    // Tier 4: 10M TPM for gpt-4o-mini
    let ingester =
        SessionIngester::from_benchmark_config(IngesterConfig::default().with_concurrency(100), &bench_config)
            .await
            .expect("Failed to create ingester");

    let answerer = AnswerGenerator::from_benchmark_config(AnswererConfig::default(), &bench_config)
        .await
        .expect("Failed to create answerer");

    let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

    // Run appropriate batch based on phase
    match runner.phase() {
        BenchmarkPhase::Ingestion => {
            println!("Running ingestion batch...\n");
            let processed = runner
                .run_ingestion_batch(&dataset.sessions, &ingester)
                .await
                .expect("Ingestion batch should succeed");
            println!("\nProcessed {} sessions in this batch", processed);
        }
        BenchmarkPhase::BatchGenerate
        | BenchmarkPhase::BatchPending
        | BenchmarkPhase::BatchProcessing => {
            println!("Batch mode phases - use test_batch_ingestion for batch API workflow");
            println!("This test uses real-time ingestion mode.");
        }
        BenchmarkPhase::Answering => {
            println!("Running answering batch...\n");
            let answered = runner
                .run_answering_batch(&dataset.questions, &answerer, &judge)
                .await
                .expect("Answering batch should succeed");
            println!("\nAnswered {} questions in this batch", answered);
        }
        BenchmarkPhase::Complete => {
            println!("Benchmark complete!");
        }
    }

    println!("\n{}", runner.progress_summary());

    if runner.is_complete() {
        println!("\n=== BENCHMARK COMPLETE ===");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
    } else {
        println!("\nRun this test again to continue the benchmark.");
    }
}

/// Submit remaining sessions from checkpoint as a batch
///
/// Splits into chunks of 800 sessions to stay under 2M token limit.
/// (800 sessions * ~2000 tokens = ~1.6M tokens, safely under limit)
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark submit_remaining_as_batch -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn submit_remaining_as_batch() {
    use engram_bench::longmemeval::{BenchmarkCheckpoint, DatasetLoader};
    use std::path::Path;

    // OpenAI Batch API limit: 2M enqueued tokens for gpt-4o-mini
    // Testing with small batch first to check quota
    const SESSIONS_PER_BATCH: usize = 100;

    println!("=== Submit Remaining Sessions as Batch ===\n");

    // Load checkpoint
    let checkpoint_path = Path::new("benchmark_checkpoint.json");
    let checkpoint =
        BenchmarkCheckpoint::load_or_create(checkpoint_path, 0, 0).expect("Should load checkpoint");

    println!(
        "Already ingested: {} sessions",
        checkpoint.ingested_sessions.len()
    );

    // Load dataset
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader.load_longmemeval_s().expect("Should load dataset");

    // Filter to remaining sessions
    let remaining: Vec<_> = dataset
        .sessions
        .into_iter()
        .filter(|s| !checkpoint.ingested_sessions.contains(&s.session_id))
        .collect();

    println!("Remaining sessions: {}", remaining.len());

    if remaining.is_empty() {
        println!("No remaining sessions to process!");
        return;
    }

    // Calculate how many batches we need
    let total_remaining = remaining.len();
    let num_batches = (total_remaining + SESSIONS_PER_BATCH - 1) / SESSIONS_PER_BATCH;
    println!(
        "Will submit {} batches of up to {} sessions each",
        num_batches, SESSIONS_PER_BATCH
    );
    println!("(OpenAI limit: 2M enqueued tokens, ~2000 tokens/session)\n");

    // Create ingester
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");
    let ingester = SessionIngester::from_benchmark_config(IngesterConfig::default(), &bench_config)
        .await
        .expect("Failed to create ingester");

    // Take first chunk only (to avoid hitting limits)
    let first_chunk: Vec<_> = remaining.into_iter().take(SESSIONS_PER_BATCH).collect();
    let chunk_size = first_chunk.len();
    let still_remaining = total_remaining - chunk_size;

    // Step 1: Generate JSONL for first chunk
    let batch_path = Path::new("batch_chunk_001.jsonl");
    println!(
        "Step 1: Generating JSONL for chunk 1 ({} sessions)...",
        chunk_size
    );
    let count = ingester
        .generate_batch_file(&first_chunk, batch_path, "gpt-4o-mini")
        .expect("Should generate batch file");
    println!("Generated {} requests in {:?}", count, batch_path);

    // Check file size
    let metadata = std::fs::metadata(batch_path).expect("Should get file metadata");
    println!(
        "File size: {:.2} MB (~{:.1}M tokens estimated)\n",
        metadata.len() as f64 / 1_000_000.0,
        (count as f64 * 2000.0) / 1_000_000.0
    );

    // Step 2: Submit batch
    println!("Step 2: Submitting batch to OpenAI...");
    let batch_id = ingester
        .submit_batch(batch_path)
        .await
        .expect("Should submit batch");

    println!("\n========================================");
    println!("BATCH 1/{} SUBMITTED!", num_batches);
    println!("========================================");
    println!("Batch ID: {}", batch_id);
    println!("Sessions in this batch: {}", count);
    println!("Remaining after this: {}", still_remaining);
    println!("\nTo check status:");
    println!("  curl https://api.openai.com/v1/batches/{} \\", batch_id);
    println!("    -H \"Authorization: Bearer $OPENAI_API_KEY\" | jq .request_counts");
    println!("\nOr visit: https://platform.openai.com/batches");

    // Save batch info
    let batch_info = serde_json::json!({
        "batch_id": batch_id,
        "sessions": count,
        "chunk": 1,
        "total_chunks": num_batches,
        "session_ids": first_chunk.iter().map(|s| &s.session_id).collect::<Vec<_>>()
    });
    std::fs::write(
        "pending_batch.json",
        serde_json::to_string_pretty(&batch_info).unwrap(),
    )
    .expect("Should save batch info");
    println!("\nBatch info saved to pending_batch.json");
    println!("\nOnce complete, run 'process_batch_results' to download and store facts.");
}

/// Automated batch processing loop
///
/// Continuously: process completed batches → submit new batches → wait → repeat
/// Until all sessions are ingested.
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark batch_loop -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn batch_loop() {
    use engram_bench::longmemeval::{BenchmarkCheckpoint, DatasetLoader};
    use engram::embedding::{EmbeddingProvider, RemoteEmbeddingProvider, EMBEDDING_DIMENSION};
    use engram::extraction::{ApiExtractorConfig, BatchClient, BatchExtractor, BatchStatus};
    use engram::storage::{QdrantConfig, QdrantStorage};
    use engram::types::Memory;
    use std::collections::HashMap;
    use std::path::Path;

    let bench_config = BenchmarkConfig::load().expect("Failed to load config");

    const SESSIONS_PER_BATCH: usize = 500;
    const MAX_CONCURRENT_BATCHES: usize = 1; // Stay under quota
    const POLL_INTERVAL_SECS: u64 = 30;

    println!("=== Automated Batch Processing Loop ===");
    println!("Batch size: {} sessions", SESSIONS_PER_BATCH);
    println!("Poll interval: {}s\n", POLL_INTERVAL_SECS);

    // Initialize components
    let client = BatchClient::from_env().expect("OPENAI_API_KEY not set");
    let embedding_provider = RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY not set");
    let qdrant_config = QdrantConfig::external("http://localhost:6334")
        .with_vector_size(EMBEDDING_DIMENSION as u64);
    let storage = QdrantStorage::new(qdrant_config)
        .await
        .expect("Qdrant connection failed");
    storage.initialize().await.expect("Qdrant init failed");
    let batch_extractor = BatchExtractor::new(ApiExtractorConfig::openai("gpt-4o-mini"));

    // Load dataset
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader.load_longmemeval_s().expect("Should load dataset");
    let all_sessions: HashMap<String, _> = dataset
        .sessions
        .into_iter()
        .map(|s| (s.session_id.clone(), s))
        .collect();
    let total_sessions = all_sessions.len();

    // Track pending batches: batch_id -> session_ids
    let mut pending_batches: HashMap<String, Vec<String>> = HashMap::new();

    // Load any existing pending batch
    if let Ok(content) = std::fs::read_to_string("pending_batch.json") {
        if let Ok(info) = serde_json::from_str::<serde_json::Value>(&content) {
            if let (Some(batch_id), Some(session_ids)) =
                (info["batch_id"].as_str(), info["session_ids"].as_array())
            {
                let ids: Vec<String> = session_ids
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                pending_batches.insert(batch_id.to_string(), ids);
                println!("Loaded pending batch: {}", batch_id);
            }
        }
    }

    loop {
        // Load checkpoint
        let checkpoint_path = Path::new("benchmark_checkpoint.json");
        let mut checkpoint =
            BenchmarkCheckpoint::load_or_create(checkpoint_path, total_sessions, 0)
                .expect("Should load checkpoint");

        let ingested_count = checkpoint.ingested_sessions.len();
        let remaining = total_sessions - ingested_count;

        println!(
            "\n--- Status: {}/{} ingested ({} remaining) ---",
            ingested_count, total_sessions, remaining
        );

        if remaining == 0 && pending_batches.is_empty() {
            println!("\n========================================");
            println!("ALL SESSIONS INGESTED!");
            println!("========================================");
            break;
        }

        // Check status of pending batches
        let mut completed_batches = Vec::new();
        for (batch_id, session_ids) in &pending_batches {
            match client.get_batch_status(batch_id).await {
                Ok(result) => {
                    match result.status {
                        BatchStatus::Completed { output_file_id, .. } => {
                            println!("Batch {} COMPLETED", batch_id);
                            completed_batches.push((
                                batch_id.clone(),
                                session_ids.clone(),
                                output_file_id,
                            ));
                        }
                        BatchStatus::InProgress {
                            completed, total, ..
                        } => {
                            println!("Batch {} in progress: {}/{}", batch_id, completed, total);
                        }
                        BatchStatus::Failed { error } => {
                            println!("Batch {} FAILED: {}", batch_id, error);
                            // Remove failed batch
                            completed_batches.push((batch_id.clone(), vec![], String::new()));
                        }
                        _ => {
                            println!("Batch {} status: {:?}", batch_id, result.status);
                        }
                    }
                }
                Err(e) => println!("Error checking batch {}: {}", batch_id, e),
            }
        }

        // Process completed batches
        for (batch_id, session_ids, output_file_id) in completed_batches {
            pending_batches.remove(&batch_id);

            if output_file_id.is_empty() {
                continue; // Failed batch, skip processing
            }

            println!(
                "Processing batch {} ({} sessions)...",
                batch_id,
                session_ids.len()
            );

            // Download results
            match client.get_results(&output_file_id).await {
                Ok(results) => {
                    let mut facts_stored = 0;
                    let mut sessions_done = 0;

                    // Collect all facts first for batch embedding
                    let mut all_facts: Vec<(String, String, String)> = Vec::new(); // (session_id, user_id, fact_content)
                    let mut processed_sessions = std::collections::HashSet::new();

                    for line in results.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }

                        if let Ok((session_id, facts)) = batch_extractor.parse_result(line) {
                            if let Some(session) = all_sessions.get(&session_id) {
                                for fact in &facts {
                                    all_facts.push((
                                        session.session_id.clone(),
                                        session.user_id.clone(),
                                        fact.content.clone(),
                                    ));
                                }
                                processed_sessions.insert(session_id);
                            }
                        }
                    }

                    println!(
                        "  Collected {} facts from {} sessions, batch embedding...",
                        all_facts.len(),
                        processed_sessions.len()
                    );

                    // Batch embed in chunks of 100
                    const EMBED_BATCH_SIZE: usize = 100;
                    for chunk in all_facts.chunks(EMBED_BATCH_SIZE) {
                        let texts: Vec<String> = chunk
                            .iter()
                            .map(|(_, _, content)| content.clone())
                            .collect();

                        match embedding_provider.embed_batch(&texts).await {
                            Ok(embeddings) => {
                                for ((session_id, user_id, content), embedding) in
                                    chunk.iter().zip(embeddings.into_iter())
                                {
                                    let memory = Memory::new(user_id, content)
                                        .with_session(session_id.clone());

                                    if storage.upsert_memory(&memory, embedding).await.is_ok() {
                                        facts_stored += 1;
                                    }
                                }
                            }
                            Err(e) => println!("  Batch embedding error: {}", e),
                        }
                    }

                    // Mark sessions as ingested
                    for session_id in processed_sessions {
                        checkpoint.ingested_sessions.insert(session_id);
                        sessions_done += 1;
                    }

                    println!(
                        "  Stored {} facts from {} sessions",
                        facts_stored, sessions_done
                    );
                    checkpoint
                        .save(checkpoint_path)
                        .expect("Should save checkpoint");

                    // Clean up output file
                    let _ = reqwest::Client::new()
                        .delete(format!(
                            "https://api.openai.com/v1/files/{}",
                            output_file_id
                        ))
                        .header(
                            "Authorization",
                            format!("Bearer {}", std::env::var("OPENAI_API_KEY").unwrap()),
                        )
                        .send()
                        .await;
                }
                Err(e) => println!("  Error downloading results: {}", e),
            }
        }

        // Submit new batches if under limit
        let remaining_sessions: Vec<_> = all_sessions
            .keys()
            .filter(|id| !checkpoint.ingested_sessions.contains(*id))
            .filter(|id| !pending_batches.values().any(|ids| ids.contains(*id)))
            .take(SESSIONS_PER_BATCH)
            .cloned()
            .collect();

        if !remaining_sessions.is_empty() && pending_batches.len() < MAX_CONCURRENT_BATCHES {
            println!(
                "\nSubmitting new batch with {} sessions...",
                remaining_sessions.len()
            );

            // Generate JSONL
            let batch_sessions: Vec<_> = remaining_sessions
                .iter()
                .filter_map(|id| all_sessions.get(id).cloned())
                .collect();

            let ingester = SessionIngester::from_benchmark_config(IngesterConfig::default(), &bench_config)
                .await
                .expect("Failed to create ingester");

            let batch_path = Path::new("batch_current.jsonl");
            if let Ok(count) =
                ingester.generate_batch_file(&batch_sessions, batch_path, "gpt-4o-mini")
            {
                match ingester.submit_batch(batch_path).await {
                    Ok(batch_id) => {
                        println!("Submitted batch: {} ({} sessions)", batch_id, count);
                        pending_batches.insert(batch_id.clone(), remaining_sessions);

                        // Save pending batch info
                        let info = serde_json::json!({
                            "batch_id": batch_id,
                            "session_ids": pending_batches.get(&batch_id).unwrap()
                        });
                        std::fs::write(
                            "pending_batch.json",
                            serde_json::to_string_pretty(&info).unwrap(),
                        )
                        .ok();
                    }
                    Err(e) => println!("Failed to submit batch: {}", e),
                }
            }
        }

        if pending_batches.is_empty() && remaining == 0 {
            break;
        }

        // Wait before next poll
        println!("\nWaiting {}s before next check...", POLL_INTERVAL_SECS);
        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }

    // Cleanup
    let _ = std::fs::remove_file("pending_batch.json");
    let _ = std::fs::remove_file("batch_current.jsonl");
    println!("\nBatch loop complete!");
}

/// Process completed batch results and optionally submit next batch
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark process_and_continue -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn process_and_continue() {
    use engram_bench::longmemeval::{BenchmarkCheckpoint, DatasetLoader};
    use engram::embedding::{EmbeddingProvider, RemoteEmbeddingProvider};
    use engram::extraction::{ApiExtractorConfig, BatchClient, BatchExtractor, BatchStatus};
    use engram::storage::{QdrantConfig, QdrantStorage};
    use engram::types::Memory;
    use std::collections::HashMap;
    use std::path::Path;

    println!("=== Process Batch Results and Continue ===\n");

    // Load pending batch info
    let batch_info: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("pending_batch.json").expect("pending_batch.json not found"),
    )
    .expect("Invalid JSON");

    let batch_id = batch_info["batch_id"].as_str().expect("No batch_id");
    let session_ids: Vec<String> = batch_info["session_ids"]
        .as_array()
        .expect("No session_ids")
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    println!("Batch ID: {}", batch_id);
    println!("Sessions in batch: {}", session_ids.len());

    // Check batch status
    let client = BatchClient::from_env().expect("OPENAI_API_KEY not set");
    let status = client
        .get_batch_status(batch_id)
        .await
        .expect("Failed to get status");

    match status.status {
        BatchStatus::Completed { output_file_id, .. } => {
            println!("Status: COMPLETED");
            println!("Output file: {}\n", output_file_id);

            // Download results
            println!("Downloading results...");
            let results = client
                .get_results(&output_file_id)
                .await
                .expect("Failed to download");
            println!("Downloaded {} bytes\n", results.len());

            // Load dataset to get session details
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
            let loader = DatasetLoader::new().with_data_dir(&data_dir);
            let dataset = loader.load_longmemeval_s().expect("Should load dataset");

            // Build session lookup
            let session_lookup: HashMap<String, _> = dataset
                .sessions
                .into_iter()
                .filter(|s| session_ids.contains(&s.session_id))
                .map(|s| (s.session_id.clone(), s))
                .collect();

            // Initialize storage
            let embedding_provider =
                RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY not set");
            let qdrant_config =
                QdrantConfig::external("http://localhost:6334").with_vector_size(1536);
            let storage = QdrantStorage::new(qdrant_config)
                .await
                .expect("Qdrant connection failed");
            storage.initialize().await.expect("Qdrant init failed");

            // Parse and store results
            let batch_extractor = BatchExtractor::new(ApiExtractorConfig::openai("gpt-4o-mini"));
            let mut facts_stored = 0;
            let mut sessions_processed = 0;
            let mut errors = Vec::new();

            for line in results.lines() {
                if line.trim().is_empty() {
                    continue;
                }

                match batch_extractor.parse_result(line) {
                    Ok((session_id, facts)) => {
                        let session = match session_lookup.get(&session_id) {
                            Some(s) => s,
                            None => {
                                errors.push(format!("Session {} not in lookup", session_id));
                                continue;
                            }
                        };

                        for fact in &facts {
                            let memory = Memory::new(&session.user_id, &fact.content)
                                .with_session(session.session_id.clone());

                            match embedding_provider.embed_document(&fact.content).await {
                                Ok(embedding) => {
                                    if let Err(e) = storage.upsert_memory(&memory, embedding).await
                                    {
                                        errors.push(format!("Storage error: {}", e));
                                    } else {
                                        facts_stored += 1;
                                    }
                                }
                                Err(e) => errors.push(format!("Embedding error: {}", e)),
                            }
                        }
                        sessions_processed += 1;
                    }
                    Err(e) => errors.push(format!("Parse error: {}", e)),
                }
            }

            println!(
                "Processed {} sessions, stored {} facts",
                sessions_processed, facts_stored
            );
            if !errors.is_empty() {
                println!("Errors: {}", errors.len());
                for e in errors.iter().take(5) {
                    println!("  - {}", e);
                }
            }

            // Update checkpoint
            let checkpoint_path = Path::new("benchmark_checkpoint.json");
            let mut checkpoint = BenchmarkCheckpoint::load_or_create(checkpoint_path, 0, 0)
                .expect("Should load checkpoint");

            for session_id in &session_ids {
                checkpoint.ingested_sessions.insert(session_id.clone());
            }
            checkpoint
                .save(checkpoint_path)
                .expect("Should save checkpoint");
            println!(
                "\nCheckpoint updated: {} total sessions ingested",
                checkpoint.ingested_sessions.len()
            );

            // Clean up batch file
            if let Err(e) = std::fs::remove_file("pending_batch.json") {
                println!("Note: Could not remove pending_batch.json: {}", e);
            }

            // Delete the output file to free quota
            println!("\nCleaning up OpenAI files...");
            let _ = reqwest::Client::new()
                .delete(format!(
                    "https://api.openai.com/v1/files/{}",
                    output_file_id
                ))
                .header(
                    "Authorization",
                    format!("Bearer {}", std::env::var("OPENAI_API_KEY").unwrap()),
                )
                .send()
                .await;

            println!("\n========================================");
            println!("BATCH PROCESSED SUCCESSFULLY!");
            println!("========================================");
            println!("Run submit_remaining_as_batch again to continue.");
        }
        BatchStatus::InProgress {
            completed, total, ..
        } => {
            println!("Status: IN PROGRESS ({}/{})", completed, total);
            println!("Please wait for batch to complete.");
        }
        BatchStatus::Failed { error } => {
            println!("Status: FAILED - {}", error);
        }
        _ => {
            println!("Status: {:?}", status.status);
        }
    }
}

/// Test the batch API ingestion workflow
///
/// This test demonstrates the 3-step batch workflow:
/// 1. Generate JSONL batch file
/// 2. Submit to OpenAI Batch API
/// 3. Poll and process results
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark test_batch_ingestion -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_batch_ingestion() {
    use engram_bench::longmemeval::DatasetLoader;
    use std::collections::HashMap;
    use std::path::Path;

    println!("=== Batch API Ingestion Test ===\n");
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");

    // Load dataset
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader.load_longmemeval_s().expect("Should load dataset");

    // Take a small subset for testing (100 sessions)
    let sessions: Vec<_> = dataset.sessions.into_iter().take(100).collect();
    println!("Testing with {} sessions\n", sessions.len());

    // Create ingester
    let ingester = SessionIngester::from_benchmark_config(IngesterConfig::default(), &bench_config)
        .await
        .expect("Failed to create ingester");

    // Step 1: Generate JSONL batch file
    println!("Step 1: Generating JSONL batch file...");
    let batch_path = Path::new("/tmp/engram_batch_test.jsonl");
    let count = ingester
        .generate_batch_file(&sessions, batch_path, "gpt-4o-mini")
        .expect("Should generate batch file");
    println!("Generated {} requests in {:?}\n", count, batch_path);

    // Step 2: Submit batch to OpenAI
    println!("Step 2: Submitting batch to OpenAI...");
    let batch_id = ingester
        .submit_batch(batch_path)
        .await
        .expect("Should submit batch");
    println!("Submitted batch: {}\n", batch_id);

    // Step 3: Poll for completion
    // Note: In production you'd save the batch_id and poll periodically
    // For this test, we'll poll a few times then exit
    println!("Step 3: Polling for completion...");
    println!("(Batch API jobs typically take 15-60 minutes for large batches)");
    println!("(For testing, we'll check status a few times then exit)\n");

    // Create session lookup for processing
    let session_lookup: HashMap<String, BenchmarkSession> = sessions
        .into_iter()
        .map(|s| (s.session_id.clone(), s))
        .collect();

    // Poll a few times
    for i in 1..=3 {
        println!("Poll attempt {}...", i);

        let result = ingester
            .poll_and_process_batch(&batch_id, &session_lookup, "gpt-4o-mini")
            .await
            .expect("Should poll batch");

        match result {
            engram_bench::longmemeval::IngesterBatchPollResult::InProgress {
                completed,
                failed,
                total,
            } => {
                println!(
                    "  Status: In Progress - {}/{} completed, {} failed",
                    completed, total, failed
                );
            }
            engram_bench::longmemeval::IngesterBatchPollResult::Completed {
                sessions_processed,
                facts_extracted,
                errors,
            } => {
                println!("  Status: COMPLETED!");
                println!("  Sessions processed: {}", sessions_processed);
                println!("  Facts extracted: {}", facts_extracted);
                println!("  Errors: {}", errors.len());
                if !errors.is_empty() {
                    println!("  Sample errors:");
                    for error in errors.iter().take(5) {
                        println!("    - {}", error);
                    }
                }
                return; // Success!
            }
            engram_bench::longmemeval::IngesterBatchPollResult::Failed { error } => {
                println!("  Status: FAILED - {}", error);
                return;
            }
        }

        // Wait before next poll
        if i < 3 {
            println!("  Waiting 30 seconds before next poll...\n");
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    }

    println!("\nBatch not yet complete after 3 polls.");
    println!("Batch ID: {}", batch_id);
    println!("To continue processing, save this batch ID and poll later.");
    println!("You can check status at: https://platform.openai.com/batches");
}

/// Mini benchmark using OpenAI Batch API (50% cheaper)
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark test_mini_batch_api -- --ignored --nocapture
///
/// This test:
/// 1. Loads a small subset of the dataset (100 sessions)
/// 2. Submits to OpenAI Batch API
/// 3. Polls until complete (may take minutes to hours)
/// 4. Runs question answering phase
#[tokio::test]
#[ignore]
async fn test_mini_batch_api() {
    use engram_bench::longmemeval::{BatchConfig, BatchRunner, BenchmarkPhase, DatasetLoader};
    use std::time::Duration;

    println!("=== Mini Benchmark with Batch API (50% cheaper) ===\n");
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");

    // Load the real dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);

    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    // Take first 100 sessions and related questions for mini benchmark
    let sessions: Vec<_> = dataset.sessions.into_iter().take(100).collect();
    let session_ids: std::collections::HashSet<_> =
        sessions.iter().map(|s| s.session_id.clone()).collect();

    // Get questions that reference these sessions
    let questions: Vec<_> = dataset
        .questions
        .into_iter()
        .filter(|q| q.session_ids.iter().any(|sid| session_ids.contains(sid)))
        .collect();

    println!(
        "Mini benchmark: {} sessions, {} questions\n",
        sessions.len(),
        questions.len()
    );

    // Configure for Batch API mode
    let config = BatchConfig::default()
        .batch_api_mode()
        .with_checkpoint_path("mini_batch_api_checkpoint.json")
        .with_batch_files_dir("batch_files");

    // Create or resume batch runner
    let mut runner =
        BatchRunner::new(config, sessions.len(), questions.len()).expect("Should create runner");

    println!("{}\n", runner.progress_summary());

    // Check if already complete
    if runner.is_complete() {
        println!("Benchmark already complete!");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
        return;
    }

    // Create ingester
    let ingester = SessionIngester::from_benchmark_config(IngesterConfig::default(), &bench_config)
        .await
        .expect("Failed to create ingester");

    // Ingestion phase - submit and poll batch API
    if runner.phase() == BenchmarkPhase::Ingestion {
        println!("=== Ingestion Phase (Batch API) ===\n");

        loop {
            let processed = runner
                .run_ingestion_batch(&sessions, &ingester)
                .await
                .expect("Ingestion batch should succeed");

            println!("\n{}\n", runner.progress_summary());

            // If we have a pending batch, poll every 30 seconds
            if runner.checkpoint().has_pending_batch() {
                println!("Batch submitted. Polling every 30 seconds...\n");
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }

            // If processed > 0, batch completed
            if processed > 0 {
                println!("Batch processing complete!\n");
            }

            // Check if we've moved to answering phase
            if runner.phase() != BenchmarkPhase::Ingestion {
                break;
            }

            // If no pending batch and no progress, something's wrong
            if processed == 0 && !runner.checkpoint().has_pending_batch() {
                break;
            }
        }
    }

    // Answering phase
    if runner.phase() == BenchmarkPhase::Answering {
        // Agentic mode is on by default; set AGENTIC=0 to disable
        let agentic = std::env::var("AGENTIC").unwrap_or("1".to_string()) == "1";
        let rerank = std::env::var("LLM_RERANK").unwrap_or_default() == "1";
        let mmr_lambda: f32 = std::env::var("MMR_LAMBDA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);

        let mut answerer_config = if agentic {
            println!("=== Answering Phase (AGENTIC) ===\n");
            AnswererConfig::default()
                .with_agentic(true)
                .with_max_iterations(10)
        } else {
            println!("=== Answering Phase ===\n");
            AnswererConfig::default()
        };

        if rerank {
            println!("LLM reranking: ENABLED");
            answerer_config = answerer_config.with_llm_reranking(true);
        }
        if mmr_lambda < 1.0 {
            println!("MMR lambda: {}", mmr_lambda);
            answerer_config = answerer_config.with_mmr_lambda(mmr_lambda);
        }

        let answerer = AnswerGenerator::from_benchmark_config(answerer_config, &bench_config)
            .await
            .expect("Failed to create answerer");

        let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

        loop {
            let answered = runner
                .run_answering_batch(&questions, &answerer, &judge)
                .await
                .expect("Answering batch should succeed");

            println!("\n{}\n", runner.progress_summary());

            if answered == 0 || runner.phase() == BenchmarkPhase::Complete {
                break;
            }
        }
    }

    // Final results
    if runner.is_complete() {
        println!("=== BENCHMARK COMPLETE ===\n");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
    } else {
        println!(
            "Benchmark not complete. Current phase: {:?}",
            runner.phase()
        );
        println!("Run again to continue.");
    }
}

/// Full LongMemEval-S benchmark using OpenAI Batch API (50% cheaper)
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark test_full_batch_api -- --ignored --nocapture
///
/// This test:
/// 1. Loads the full LongMemEval-S dataset (~500 sessions, ~500 questions)
/// 2. Submits all sessions to OpenAI Batch API in chunks
/// 3. Polls until complete (may take 10-30 minutes)
/// 4. Runs question answering phase
/// 5. Reports final accuracy
///
/// Can be interrupted and resumed - progress is saved to checkpoint file.
#[tokio::test]
#[ignore]
async fn test_full_batch_api() {
    use engram_bench::longmemeval::{BatchConfig, BatchRunner, BenchmarkPhase, DatasetLoader};
    use std::time::Duration;

    println!("=== Full LongMemEval-S Benchmark with Batch API (50% cheaper) ===\n");
    let bench_config = BenchmarkConfig::load().expect("Failed to load config");

    // Load the full dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);

    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    println!(
        "Full dataset: {} sessions, {} questions\n",
        dataset.sessions.len(),
        dataset.questions.len()
    );

    // Configure for Batch API mode
    let answer_concurrency: usize = std::env::var("ANSWER_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let config = BatchConfig::default()
        .batch_api_mode()
        .with_checkpoint_path("full_batch_api_checkpoint.json")
        .with_batch_files_dir("batch_files")
        .with_answer_concurrency(answer_concurrency);

    // Create or resume batch runner
    let mut runner = BatchRunner::new(config, dataset.sessions.len(), dataset.questions.len())
        .expect("Should create runner");

    println!("{}\n", runner.progress_summary());

    // Check if already complete
    if runner.is_complete() {
        println!("Benchmark already complete!");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
        return;
    }

    // Create ingester
    let ingester = SessionIngester::from_benchmark_config(IngesterConfig::default(), &bench_config)
        .await
        .expect("Failed to create ingester");

    // Ingestion phase - submit and poll batch API
    // Keep looping until we transition to Answering phase
    while runner.phase() == BenchmarkPhase::Ingestion
        || runner.phase() == BenchmarkPhase::BatchGenerate
        || runner.phase() == BenchmarkPhase::BatchPending
    {
        if runner.phase() == BenchmarkPhase::Ingestion
            || runner.phase() == BenchmarkPhase::BatchGenerate
        {
            println!(
                "=== Ingestion Phase (Batch API) - Batch {} ===\n",
                runner.checkpoint().ingested_sessions.len() / 500 + 1
            );
        }

        let processed = runner
            .run_ingestion_batch(&dataset.sessions, &ingester)
            .await
            .expect("Ingestion batch should succeed");

        println!("\n{}\n", runner.progress_summary());

        // If we have a pending batch, poll every 30 seconds
        if runner.checkpoint().has_pending_batch() {
            println!("Batch submitted. Polling every 30 seconds...\n");
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }

        // If processed > 0, batch completed
        if processed > 0 {
            println!("Batch processing complete!\n");
        }

        // Check if we've moved to answering phase
        if runner.phase() == BenchmarkPhase::Answering || runner.phase() == BenchmarkPhase::Complete
        {
            break;
        }

        // If no pending batch and no progress, something's wrong
        if processed == 0 && !runner.checkpoint().has_pending_batch() {
            println!("Warning: No progress made and no pending batch. Breaking loop.");
            break;
        }
    }

    // Answering phase
    if runner.phase() == BenchmarkPhase::Answering {
        println!("=== Answering Phase ===\n");

        let answerer = AnswerGenerator::from_benchmark_config(AnswererConfig::default(), &bench_config)
            .await
            .expect("Failed to create answerer");

        let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

        loop {
            let answered = runner
                .run_answering_batch(&dataset.questions, &answerer, &judge)
                .await
                .expect("Answering batch should succeed");

            println!("\n{}\n", runner.progress_summary());

            if answered == 0 || runner.phase() == BenchmarkPhase::Complete {
                break;
            }
        }
    }

    // Final results
    if runner.is_complete() {
        println!("=== BENCHMARK COMPLETE ===\n");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
    } else {
        println!(
            "Benchmark not complete. Current phase: {:?}",
            runner.phase()
        );
        println!("Run again to continue.");
    }
}

/// Validation benchmark: ~50 stratified questions for fast feedback loop
///
/// Uses deterministic stratified sampling (seed=42) to select ~50 questions
/// proportionally across all 5 categories. Runs full pipeline: ingest → answer → judge.
///
/// Run with: OPENAI_API_KEY=... cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_validation_benchmark() {
    use engram_bench::longmemeval::{BatchConfig, BatchRunner, BenchmarkPhase, DatasetLoader};
    use engram::embedding::EMBEDDING_DIMENSION;
    use engram::storage::{QdrantConfig, QdrantStorage};

    // Load runtime config (TOML + env overrides)
    let bench_config = BenchmarkConfig::load().expect("Failed to load benchmark config");
    let qdrant_url = bench_config.benchmark.qdrant_url.clone();

    // Parse ingestion mode: INGESTION=full|skip|incremental (default: full)
    // Backward compat: SKIP_INGESTION=1 maps to INGESTION=skip
    let ingestion_mode = if std::env::var("SKIP_INGESTION").unwrap_or_default() == "1" {
        "skip".to_string()
    } else {
        std::env::var("INGESTION")
            .unwrap_or_else(|_| "full".to_string())
            .to_lowercase()
    };
    assert!(
        ["full", "skip", "incremental", "additive"].contains(&ingestion_mode.as_str()),
        "INGESTION must be one of: full, skip, incremental, additive (got '{}')",
        ingestion_mode
    );
    let skip_ingestion = ingestion_mode == "skip";

    println!("=== Validation Benchmark (~50 questions, stratified) ===");
    println!("  Ingestion mode: {}\n", ingestion_mode.to_uppercase());

    // Clean state: delete stale checkpoint and clear Qdrant (unless skipping/incremental)
    let full_benchmark = std::env::var("FULL_BENCHMARK").unwrap_or_default() == "1";
    let checkpoint_path = if full_benchmark {
        "full_benchmark_checkpoint.json"
    } else {
        "validation_checkpoint.json"
    };
    if ingestion_mode == "full" {
        if std::path::Path::new(checkpoint_path).exists() {
            std::fs::remove_file(checkpoint_path).expect("Failed to delete checkpoint");
            println!("Deleted stale checkpoint");
        }

        let qdrant_config = QdrantConfig::external(&qdrant_url)
            .with_vector_size(EMBEDDING_DIMENSION as u64);
        let storage = QdrantStorage::new(qdrant_config)
            .await
            .expect(&format!("Qdrant connection failed — is Qdrant running at {}?", qdrant_url));
        storage
            .clear_all_collections()
            .await
            .expect("Failed to clear Qdrant collections");
        println!("Qdrant cleared for clean run.\n");
    } else if std::path::Path::new(checkpoint_path).exists() {
        std::fs::remove_file(checkpoint_path).expect("Failed to delete checkpoint");
        println!("Deleted stale checkpoint (keeping Qdrant data)\n");
    }

    // Load the full dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);

    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    // Question selection: QUESTION_IDS > FULL_BENCHMARK > stratified 50q
    // QUESTION_IDS=@path/to/file.txt or QUESTION_IDS=1,2,3,... (1-indexed)
    let question_ids_env = std::env::var("QUESTION_IDS").ok();
    let sampled_questions = if let Some(ref ids_spec) = question_ids_env {
        let id_list: Vec<usize> = if ids_spec.starts_with('@') {
            // Read IDs from file (one per line)
            // Resolve relative paths from workspace root (cargo test runs from CARGO_MANIFEST_DIR)
            let raw_path = &ids_spec[1..];
            let path = if std::path::Path::new(raw_path).is_absolute() {
                raw_path.to_string()
            } else {
                let workspace_root = format!("{}/../..", manifest_dir);
                format!("{}/{}", workspace_root, raw_path)
            };
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read QUESTION_IDS file {}: {}", path, e));
            content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    l.trim()
                        .parse::<usize>()
                        .unwrap_or_else(|e| panic!("Invalid question ID '{}': {}", l, e))
                })
                .collect()
        } else {
            // Comma-separated IDs
            ids_spec
                .split(',')
                .map(|s| {
                    s.trim()
                        .parse::<usize>()
                        .unwrap_or_else(|e| panic!("Invalid question ID '{}': {}", s, e))
                })
                .collect()
        };
        // Filter questions by 1-indexed position
        let selected: Vec<_> = dataset
            .questions
            .iter()
            .enumerate()
            .filter(|(i, _)| id_list.contains(&(i + 1)))
            .map(|(_, q)| q.clone())
            .collect();
        println!(
            "QUESTION_IDS MODE: selected {}/{} questions from ID list\n",
            selected.len(),
            id_list.len()
        );
        selected
    } else if full_benchmark {
        println!(
            "FULL BENCHMARK MODE: running all {} questions\n",
            dataset.questions.len()
        );
        dataset.questions.clone()
    } else {
        // Stratified sample: ~50 questions proportional to category distribution
        stratified_sample(&dataset.questions, 50, 42)
    };

    // Collect session IDs needed for these questions
    let needed_session_ids: std::collections::HashSet<_> = sampled_questions
        .iter()
        .flat_map(|q| q.session_ids.iter().cloned())
        .collect();

    // Filter sessions to only those needed
    let sessions: Vec<_> = dataset
        .sessions
        .into_iter()
        .filter(|s| needed_session_ids.contains(&s.session_id))
        .collect();

    // Print category distribution
    let mut category_counts = std::collections::HashMap::new();
    for q in &sampled_questions {
        *category_counts.entry(q.category).or_insert(0usize) += 1;
    }
    println!(
        "Selected {} questions, {} sessions",
        sampled_questions.len(),
        sessions.len()
    );
    println!("Category distribution:");
    for cat in QuestionCategory::all() {
        println!("  {:?}: {}", cat, category_counts.get(&cat).unwrap_or(&0));
    }
    println!();

    // --- Manifest check (on skip/incremental) ---
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = format!(
        "{}/../../data/longmemeval/.qdrant_manifest.json",
        manifest_dir
    );
    if ingestion_mode != "full" {
        if let Ok(manifest_content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&manifest_content) {
                let manifest_qids = manifest
                    .get("question_ids_file")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let manifest_qcount = manifest
                    .get("question_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let manifest_ts = manifest
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let current_qids = question_ids_env.as_deref().unwrap_or(if full_benchmark {
                    "FULL_BENCHMARK"
                } else {
                    "stratified_50"
                });
                println!("=== Ingestion Manifest ===");
                println!("  Ingested at: {}", manifest_ts);
                println!(
                    "  Question set: {} ({} questions)",
                    manifest_qids, manifest_qcount
                );
                if manifest_qids != current_qids {
                    println!("  WARNING: Qdrant was ingested for '{}' ({}q) but you're answering '{}' ({}q). Data may be missing!",
                        manifest_qids, manifest_qcount, current_qids, sampled_questions.len());
                }
                println!();
            }
        } else {
            println!("WARNING: No ingestion manifest found at {}. Cannot verify Qdrant data provenance.\n", manifest_path);
        }
    }

    // --- Pre-flight data verification ---
    // Sample user_ids from selected questions and verify they have data in Qdrant
    // Only abort for "skip" mode; "incremental" will add missing data in the ingestion phase
    if ingestion_mode == "skip" {
        let qdrant_config = QdrantConfig::external(&qdrant_url)
            .with_vector_size(EMBEDDING_DIMENSION as u64);
        let storage = QdrantStorage::new(qdrant_config)
            .await
            .expect(&format!("Qdrant connection failed — is Qdrant running at {}?", qdrant_url));

        // Get unique user_ids for selected questions
        let user_ids: Vec<String> = sampled_questions
            .iter()
            .map(|q| format!("user_{}", q.id))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        // Sample up to 10 users to check
        let sample_size = user_ids.len().min(10);
        let sampled_users: Vec<_> = user_ids.iter().take(sample_size).collect();

        println!(
            "Pre-flight check: verifying {} of {} users have data in Qdrant...",
            sample_size,
            user_ids.len()
        );
        let mut verified = 0u64;
        let mut total_points = 0u64;
        let mut missing_users = Vec::new();

        for uid in &sampled_users {
            let count = storage
                .count_user_points(uid)
                .await
                .expect("Failed to count user points");
            if count == 0 {
                missing_users.push(uid.as_str());
            } else {
                verified += 1;
                total_points += count;
            }
        }

        if !missing_users.is_empty() {
            panic!(
                "ABORT: Pre-flight check failed! {} of {} sampled users have 0 points in Qdrant.\n\
                 Missing users: {:?}\n\
                 Questions need data for {} users. Run without INGESTION=skip or use INGESTION=incremental to ingest missing data.",
                missing_users.len(), sample_size, missing_users, user_ids.len()
            );
        }

        let avg_points = if verified > 0 {
            total_points / verified
        } else {
            0
        };
        println!(
            "  Pre-flight PASSED: {}/{} users verified, ~{} points/user\n",
            verified, sample_size, avg_points
        );
    }

    // Configure batch runner — use BATCH_API=1 for batch API mode (separate rate limits, 50% cheaper)
    let use_batch_api = std::env::var("BATCH_API").unwrap_or_default() == "1";
    let answer_concurrency = bench_config.benchmark.answer_concurrency;
                       // For full benchmark, process all questions in one batch
    let questions_per_batch = if full_benchmark { 600 } else { 100 };
    let mut config = if use_batch_api {
        println!("Using Batch API mode for ingestion\n");
        BatchConfig::default()
            .batch_api_mode()
            .with_checkpoint_path(checkpoint_path)
            .with_batch_files_dir("validation_batch_files")
            .with_answer_concurrency(answer_concurrency)
    } else {
        BatchConfig::default()
            .with_checkpoint_path(checkpoint_path)
            .with_answer_concurrency(answer_concurrency)
    };
    config.questions_per_batch = questions_per_batch;

    // Create or resume batch runner
    let mut runner = BatchRunner::new(config, sessions.len(), sampled_questions.len())
        .expect("Should create runner");

    // Skip directly to answering phase when ingestion_mode=skip
    if skip_ingestion {
        runner.force_answering_phase();
        println!("Forced to Answering phase (skipping ingestion)\n");
    }

    println!("{}\n", runner.progress_summary());

    // Check if already complete
    if runner.is_complete() {
        println!("Validation benchmark already complete!");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
        return;
    }

    // Ingestion phase (skipped when ingestion_mode=skip)
    if runner.phase() == BenchmarkPhase::Ingestion
        || runner.phase() == BenchmarkPhase::BatchGenerate
        || runner.phase() == BenchmarkPhase::BatchPending
    {
        // For incremental mode: filter sessions to only those for users missing from Qdrant
        let ingestion_sessions = if ingestion_mode == "incremental" {
            let qdrant_config = QdrantConfig::external(&qdrant_url)
                .with_vector_size(EMBEDDING_DIMENSION as u64);
            let storage = QdrantStorage::new(qdrant_config)
                .await
                .expect("Qdrant connection failed");

            // Get unique user_ids needed
            let user_ids: std::collections::HashSet<String> = sampled_questions
                .iter()
                .map(|q| format!("user_{}", q.id))
                .collect();

            // Check which users already have data
            let mut missing_user_ids = std::collections::HashSet::new();
            let mut existing_count = 0u64;
            for uid in &user_ids {
                let count = storage
                    .count_user_points(uid)
                    .await
                    .expect("Failed to count user points");
                if count == 0 {
                    missing_user_ids.insert(uid.clone());
                } else {
                    existing_count += 1;
                }
            }

            if missing_user_ids.is_empty() {
                println!(
                    "INCREMENTAL: All {} users already have data. Skipping ingestion.\n",
                    user_ids.len()
                );
                runner.force_answering_phase();
                sessions.clone() // won't be used, but needed for type
            } else {
                println!(
                    "INCREMENTAL: {} users already present, {} users need ingestion.",
                    existing_count,
                    missing_user_ids.len()
                );
                // Filter sessions to only missing users
                let filtered: Vec<_> = sessions
                    .iter()
                    .filter(|s| missing_user_ids.contains(&s.user_id))
                    .cloned()
                    .collect();
                println!(
                    "  Ingesting {} sessions (out of {} total) for missing users.\n",
                    filtered.len(),
                    sessions.len()
                );
                filtered
            }
        } else {
            sessions.clone()
        };

        // Only run ingestion if we didn't skip it (incremental with all present)
        if runner.phase() == BenchmarkPhase::Ingestion
            || runner.phase() == BenchmarkPhase::BatchGenerate
            || runner.phase() == BenchmarkPhase::BatchPending
        {
            // Build ingester config from runtime config (TOML + env overrides)
            let mut ingester_config = bench_config.to_ingester_config();
            // Env-only overrides not in BenchmarkConfig
            let extraction_temp: f32 = std::env::var("EXTRACTION_TEMP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            ingester_config = ingester_config.with_extraction_temperature(extraction_temp);
            let extraction_seed = ingester_config.extraction_seed;
            let graph_store_enabled = std::env::var("GRAPH_STORE").unwrap_or_default() == "1";
            let causal_links_ingestion = std::env::var("CAUSAL_LINKS").unwrap_or_default() == "1";
            if causal_links_ingestion {
                ingester_config = ingester_config.with_causal_links(true);
            }
            let ingestion_consolidation =
                std::env::var("INGESTION_CONSOLIDATION").unwrap_or_default() == "1";
            if ingestion_consolidation {
                ingester_config = ingester_config.with_consolidation(true);
            }
            let ingestion_consolidation_threshold: f32 =
                std::env::var("INGESTION_CONSOLIDATION_THRESHOLD")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(ingester_config.consolidation_threshold);
            ingester_config = ingester_config.with_consolidation_threshold(ingestion_consolidation_threshold);
            println!("Using ingestion model: {}, concurrency: {}, temp: {}, seed: {:?}, graph_store: {}, causal_links: {}, consolidation: {}\n",
                ingester_config.model, ingester_config.concurrency, extraction_temp, extraction_seed, graph_store_enabled, causal_links_ingestion, ingestion_consolidation);
            let extraction_cache_dir = std::env::var("EXTRACTION_CACHE_DIR").ok();
            if let Some(ref cache_dir) = extraction_cache_dir {
                println!("Extraction cache enabled: {}", cache_dir);
                ingester_config = ingester_config.with_extraction_cache_dir(cache_dir);
            }
            let skip_messages = std::env::var("SKIP_MESSAGES").unwrap_or_default() == "1";
            if skip_messages {
                ingester_config = ingester_config.with_skip_messages(true);
                println!("  Skip messages: YES");
            }
            // Initialize SurrealDB graph store if enabled
            let graph_store: Option<std::sync::Arc<engram::storage::GraphStore>> = if graph_store_enabled {
                let graph_db_path = format!("{}/../../data/longmemeval/surrealdb", manifest_dir);
                // Ensure parent directory exists
                std::fs::create_dir_all(&graph_db_path).ok();
                match engram::storage::GraphStore::new_rocksdb(&graph_db_path).await {
                    Ok(store) => {
                        // Clear graph on full ingestion (mirrors Qdrant clear above)
                        if ingestion_mode == "full" {
                            store.clear_all().await.expect("Failed to clear SurrealDB graph");
                            println!("SurrealDB graph cleared for clean run.");
                        }
                        println!("SurrealDB graph store: INITIALIZED at {}", graph_db_path);
                        Some(std::sync::Arc::new(store))
                    }
                    Err(e) => {
                        println!("WARNING: Failed to initialize SurrealDB graph store: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let mut ingester = SessionIngester::from_benchmark_config(ingester_config, &bench_config)
                .await
                .expect("Failed to create ingester");
            if let Some(ref store) = graph_store {
                ingester = ingester.with_graph_store(std::sync::Arc::clone(store));
            }

            if use_batch_api {
                // Batch API ingestion loop: submit → poll → process
                while runner.phase() == BenchmarkPhase::Ingestion
                    || runner.phase() == BenchmarkPhase::BatchGenerate
                    || runner.phase() == BenchmarkPhase::BatchPending
                {
                    let processed = runner
                        .run_ingestion_batch(&ingestion_sessions, &ingester)
                        .await
                        .expect("Ingestion batch should succeed");
                    println!("\n{}\n", runner.progress_summary());

                    if runner.checkpoint().has_pending_batch() {
                        println!("Batch submitted. Polling every 30 seconds...\n");
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                        continue;
                    }
                    if processed > 0 {
                        println!("Batch processing complete!\n");
                    }
                    if runner.phase() == BenchmarkPhase::Answering
                        || runner.phase() == BenchmarkPhase::Complete
                    {
                        break;
                    }
                    if processed == 0 && !runner.checkpoint().has_pending_batch() {
                        println!("Warning: No progress and no pending batch. Breaking.");
                        break;
                    }
                }
            } else {
                // Direct API ingestion loop
                let mut stall_count = 0;
                let max_stalls = 2;
                while runner.phase() == BenchmarkPhase::Ingestion {
                    let before = runner.checkpoint().ingested_sessions.len();
                    let processed = runner
                        .run_ingestion_batch(&ingestion_sessions, &ingester)
                        .await
                        .expect("Ingestion batch should succeed");
                    let after = runner.checkpoint().ingested_sessions.len();
                    let newly_ingested = after - before;

                    println!("\n{}\n", runner.progress_summary());

                    if processed == 0 {
                        break;
                    }

                    if newly_ingested == 0 {
                        stall_count += 1;
                        if stall_count >= max_stalls {
                            println!("Ingestion stalled: {}/{} sessions after {} consecutive no-progress batches.",
                                after, ingestion_sessions.len(), stall_count);
                            break;
                        }
                    } else {
                        stall_count = 0;
                    }
                }
            }

            if runner.phase() == BenchmarkPhase::Ingestion {
                runner.force_answering_phase();
            }

            // --- Report extraction cache stats ---
            if let Some(ref cache_dir) = extraction_cache_dir {
                if let Ok(entries) = std::fs::read_dir(cache_dir) {
                    let count = entries.filter(|e| {
                        e.as_ref().ok().map_or(false, |e| {
                            e.path().extension().map_or(false, |ext| ext == "json")
                        })
                    }).count();
                    println!("Extraction cache: {} entries in {}", count, cache_dir);
                }
            }

            // --- Log SurrealDB graph stats if enabled ---
            if let Some(ref store) = graph_store {
                match store.stats_all().await {
                    Ok(stats) => {
                        println!("SurrealDB graph: {} entities, {} relationships, {} mentions",
                            stats.entity_count, stats.relationship_count, stats.mention_count);
                    }
                    Err(e) => println!("WARNING: Failed to get graph stats: {}", e),
                }
            }

            // --- Write ingestion manifest ---
            {
                let qdrant_config = QdrantConfig::external(&qdrant_url)
                    .with_vector_size(EMBEDDING_DIMENSION as u64);
                let storage = QdrantStorage::new(qdrant_config)
                    .await
                    .expect("Qdrant connection failed");

                let collection_counts = storage.get_collection_counts().await.unwrap_or_default();
                let messages_count = storage.get_messages_count().await.unwrap_or(0);

                let user_ids: std::collections::HashSet<String> = sampled_questions
                    .iter()
                    .map(|q| format!("user_{}", q.id))
                    .collect();

                let current_qids = question_ids_env.as_deref().unwrap_or(if full_benchmark {
                    "FULL_BENCHMARK"
                } else {
                    "stratified_50"
                });

                let mut coll_map = serde_json::Map::new();
                for (name, count) in &collection_counts {
                    coll_map.insert(name.clone(), serde_json::Value::Number((*count).into()));
                }
                coll_map.insert(
                    "messages".to_string(),
                    serde_json::Value::Number(messages_count.into()),
                );

                let total_facts: u64 = collection_counts.iter().map(|(_, c)| c).sum();

                let manifest = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "question_ids_file": current_qids,
                    "question_count": sampled_questions.len(),
                    "user_count": user_ids.len(),
                    "session_count": ingestion_sessions.len(),
                    "total_facts": total_facts,
                    "total_messages": messages_count,
                    "collection_counts": coll_map,
                    "ingestion_mode": ingestion_mode,
                    "extraction_seed": extraction_seed,
                });

                if let Err(e) = std::fs::write(
                    &manifest_path,
                    serde_json::to_string_pretty(&manifest).unwrap(),
                ) {
                    println!("WARNING: Failed to write ingestion manifest: {}", e);
                } else {
                    println!("Ingestion manifest written to {}", manifest_path);
                }
            }
        }
    }

    // INGESTION_ONLY=1: stop after ingestion, skip answering
    if std::env::var("INGESTION_ONLY").unwrap_or_default() == "1" {
        println!("\n=== INGESTION_ONLY=1: Stopping after ingestion. ===\n");
        println!("{}\n", runner.progress_summary());
        return;
    }

    // Answering phase
    if runner.phase() == BenchmarkPhase::Answering {
        // Build answerer config from runtime config (TOML + env overrides already applied)
        let mut answerer_config = bench_config.to_answerer_config();
        let agentic = answerer_config.agentic;
        if agentic {
            println!(
                "=== Answering Phase (AGENTIC, max_iterations={}) ===\n",
                answerer_config.max_iterations.unwrap_or(20)
            );
        } else {
            println!("=== Answering Phase ===\n");
        }
        println!("Answer model: {}, temp: {}", answerer_config.answer_model, answerer_config.temperature);

        // Env-only feature toggles (not in BenchmarkConfig — rare/experimental features)
        let rerank = std::env::var("LLM_RERANK").unwrap_or_default() == "1";
        let mmr_lambda: f32 = std::env::var("MMR_LAMBDA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
        let session_ndcg = std::env::var("SESSION_NDCG").unwrap_or_default() == "1"
            && std::env::var("NO_NDCG").unwrap_or_default() != "1";
        let chain_of_note = std::env::var("CHAIN_OF_NOTE").unwrap_or_default() == "1"
            && std::env::var("NO_CON").unwrap_or_default() != "1";
        let temporal_rrf = std::env::var("TEMPORAL_RRF").unwrap_or_default() == "1"
            && std::env::var("NO_TEMPORAL").unwrap_or_default() != "1";
        let entity_linked = std::env::var("ENTITY_LINKED").unwrap_or_default() == "1"
            && std::env::var("NO_ENTITY").unwrap_or_default() != "1";

        if agentic {
            println!("Tool result limit: {}", answerer_config.tool_result_limit);
            println!(
                "Strategy guidance: {}",
                if answerer_config.use_strategy { "ENABLED" } else { "DISABLED" }
            );
            println!(
                "Prefetch: explicit={}, deductive={}, messages={}",
                answerer_config.prefetch_explicit, answerer_config.prefetch_deductive, answerer_config.prefetch_messages
            );
        }
        println!(
            "Relative dates: {}",
            if answerer_config.relative_dates {
                "ENABLED"
            } else {
                "DISABLED"
            }
        );
        if answerer_config.enable_consolidation {
            println!(
                "Consolidation: ENABLED (threshold={})",
                answerer_config.consolidation_threshold
            );
        }
        if answerer_config.enable_cross_encoder_rerank {
            println!("Cross-encoder rerank: ENABLED (top_k={})", answerer_config.cross_encoder_rerank_top_k);
        }

        if rerank {
            println!("LLM reranking: ENABLED");
            answerer_config = answerer_config.with_llm_reranking(true);
        }
        if mmr_lambda < 1.0 {
            println!("MMR lambda: {}", mmr_lambda);
            answerer_config = answerer_config.with_mmr_lambda(mmr_lambda);
        }
        if session_ndcg {
            answerer_config.session_ndcg = true;
            println!(
                "Session NDCG: ENABLED (top_sessions={})",
                answerer_config.ndcg_top_sessions
            );
        } else {
            println!("Session NDCG: DISABLED (default)");
        }
        if chain_of_note {
            answerer_config.enable_chain_of_note = true;
            println!("Chain-of-Note: ENABLED");
        } else {
            println!("Chain-of-Note: DISABLED (default)");
        }
        if temporal_rrf {
            answerer_config.enable_temporal_rrf = true;
            println!("Temporal RRF: ENABLED");
        } else {
            println!("Temporal RRF: DISABLED (default)");
        }
        if entity_linked {
            answerer_config.enable_entity_linked = true;
            println!("Entity-linked: ENABLED");
        } else {
            println!("Entity-linked: DISABLED (default)");
        }
        let mut answerer = AnswerGenerator::from_benchmark_config(answerer_config, &bench_config)
            .await
            .expect("Failed to create answerer");

        // P22: Wire ensemble routing if enabled
        if let Some(ref ec) = bench_config.ensemble {
            if ec.enabled {
                let registry = bench_config.model_registry();
                let fallback_profile = registry.get(&ec.fallback_model)
                    .unwrap_or_else(|e| panic!("No ModelProfile for ensemble.fallback_model '{}': {}", ec.fallback_model, e))
                    .clone();
                let registry_arc = std::sync::Arc::new(registry);
                let fallback_client = LlmClient::from_model_profile(
                    &fallback_profile,
                    &ec.fallback_model,
                    Some(registry_arc),
                    bench_config.llm.clone(),
                ).unwrap_or_else(|e| panic!("Failed to create fallback LlmClient for '{}': {}", ec.fallback_model, e));
                println!("P22 Ensemble: primary={}, fallback={}, abstention={}, loop_break={}, abs_questions={}",
                    ec.primary_model, ec.fallback_model,
                    ec.fallback_on_abstention, ec.fallback_on_loop_break, ec.fallback_on_abs_questions);
                answerer = answerer.with_ensemble(fallback_client, ec.clone());
            }
        }

        let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

        loop {
            let answered = runner
                .run_answering_batch(&sampled_questions, &answerer, &judge)
                .await
                .expect("Answering batch should succeed");

            println!("\n{}\n", runner.progress_summary());

            if answered == 0 || runner.phase() == BenchmarkPhase::Complete {
                break;
            }
        }
    }

    // Final results
    if runner.is_complete() {
        println!("=== VALIDATION BENCHMARK COMPLETE ===\n");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }

        // Save answers for re-judge harness
        let save_answers = std::env::var("SAVE_ANSWERS").unwrap_or_default();
        if !save_answers.is_empty() {
            let answers: Vec<serde_json::Value> = runner
                .checkpoint()
                .question_results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "question_id": r.question_id,
                        "question": r.question,
                        "expected": r.expected,
                        "generated": r.generated,
                        "category": r.category.as_str(),
                        "original_correct": r.is_correct,
                        "original_score": r.score,
                    })
                })
                .collect();
            let path = if save_answers == "1" {
                "saved_answers.json".to_string()
            } else {
                save_answers
            };
            match std::fs::write(&path, serde_json::to_string_pretty(&answers).unwrap()) {
                Ok(_) => println!("\nAnswers saved to {} ({} records)", path, answers.len()),
                Err(e) => println!("\nWARNING: Failed to save answers to {}: {}", path, e),
            }
        }
    } else {
        println!(
            "Validation benchmark not complete. Current phase: {:?}",
            runner.phase()
        );
        println!("Run again to continue.");
    }
}

/// Validation benchmark using a local Ollama model (real-time, no Batch API)
///
/// Uses Ollama for extraction + answering, OpenAI for embeddings only.
/// Clears Qdrant before running for a clean state.
///
/// Requires:
/// - Ollama running locally with the target model pulled
/// - OPENAI_API_KEY set (for embeddings)
/// - Qdrant running at localhost:6334
///
/// Run with:
///   export $(cat .env | xargs) && cargo test --release --test integration_benchmark test_validation_ollama -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_validation_ollama() {
    use engram_bench::longmemeval::{BatchConfig, BatchRunner, BenchmarkPhase, DatasetLoader};
    use engram::embedding::{RemoteEmbeddingProvider, EMBEDDING_DIMENSION};
    use engram::extraction::{ApiExtractor, ApiExtractorConfig};
    use engram::storage::{QdrantConfig, QdrantStorage};

    // Configuration — change these to test different models
    let ollama_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_string());
    let ollama_url = std::env::var("OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".to_string());
    let concurrency: usize = std::env::var("OLLAMA_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    println!(
        "=== Validation Benchmark (Ollama: {}, concurrency={}) ===\n",
        ollama_model, concurrency
    );

    // --- Setup components ---

    // Qdrant storage
    let qdrant_config = QdrantConfig::external("http://localhost:6334")
        .with_vector_size(EMBEDDING_DIMENSION as u64);
    let storage = QdrantStorage::new(qdrant_config)
        .await
        .expect("Qdrant connection failed — is Qdrant running at localhost:6334?");

    // Clear all data for clean run
    println!("Clearing Qdrant collections...");
    storage
        .clear_all_collections()
        .await
        .expect("Failed to clear Qdrant collections");
    println!("Qdrant cleared.\n");

    let storage = Arc::new(storage);

    // Embedding provider (still uses OpenAI)
    let embedding_provider = Arc::new(
        RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY must be set for embeddings"),
    );

    // Extractor — point at Ollama
    let extractor_config = ApiExtractorConfig::custom(&ollama_model, &ollama_url)
        .with_api_key("ollama")
        .with_timeout(300)
        .with_max_retries(2);
    let extractor = ApiExtractor::new(extractor_config);

    // Ingester — two-pass extraction for best quality
    let ingester = SessionIngester::new(
        IngesterConfig::default()
            .with_concurrency(concurrency)
            .with_clear_before_ingest(false), // already cleared above
    )
    .with_extractor(extractor)
    .with_embedding_provider(embedding_provider.clone())
    .with_storage(storage.clone());

    // Load dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    // Stratified sample
    let sampled_questions = stratified_sample(&dataset.questions, 50, 42);
    let needed_session_ids: std::collections::HashSet<_> = sampled_questions
        .iter()
        .flat_map(|q| q.session_ids.iter().cloned())
        .collect();
    let sessions: Vec<_> = dataset
        .sessions
        .into_iter()
        .filter(|s| needed_session_ids.contains(&s.session_id))
        .collect();

    // Print category distribution
    let mut category_counts = std::collections::HashMap::new();
    for q in &sampled_questions {
        *category_counts.entry(q.category).or_insert(0usize) += 1;
    }
    println!(
        "Selected {} questions, {} sessions",
        sampled_questions.len(),
        sessions.len()
    );
    println!("Category distribution:");
    for cat in QuestionCategory::all() {
        println!("  {:?}: {}", cat, category_counts.get(&cat).unwrap_or(&0));
    }
    println!();

    // Configure for real-time (Async) mode
    let checkpoint_name = format!(
        "ollama_{}_checkpoint.json",
        ollama_model.replace([':', '.', '/'], "_")
    );
    let config = BatchConfig::default()
        .async_mode()
        .with_checkpoint_path(&checkpoint_name)
        .with_batch_files_dir("batch_files");

    let mut runner = BatchRunner::new(config, sessions.len(), sampled_questions.len())
        .expect("Should create runner");

    println!("{}\n", runner.progress_summary());

    if runner.is_complete() {
        println!("Benchmark already complete!");
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
        return;
    }

    // --- Ingestion phase ---
    if runner.phase() == BenchmarkPhase::Ingestion {
        println!("=== Ingestion Phase (Ollama real-time) ===\n");
        let ingest_start = std::time::Instant::now();

        loop {
            let processed = runner
                .run_ingestion_batch(&sessions, &ingester)
                .await
                .expect("Ingestion batch should succeed");

            println!("\n{}\n", runner.progress_summary());

            if processed == 0 || runner.phase() != BenchmarkPhase::Ingestion {
                break;
            }
        }

        let ingest_elapsed = ingest_start.elapsed();
        println!(
            "Ingestion complete in {:.1}s ({:.1} sessions/min)\n",
            ingest_elapsed.as_secs_f64(),
            sessions.len() as f64 / ingest_elapsed.as_secs_f64() * 60.0,
        );
    }

    // --- Answering phase ---
    if runner.phase() == BenchmarkPhase::Answering {
        println!("=== Answering Phase (Ollama: {}) ===\n", ollama_model);

        let answerer_config = AnswererConfig::default().with_model(&ollama_model);
        let llm_client = LlmClient::new("ollama").expect("Failed to create LLM client").with_base_url(&ollama_url);

        let answerer = AnswerGenerator::new(answerer_config)
            .with_embedding_provider(embedding_provider.clone())
            .with_storage(storage.clone())
            .with_llm_client(llm_client);

        let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

        let answer_start = std::time::Instant::now();

        loop {
            let answered = runner
                .run_answering_batch(&sampled_questions, &answerer, &judge)
                .await
                .expect("Answering batch should succeed");

            println!("\n{}\n", runner.progress_summary());

            if answered == 0 || runner.phase() == BenchmarkPhase::Complete {
                break;
            }
        }

        let answer_elapsed = answer_start.elapsed();
        println!(
            "Answering complete in {:.1}s ({:.1} questions/min)\n",
            answer_elapsed.as_secs_f64(),
            sampled_questions.len() as f64 / answer_elapsed.as_secs_f64() * 60.0,
        );
    }

    // --- Final results ---
    if runner.is_complete() {
        println!("=== OLLAMA VALIDATION BENCHMARK COMPLETE ===\n");
        println!("Model: {}", ollama_model);
        if let Some(results) = runner.final_results() {
            println!("{}", results);
        }
    } else {
        println!(
            "Benchmark not complete. Current phase: {:?}",
            runner.phase()
        );
        println!("Run again to continue.");
    }
}

/// A8: Retrieval observability diagnostic — measures retrieval recall@K.
/// For each question, checks if the target session_ids appear in top-K search results.
/// Matches actual prefetch behavior: user_id scoping, is_latest filter, explicit+deductive+messages.
/// Cost: embedding only (~$0.50 for 500q), no LLM calls.
///
/// Supports QUESTION_IDS=@path and FULL_BENCHMARK=1 (same as test_validation_benchmark).
///
/// Run with: OPENAI_API_KEY=... FULL_BENCHMARK=1 cargo test --release --test integration_benchmark test_retrieval_recall -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_retrieval_recall() {
    use engram_bench::longmemeval::DatasetLoader;
    use engram::embedding::{EmbeddingProvider, RemoteEmbeddingProvider, EMBEDDING_DIMENSION};
    use engram::storage::{QdrantConfig, QdrantStorage};
    use qdrant_client::qdrant::{Condition, Filter};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Semaphore;

    println!("=== A8: Retrieval Observability Diagnostic ===\n");

    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY required");

    // Load dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    // Connect to Qdrant
    let qdrant_config = QdrantConfig::external("http://localhost:6334")
        .with_vector_size(EMBEDDING_DIMENSION as u64);
    let storage = Arc::new(
        QdrantStorage::new(qdrant_config)
            .await
            .expect("Qdrant connection failed"),
    );

    let embedding_provider = Arc::new(RemoteEmbeddingProvider::new(&api_key, None::<String>).expect("Failed to initialize embedding provider"));

    // Question selection: same logic as test_validation_benchmark
    let full_benchmark = std::env::var("FULL_BENCHMARK").unwrap_or_default() == "1";
    let question_ids_env = std::env::var("QUESTION_IDS").ok();
    let questions: Vec<BenchmarkQuestion> = if let Some(ref ids_spec) = question_ids_env {
        let id_list: Vec<usize> = if ids_spec.starts_with('@') {
            let raw_path = &ids_spec[1..];
            let path = if std::path::Path::new(raw_path).is_absolute() {
                raw_path.to_string()
            } else {
                let workspace_root = format!("{}/../..", manifest_dir);
                format!("{}/{}", workspace_root, raw_path)
            };
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read QUESTION_IDS file {}: {}", path, e));
            content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().parse::<usize>().unwrap_or_else(|e| panic!("Invalid ID '{}': {}", l, e)))
                .collect()
        } else {
            ids_spec.split(',').map(|s| s.trim().parse::<usize>().unwrap()).collect()
        };
        dataset.questions.iter().enumerate()
            .filter(|(i, _)| id_list.contains(&(i + 1)))
            .map(|(_, q)| q.clone())
            .collect()
    } else if full_benchmark {
        dataset.questions.clone()
    } else {
        stratified_sample(&dataset.questions, 50, 42)
    };

    // Print category distribution
    let mut category_counts = std::collections::HashMap::new();
    for q in &questions {
        *category_counts.entry(q.category).or_insert(0usize) += 1;
    }
    println!("Testing retrieval recall on {} questions", questions.len());
    println!("Category distribution:");
    for cat in QuestionCategory::all() {
        println!("  {:?}: {}", cat, category_counts.get(&cat).unwrap_or(&0));
    }
    println!();

    // Configurable k values matching actual prefetch
    let fact_k: u64 = 25; // 15 explicit + 10 deductive combined
    let msg_k: usize = 20;

    // Results storage (thread-safe for concurrent execution)
    struct QuestionResult {
        idx: usize,
        question_id: String,
        category: QuestionCategory,
        target_sessions: Vec<String>,
        found_sessions: std::collections::HashSet<String>,
        fact_sessions: std::collections::HashSet<String>,
        msg_sessions: std::collections::HashSet<String>,
    }

    let concurrency: usize = std::env::var("DIAGNOSTIC_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let progress = Arc::new(AtomicUsize::new(0));
    let total = questions.len();

    // Run searches concurrently
    let mut handles = Vec::new();
    for (i, question) in questions.iter().enumerate() {
        // Use answer_session_ids (needle sessions) not session_ids (full haystack)
        if question.answer_session_ids.is_empty() {
            continue;
        }
        let storage = Arc::clone(&storage);
        let embedding_provider = Arc::clone(&embedding_provider);
        let semaphore = Arc::clone(&semaphore);
        let progress = Arc::clone(&progress);
        let question = question.clone();
        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let user_id = format!("user_{}", question.id);

            // Embed the question
            let embedding = match embedding_provider.embed(&question.question).await {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("  [{}] embedding error: {}", i + 1, e);
                    let done = progress.fetch_add(1, Ordering::Relaxed) + 1;
                    if done % 50 == 0 { eprintln!("  Progress: {}/{}", done, total); }
                    return None;
                }
            };

            // Search facts: user_id + is_latest=true (matches exec_search_facts)
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
                    fact_k,
                    None,
                )
                .await
                .unwrap_or_default();

            // Search messages: user_id filter (matches exec_search_messages)
            let msg_filter = Some(Filter {
                must: vec![Condition::matches("user_id", user_id).into()],
                ..Default::default()
            });
            let msg_results = storage
                .search_messages_hybrid(embedding, &question.question, msg_filter, msg_k)
                .await
                .unwrap_or_default();

            // Collect session_ids
            let mut fact_sessions = std::collections::HashSet::new();
            for (mem, _) in &fact_results {
                if let Some(ref sid) = mem.session_id {
                    fact_sessions.insert(sid.clone());
                }
            }
            let mut msg_sessions = std::collections::HashSet::new();
            for point in &msg_results {
                if let Some(val) = point.payload.get("session_id") {
                    if let Some(qdrant_client::qdrant::value::Kind::StringValue(sid)) = &val.kind {
                        msg_sessions.insert(sid.clone());
                    }
                }
            }
            let found_sessions: std::collections::HashSet<String> =
                fact_sessions.union(&msg_sessions).cloned().collect();

            let done = progress.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 50 == 0 { eprintln!("  Progress: {}/{}", done, total); }

            Some(QuestionResult {
                idx: i,
                question_id: question.id.clone(),
                category: question.category,
                target_sessions: question.answer_session_ids.clone(),
                found_sessions,
                fact_sessions,
                msg_sessions,
            })
        });
        handles.push(handle);
    }

    // Collect results
    let mut results: Vec<QuestionResult> = Vec::new();
    for handle in handles {
        if let Ok(Some(r)) = handle.await {
            results.push(r);
        }
    }
    results.sort_by_key(|r| r.idx);

    // Compute stats per category
    let mut category_total: std::collections::BTreeMap<QuestionCategory, usize> =
        std::collections::BTreeMap::new();
    let mut category_full: std::collections::BTreeMap<QuestionCategory, usize> =
        std::collections::BTreeMap::new();
    let mut category_partial: std::collections::BTreeMap<QuestionCategory, usize> =
        std::collections::BTreeMap::new();
    // Track questions where facts alone suffice vs need messages
    let mut category_facts_only: std::collections::BTreeMap<QuestionCategory, usize> =
        std::collections::BTreeMap::new();

    let mut miss_details: Vec<String> = Vec::new();

    for r in &results {
        *category_total.entry(r.category).or_insert(0) += 1;

        let found_count = r.target_sessions.iter()
            .filter(|sid| r.found_sessions.contains(sid.as_str()))
            .count();
        let all_found = found_count == r.target_sessions.len();

        if all_found {
            *category_full.entry(r.category).or_insert(0) += 1;
            // Check if facts alone were sufficient
            let facts_sufficient = r.target_sessions.iter()
                .all(|sid| r.fact_sessions.contains(sid));
            if facts_sufficient {
                *category_facts_only.entry(r.category).or_insert(0) += 1;
            }
        } else if found_count > 0 {
            *category_partial.entry(r.category).or_insert(0) += 1;
        }

        if !all_found {
            let missing: Vec<&String> = r.target_sessions.iter()
                .filter(|sid| !r.found_sessions.contains(sid.as_str()))
                .collect();
            miss_details.push(format!(
                "  Q{} [{}] ({:?}) recall={}/{} missing={:?}",
                r.idx + 1, r.question_id, r.category, found_count, r.target_sessions.len(), missing
            ));
        }
    }

    // Print results table
    println!("\n=== Retrieval Recall Results (facts@{} + messages@{}) ===\n", fact_k, msg_k);
    println!(
        "{:<15} {:>6} {:>8} {:>8} {:>8} {:>10} {:>12}",
        "Category", "Total", "Full", "Partial", "Miss", "Full%", "FactsOnly"
    );
    println!("{}", "-".repeat(75));

    let mut grand_total = 0;
    let mut grand_full = 0;
    let mut grand_partial = 0;
    let mut grand_facts_only = 0;

    for cat in [
        QuestionCategory::Abstention,
        QuestionCategory::Extraction,
        QuestionCategory::MultiSession,
        QuestionCategory::Temporal,
        QuestionCategory::Updates,
    ] {
        let total = category_total.get(&cat).copied().unwrap_or(0);
        let full = category_full.get(&cat).copied().unwrap_or(0);
        let partial = category_partial.get(&cat).copied().unwrap_or(0);
        let miss = total.saturating_sub(full + partial);
        let facts_only = category_facts_only.get(&cat).copied().unwrap_or(0);
        let pct = if total > 0 { full as f64 / total as f64 * 100.0 } else { 0.0 };
        grand_total += total;
        grand_full += full;
        grand_partial += partial;
        grand_facts_only += facts_only;
        println!(
            "{:<15} {:>6} {:>8} {:>8} {:>8} {:>9.1}% {:>12}",
            format!("{:?}", cat), total, full, partial, miss, pct, facts_only
        );
    }
    let grand_miss = grand_total.saturating_sub(grand_full + grand_partial);
    let grand_pct = if grand_total > 0 { grand_full as f64 / grand_total as f64 * 100.0 } else { 0.0 };
    println!("{}", "-".repeat(75));
    println!(
        "{:<15} {:>6} {:>8} {:>8} {:>8} {:>9.1}% {:>12}",
        "TOTAL", grand_total, grand_full, grand_partial, grand_miss, grand_pct, grand_facts_only
    );

    // Print miss details
    if !miss_details.is_empty() {
        println!("\n=== Questions with Incomplete Retrieval ({}) ===\n", miss_details.len());
        for detail in &miss_details {
            println!("{}", detail);
        }
    }

    // Summary
    println!("\n=== Summary ===");
    println!("Full recall (all target sessions found): {:.1}% ({}/{})", grand_pct, grand_full, grand_total);
    println!("Partial recall (some but not all found): {:.1}% ({}/{})",
        grand_partial as f64 / grand_total.max(1) as f64 * 100.0, grand_partial, grand_total);
    println!("Complete miss (no target sessions found): {:.1}% ({}/{})",
        grand_miss as f64 / grand_total.max(1) as f64 * 100.0, grand_miss, grand_total);
    println!("Facts-only sufficient (no messages needed): {:.1}% ({}/{})",
        grand_facts_only as f64 / grand_total.max(1) as f64 * 100.0, grand_facts_only, grand_total);
    println!("\nRetrieval ceiling (full+partial): {:.1}% ({}/{})",
        (grand_full + grand_partial) as f64 / grand_total.max(1) as f64 * 100.0,
        grand_full + grand_partial, grand_total);
}

/// P9: Offline Retrieval Recall Harness — measures retrieval quality without LLM costs.
///
/// Two modes:
/// - SinglePass: one vector+fulltext search per question (matches A8, ~$0.10 for 60q)
/// - AgenticSynthetic: scripted multi-tool policy (no LLM, ~$0.30 for 60q)
///
/// Env vars:
///   RECALL_MODE=single_pass|agentic_synthetic (default: single_pass)
///   RECALL_OUTPUT=path/to/output.jsonl (optional, writes JSONL)
///   ANSWERS_FILE=path/to/saved_answers.json (optional, cross-references correctness)
///   QUESTION_IDS=@path or comma-separated (same as test_validation_benchmark)
///   FULL_BENCHMARK=1 (same as test_validation_benchmark)
///
/// Run with:
///   export $(cat .env | xargs) && QUESTION_IDS=@data/longmemeval/fast_60.txt cargo test --release --test integration_benchmark test_recall_harness -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_recall_harness() {
    use engram_bench::longmemeval::recall_harness::{
        cross_reference_answers, print_summary, run_recall_harness, write_jsonl, RecallConfig,
        RecallMode,
    };
    use engram_bench::longmemeval::DatasetLoader;
    use engram::embedding::{RemoteEmbeddingProvider, EMBEDDING_DIMENSION};
    use engram::storage::{QdrantConfig, QdrantStorage};

    println!("=== P9: Retrieval Recall Harness ===\n");

    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY required");

    // Parse mode
    let mode = match std::env::var("RECALL_MODE")
        .unwrap_or_else(|_| "single_pass".to_string())
        .as_str()
    {
        "agentic_synthetic" | "agentic" => RecallMode::AgenticSynthetic,
        _ => RecallMode::SinglePass,
    };

    let output_path = std::env::var("RECALL_OUTPUT").ok().map(std::path::PathBuf::from);
    let answers_file = std::env::var("ANSWERS_FILE").ok().map(std::path::PathBuf::from);

    // Load dataset
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let data_dir = format!("{}/../../data/benchmarks/longmemeval", manifest_dir);
    let loader = DatasetLoader::new().with_data_dir(&data_dir);
    let dataset = loader
        .load_longmemeval_s()
        .expect("Should load LongMemEval-S dataset");

    // Connect to Qdrant
    let qdrant_config = QdrantConfig::external("http://localhost:6334")
        .with_vector_size(EMBEDDING_DIMENSION as u64);
    let storage = Arc::new(
        QdrantStorage::new(qdrant_config)
            .await
            .expect("Qdrant connection failed"),
    );

    let embedding_provider = Arc::new(RemoteEmbeddingProvider::new(&api_key, None::<String>).expect("Failed to initialize embedding provider"));

    // Question selection: same logic as test_validation_benchmark
    let full_benchmark = std::env::var("FULL_BENCHMARK").unwrap_or_default() == "1";
    let question_ids_env = std::env::var("QUESTION_IDS").ok();
    let questions: Vec<BenchmarkQuestion> = if let Some(ref ids_spec) = question_ids_env {
        let id_list: Vec<usize> = if ids_spec.starts_with('@') {
            let raw_path = &ids_spec[1..];
            let path = if std::path::Path::new(raw_path).is_absolute() {
                raw_path.to_string()
            } else {
                let workspace_root = format!("{}/../..", manifest_dir);
                format!("{}/{}", workspace_root, raw_path)
            };
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read QUESTION_IDS file {}: {}", path, e));
            content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    l.trim()
                        .parse::<usize>()
                        .unwrap_or_else(|e| panic!("Invalid ID '{}': {}", l, e))
                })
                .collect()
        } else {
            ids_spec
                .split(',')
                .map(|s| s.trim().parse::<usize>().unwrap())
                .collect()
        };
        dataset
            .questions
            .iter()
            .enumerate()
            .filter(|(i, _)| id_list.contains(&(i + 1)))
            .map(|(_, q)| q.clone())
            .collect()
    } else if full_benchmark {
        dataset.questions.clone()
    } else {
        stratified_sample(&dataset.questions, 50, 42)
    };

    // Print config
    println!("Mode: {}", mode);
    println!("Questions: {}", questions.len());
    if let Some(ref p) = output_path {
        println!("Output: {}", p.display());
    }
    if let Some(ref p) = answers_file {
        println!("Answers file: {}", p.display());
    }

    // Category distribution
    let mut category_counts = std::collections::HashMap::new();
    for q in &questions {
        *category_counts.entry(q.category).or_insert(0usize) += 1;
    }
    println!("\nCategory distribution:");
    for cat in QuestionCategory::all() {
        if let Some(&count) = category_counts.get(&cat) {
            println!("  {:?}: {}", cat, count);
        }
    }
    println!();

    // Build config
    let config = RecallConfig {
        mode,
        fact_k: 25,
        msg_k: 20,
        concurrency: 20,
        output_path: output_path.clone(),
        answers_file: answers_file.clone(),
    };

    // Run harness
    let start = std::time::Instant::now();
    let mut results = run_recall_harness(&config, &questions, storage, embedding_provider).await;
    let elapsed = start.elapsed();

    // Cross-reference with answers file
    if let Some(ref path) = answers_file {
        cross_reference_answers(&mut results, path);
    }

    // Print summary
    print_summary(&results, mode);

    // Write JSONL output
    if let Some(ref path) = output_path {
        write_jsonl(&results, path).expect("Failed to write JSONL");
    }

    println!("\nCompleted in {:.1}s", elapsed.as_secs_f64());
}

/// Re-judge harness: loads saved answers and re-judges without re-answering.
/// Enables rapid iteration on judge changes at ~$0.50/run (judge LLM cost only).
///
/// Input: ANSWERS_FILE env var pointing to a saved_answers.json or checkpoint JSON.
///        Generate via: SAVE_ANSWERS=1 in the benchmark, or use the checkpoint JSON directly.
///
/// Run with:
///   export $(cat .env | xargs) && ANSWERS_FILE=saved_answers.json cargo test --release --test integration_benchmark test_rejudge -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_rejudge() {
    use engram_bench::longmemeval::{Judge, JudgeConfig};
    use engram_bench::QuestionCategory;
    use futures::stream::{self, StreamExt};

    let answers_file = std::env::var("ANSWERS_FILE")
        .expect("ANSWERS_FILE env var required (path to saved_answers.json or checkpoint JSON)");

    println!("=== Re-Judge Harness ===\n");
    println!("Loading answers from: {}\n", answers_file);

    let content = std::fs::read_to_string(&answers_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", answers_file, e));

    // Auto-detect format: checkpoint (has "question_results" key) vs standalone answers array
    let records: Vec<serde_json::Value> = {
        let parsed: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Invalid JSON in {}: {}", answers_file, e));

        if let Some(results) = parsed.get("question_results") {
            // Checkpoint format — extract and convert
            results
                .as_array()
                .expect("question_results should be an array")
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "question_id": r["question_id"],
                        "question": r["question"],
                        "expected": r["expected"],
                        "generated": r["generated"],
                        "category": r["category"],
                        "original_correct": r["is_correct"],
                        "original_score": r["score"],
                    })
                })
                .collect()
        } else if parsed.is_array() {
            parsed.as_array().unwrap().clone()
        } else {
            panic!("Unrecognized format: expected array or object with 'question_results'");
        }
    };

    println!("Loaded {} answer records\n", records.len());

    let judge = Judge::new(JudgeConfig::default()).with_llm_from_env();

    let concurrency: usize = std::env::var("REJUDGE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    struct RejudgeResult {
        question_id: String,
        category: String,
        old_correct: bool,
        new_correct: bool,
        cost: f32,
    }

    let results: Vec<RejudgeResult> = stream::iter(records.into_iter().enumerate())
        .map(|(i, record)| {
            let judge = &judge;
            async move {
                let question_id = record["question_id"]
                    .as_str()
                    .unwrap_or("?")
                    .to_string();
                let question = record["question"].as_str().unwrap_or("");
                let expected = record["expected"].as_str().unwrap_or("");
                let generated = record["generated"].as_str().unwrap_or("");
                let category_str = record["category"]
                    .as_str()
                    .unwrap_or("extraction")
                    .to_string();
                let was_correct = record["original_correct"].as_bool().unwrap_or(false);

                let category: QuestionCategory = category_str
                    .parse()
                    .unwrap_or(QuestionCategory::Extraction);

                let judge_result = judge
                    .judge_async(question, expected, generated, category)
                    .await
                    .unwrap_or_else(|e| {
                        eprintln!("Judge error for Q{}: {}", question_id, e);
                        engram_bench::longmemeval::JudgeResult::incorrect(format!(
                            "Judge error: {}",
                            e
                        ))
                    });

                let mark = match (was_correct, judge_result.is_correct) {
                    (false, true) => "+FIX",
                    (true, false) => "-REG",
                    _ => "",
                };
                if !mark.is_empty() {
                    println!(
                        "  {} Q{} [{}] {} | E: {} | G: {}",
                        mark,
                        i + 1,
                        category_str,
                        question.chars().take(60).collect::<String>(),
                        expected.chars().take(50).collect::<String>(),
                        generated.chars().take(50).collect::<String>(),
                    );
                }

                RejudgeResult {
                    question_id,
                    category: category_str,
                    old_correct: was_correct,
                    new_correct: judge_result.is_correct,
                    cost: judge_result.cost_usd,
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    // Aggregate
    let mut old_correct = 0usize;
    let mut new_correct = 0usize;
    let mut flipped_to_correct = Vec::new();
    let mut flipped_to_wrong = Vec::new();
    let mut total_judge_cost = 0.0f32;

    let mut cat_old: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut cat_new: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();

    for r in &results {
        if r.old_correct {
            old_correct += 1;
        }
        if r.new_correct {
            new_correct += 1;
        }
        total_judge_cost += r.cost;

        {
            let entry = cat_old.entry(r.category.clone()).or_insert((0, 0));
            entry.0 += 1;
            if r.old_correct {
                entry.1 += 1;
            }
        }
        {
            let entry = cat_new.entry(r.category.clone()).or_insert((0, 0));
            entry.0 += 1;
            if r.new_correct {
                entry.1 += 1;
            }
        }

        if !r.old_correct && r.new_correct {
            flipped_to_correct.push(r.question_id.clone());
        }
        if r.old_correct && !r.new_correct {
            flipped_to_wrong.push(r.question_id.clone());
        }
    }

    let total = results.len();
    println!("\n=== Re-Judge Results ===\n");
    println!(
        "Old score: {}/{} ({:.1}%)",
        old_correct,
        total,
        old_correct as f64 / total.max(1) as f64 * 100.0
    );
    println!(
        "New score: {}/{} ({:.1}%)",
        new_correct,
        total,
        new_correct as f64 / total.max(1) as f64 * 100.0
    );
    let delta = new_correct as i64 - old_correct as i64;
    println!(
        "Delta: {:+} ({:+.1}pp)",
        delta,
        delta as f64 / total.max(1) as f64 * 100.0
    );

    println!("\nFlipped wrong->correct ({}):", flipped_to_correct.len());
    for qid in &flipped_to_correct {
        println!("  + {}", qid);
    }
    println!("\nFlipped correct->wrong ({}):", flipped_to_wrong.len());
    for qid in &flipped_to_wrong {
        println!("  - {}", qid);
    }

    println!("\nBy Category (old -> new):");
    let mut cats: Vec<_> = cat_old.keys().cloned().collect();
    cats.sort();
    for cat in &cats {
        let (total_c, old_c) = cat_old.get(cat).unwrap_or(&(0, 0));
        let (_, new_c) = cat_new.get(cat).unwrap_or(&(0, 0));
        let delta_c = *new_c as i64 - *old_c as i64;
        println!(
            "  {:<15} {}/{} -> {}/{} ({:+})",
            cat, old_c, total_c, new_c, total_c, delta_c
        );
    }

    println!("\nJudge cost: ${:.4}", total_judge_cost);

    if !flipped_to_wrong.is_empty() {
        println!(
            "\nWARNING: {} correct->wrong regressions!",
            flipped_to_wrong.len()
        );
    }
    if delta > 0 && flipped_to_wrong.is_empty() {
        println!("\nSHIP: Net positive delta with 0 regressions.");
    }
}
