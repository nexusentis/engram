//! Trait abstraction for LLM clients.
//!
//! Defines the `LlmClient` trait that both `HttpLlmClient` and test mocks can implement.

use async_trait::async_trait;
use serde_json::Value;

use super::types::CompletionResult;
use crate::error::LlmError;

/// Trait for LLM completion with tool-calling support.
///
/// Implementors provide the ability to send messages + tool schemas to an LLM
/// and receive either tool calls or a text response.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Complete with tool-calling support (for agentic loops).
    ///
    /// Sends messages + tool definitions to an LLM API, returns either tool calls or text.
    async fn complete_with_tools(
        &self,
        model: &str,
        messages: &[Value],
        tools: &[Value],
        temperature: f32,
    ) -> Result<CompletionResult, LlmError>;

    /// Optional: return the model name this client is configured to use.
    ///
    /// Used by ensemble logic to identify which model produced an answer.
    fn model_name(&self) -> Option<&str> {
        None
    }
}
