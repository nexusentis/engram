//! Error types for the agent framework.

/// Errors that can occur during agent execution.
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    /// LLM API call failed.
    #[error("LLM error: {0}")]
    Llm(#[from] engram_core::error::LlmError),

    /// Tool execution failed.
    #[error("Tool error: {0}")]
    Tool(String),

    /// Agent exhausted all iterations without producing an answer.
    #[error("No answer after {0} iterations")]
    NoAnswer(usize),
}
