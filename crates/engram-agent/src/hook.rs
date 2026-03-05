//! Lifecycle hooks for customizing agent behavior.

use serde_json::Value;

use crate::types::LoopState;

/// Lifecycle hooks that customize agent behavior without modifying the core loop.
///
/// All methods have default no-op implementations, so consumers only override
/// the hooks they need.
///
/// # Hook ordering in the loop
///
/// For each tool call in an iteration:
/// 1. **done() calls**: `validate_done()` is checked *before* duplicate detection.
/// 2. **Duplicate detection**: if the call is a duplicate, it's skipped.
/// 3. **`pre_tool_execute()`**: called before executing a non-done, non-duplicate tool.
/// 4. **Tool execution**: the tool runs.
/// 5. **`post_tool_execute()`**: called after execution, can transform the result text.
pub trait AgentHook: Send + Sync {
    /// Called before a non-done tool executes.
    ///
    /// Return `Err(rejection_message)` to skip execution and inject the rejection
    /// as a tool result instead. Covers pre-retrieval guards (e.g. date_diff guard).
    fn pre_tool_execute(
        &self,
        _tool_name: &str,
        _args: &Value,
        _state: &LoopState<'_>,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Called after a tool executes. Can transform the result text.
    ///
    /// Covers post-execution transforms (e.g. P12 Update truncation direction).
    /// The default implementation returns the result unchanged.
    fn post_tool_execute(
        &self,
        _tool_name: &str,
        result: String,
        _state: &LoopState<'_>,
    ) -> String {
        result
    }

    /// Validate a done() call.
    ///
    /// Receives the full done arguments (answer, latest_date, computed_value).
    /// Return `Ok(())` to accept the answer, or `Err(rejection_message)` to reject
    /// and inject the rejection as a tool result, forcing the agent to continue.
    ///
    /// Covers all benchmark gates (temporal, enumeration, update, abstention, etc.).
    fn validate_done(
        &self,
        _done_args: &Value,
        _state: &LoopState<'_>,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Default no-op hook implementation.
pub(crate) struct NoOpHook;

impl AgentHook for NoOpHook {}
