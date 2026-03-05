//! Tool trait for agent tool execution.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::AgentError;

/// A tool that the agent can invoke via function-calling.
///
/// Implementors provide a name, a JSON schema (OpenAI function-calling format),
/// and an async `execute` method.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The tool's function name (must match the schema).
    fn name(&self) -> &str;

    /// OpenAI-format tool schema (`{"type":"function","function":{...}}`).
    fn schema(&self) -> Value;

    /// Execute the tool with the given arguments.
    ///
    /// Returns the result text on success. Errors are caught by the agent loop
    /// and converted to error-message tool results (never propagated).
    async fn execute(&self, args: Value) -> Result<String, AgentError>;
}
