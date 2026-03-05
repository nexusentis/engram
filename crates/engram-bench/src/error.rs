//! Error types for the benchmark harness

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BenchmarkError {
    #[error("Dataset download failed: {0}")]
    Download(String),

    #[error("Dataset parsing failed: {0}")]
    Parse(String),

    #[error("Session ingestion failed: {0}")]
    Ingestion(String),

    #[error("Question answering failed: {0}")]
    Answering(String),

    #[error("Judge evaluation failed: {0}")]
    Judge(String),

    #[error("Result storage failed: {0}")]
    Storage(String),

    #[error("Configuration invalid: {0}")]
    Config(String),

    #[error("HTTP request failed: {0}")]
    Http(String),
}

/// Unified error type for benchmark operations.
///
/// Wraps both core library errors and benchmark-specific errors.
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] engram::error::Error),

    #[error("Benchmark: {0}")]
    Benchmark(#[from] BenchmarkError),

    #[error("Serialization: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
