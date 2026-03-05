//! Error types for the engram library

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Extraction error: {0}")]
    Extraction(#[from] ExtractionError),

    #[error("Retrieval error: {0}")]
    Retrieval(#[from] RetrievalError),

    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Qdrant error: {0}")]
    Qdrant(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Memory not found: {0}")]
    MemoryNotFound(String),
}

#[derive(Error, Debug)]
pub enum ExtractionError {
    #[error("Model error: {0}")]
    Model(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Validation failed: {0}")]
    Validation(String),
}

#[derive(Error, Debug)]
pub enum RetrievalError {
    #[error("Search error: {0}")]
    Search(String),

    #[error("Embedding error: {0}")]
    Embedding(String),
}

#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("API request failed: {0}")]
    ApiRequest(String),

    #[error("API response error: {0}")]
    ApiResponse(String),

    #[error("Response parsing failed: {0}")]
    ResponseParsing(String),

    #[error("Empty result from embedding provider")]
    EmptyResult,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    FileRead(String),

    #[error("Failed to parse config: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("API error {status}: {body}")]
    ApiError { status: u16, body: String },

    #[error("Rate limited after {retries} retries: {body}")]
    RateLimited { retries: u32, body: String },

    #[error("Token refresh failed: {0}")]
    TokenRefresh(String),

    #[error("No API key: {0}")]
    NoApiKey(String),

    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;
