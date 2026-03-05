//! Benchmarking suite for evaluating memory system performance
//!
//! This module provides infrastructure for running the LongMemEval-S benchmark
//! against the Engram memory system, including session ingestion, answer
//! generation, GPT-4o judging, and result reporting.

pub mod error;
pub mod longmemeval;
mod types;

pub use types::{
    BenchmarkConfig, BenchmarkMessage, BenchmarkQuestion, BenchmarkResult, BenchmarkSession,
    CategoryScore, QuestionCategory, QuestionResult, RetrievedMemoryInfo,
};
