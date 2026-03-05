//! Types used by the agent loop and hooks.

use serde_json::Value;

/// Result of a completed agent run.
#[derive(Debug)]
pub struct AgentResult {
    /// Final answer text.
    pub answer: String,
    /// Total estimated cost in USD.
    pub cost: f32,
    /// Total prompt tokens consumed.
    pub prompt_tokens: u64,
    /// Total completion tokens consumed.
    pub completion_tokens: u64,
    /// Number of iterations executed.
    pub iterations: u32,
    /// Per-tool-call trace (for debugging / telemetry).
    pub tool_trace: Vec<ToolTraceEntry>,
    /// Structured per-call events (for gate inspection).
    pub tool_events: Vec<ToolEvent>,
    /// Whether the loop broke without a done() answer.
    pub loop_break: bool,
    /// Why the loop broke (if it did).
    pub loop_break_reason: Option<LoopBreakReason>,
}

/// A single entry in the tool call trace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolTraceEntry {
    /// Tool name.
    pub tool: String,
    /// 1-based iteration number.
    pub iteration: u32,
    /// Characters in the result.
    pub chars: usize,
    /// Whether this was a duplicate call (skipped).
    pub duplicate: bool,
}

/// Structured per-call data for gate/hook inspection.
#[derive(Debug, Clone)]
pub struct ToolEvent {
    /// Tool name.
    pub tool_name: String,
    /// Tool call ID from the LLM.
    pub tool_call_id: String,
    /// Parsed arguments.
    pub args: Value,
    /// Result text (post-hook transform).
    pub result: String,
    /// False if the tool returned an error.
    pub success: bool,
    /// Whether the call was detected as a duplicate.
    pub duplicate: bool,
}

/// Reason the agent loop terminated early.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoopBreakReason {
    /// Consecutive duplicate tool calls exceeded threshold.
    DuplicateDetection,
    /// Cost exceeded circuit breaker limit.
    CostLimit,
    /// Maximum iterations reached without a done() answer.
    IterationExhaustion,
}

/// Read-only snapshot of loop state, passed to [`AgentHook`](crate::AgentHook) methods.
pub struct LoopState<'a> {
    /// Current iteration (0-based).
    pub iteration: u32,
    /// Cumulative cost so far.
    pub total_cost: f32,
    /// Cumulative prompt tokens.
    pub prompt_tokens: u64,
    /// Cumulative completion tokens.
    pub completion_tokens: u64,
    /// Tool trace so far.
    pub tool_trace: &'a [ToolTraceEntry],
    /// Structured tool events so far.
    pub tool_events: &'a [ToolEvent],
    /// Full message history.
    pub messages: &'a [Value],
}

impl<'a> LoopState<'a> {
    /// Count non-duplicate calls to a specific tool.
    pub fn tool_call_count(&self, tool_name: &str) -> usize {
        self.tool_trace
            .iter()
            .filter(|t| t.tool == tool_name && !t.duplicate)
            .count()
    }

    /// Whether a specific tool has been called at least once (non-duplicate).
    pub fn has_called(&self, tool_name: &str) -> bool {
        self.tool_call_count(tool_name) > 0
    }
}
