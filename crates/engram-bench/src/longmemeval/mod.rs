//! LongMemEval-S benchmark harness
//!
//! This module provides the full evaluation harness for the LongMemEval-S benchmark,
//! including session ingestion, answer generation, GPT-4o judging, and result reporting.
//!
//! # Architecture
//!
//! The harness consists of several components:
//!
//! - **SessionIngester**: Ingests benchmark sessions into the memory system
//! - **AnswerGenerator**: Generates answers using retrieval + LLM
//! - **Judge**: Evaluates answers using GPT-4o semantic comparison
//! - **LongMemEvalHarness**: Orchestrates the full benchmark pipeline
//!
//! # Usage
//!
//! ```ignore
//! use engram_bench::longmemeval::{LongMemEvalHarness, DatasetLoader};
//! use engram_bench::BenchmarkConfig;
//!
//! // Create configuration
//! let config = BenchmarkConfig::new("my-run")
//!     .with_extraction_mode("local-fast")
//!     .with_max_questions(100);
//!
//! // Create harness
//! let harness = LongMemEvalHarness::with_defaults(config);
//!
//! // Load dataset and run
//! let loader = DatasetLoader::new();
//! let dataset = loader.load_longmemeval_s()?;
//! let result = harness.run(&dataset.sessions, &dataset.questions)?;
//!
//! println!("Accuracy: {:.1}%", result.accuracy * 100.0);
//! ```

mod answerer;
pub mod benchmark_config;
mod batch_runner;
pub mod gates;
mod harness;
mod ingester;
mod judge;
pub mod recall_harness;

pub use answerer::{estimate_cost, set_global_model_registry, AnswerGenerator, AnswerResult, AnswererConfig, GraphAugmentConfig, LlmClient};
pub use benchmark_config::{BenchmarkConfig, GateThresholds, LlmClientConfig, ModelProfile, ModelRegistry, EnsembleConfig};
pub use batch_runner::{
    BatchConfig, BatchRunner, BenchmarkCheckpoint, BenchmarkPhase, FinalResults, IngestionApiMode,
    QuestionResult,
};
pub use harness::{
    CategoryComparison, DatasetLoader, LongMemEvalDataset, LongMemEvalHarness, RunComparison,
};
pub use ingester::{
    BatchPollResult as IngesterBatchPollResult, IngesterConfig, IngestionMode, IngestionStats,
    SessionIngester, SessionStats, MAX_CONCURRENCY,
};
pub use judge::{Judge, JudgeConfig, JudgeResult};

// Re-export core types that were formerly in bench tools/
pub use engram::agent::{ToolExecutor, ToolExecutionResult};
pub use engram::llm::{AgentResponse, CompletionResult, ToolCall};
