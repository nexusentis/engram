//! Core types for the answerer module.

use engram::retrieval::AbstentionReason;

use engram::agent::QuestionStrategy;
use crate::types::RetrievedMemoryInfo;

/// Single entry in the tool call trace
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolTraceEntry {
    pub tool: String,
    pub iteration: u32,
    pub chars: usize,
    pub duplicate: bool,
}

/// Reason the agentic loop broke before getting a done() answer
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoopBreakReason {
    /// Consecutive duplicate tool calls detected
    DuplicateDetection,
    /// Cost exceeded circuit breaker limit
    CostLimit,
    /// Max iterations reached without answer
    IterationExhaustion,
}

/// Result from answering a question
#[derive(Debug, Clone)]
pub struct AnswerResult {
    /// Generated answer text
    pub answer: String,
    /// Memories retrieved for context
    pub retrieved_memories: Vec<RetrievedMemoryInfo>,
    /// Time spent on retrieval in milliseconds
    pub retrieval_time_ms: u64,
    /// Time spent on answer generation in milliseconds
    pub answer_time_ms: u64,
    /// Total time in milliseconds
    pub total_time_ms: u64,
    /// Whether the system abstained from answering
    pub abstained: bool,
    /// Reason for abstention (if abstained)
    pub abstention_reason: Option<AbstentionReason>,
    /// Estimated cost in USD
    pub cost_usd: f32,
    /// Tool call trace for debugging
    pub tool_trace: Vec<ToolTraceEntry>,
    /// Whether the agentic loop broke (dupes, cost, exhaustion) rather than done()
    pub loop_break: bool,
    /// Why the loop broke
    pub loop_break_reason: Option<LoopBreakReason>,
    /// Whether ensemble fallback was used (P22)
    pub fallback_used: bool,
    /// Why fallback was triggered (e.g., "abstention", "loop_break:DuplicateDetection")
    pub fallback_reason: Option<String>,
    /// Model that ran first (for telemetry)
    pub primary_model: Option<String>,
    /// Model that produced the final answer (for telemetry)
    pub final_model: Option<String>,
    /// P31: Strategy used for this question (for ensemble routing)
    pub strategy: Option<QuestionStrategy>,
    /// P31: Number of agentic iterations used
    pub iterations: u32,
}

impl AnswerResult {
    /// Create a new answer result
    pub fn new(answer: impl Into<String>) -> Self {
        Self {
            answer: answer.into(),
            retrieved_memories: Vec::new(),
            retrieval_time_ms: 0,
            answer_time_ms: 0,
            total_time_ms: 0,
            abstained: false,
            abstention_reason: None,
            cost_usd: 0.0,
            tool_trace: Vec::new(),
            loop_break: false,
            loop_break_reason: None,
            fallback_used: false,
            fallback_reason: None,
            primary_model: None,
            final_model: None,
            strategy: None,
            iterations: 0,
        }
    }

    /// Create an abstention result with a specific reason
    pub fn abstention_with_reason(reason: AbstentionReason) -> Self {
        Self {
            answer: reason.message().to_string(),
            retrieved_memories: Vec::new(),
            retrieval_time_ms: 0,
            answer_time_ms: 0,
            total_time_ms: 0,
            abstained: true,
            abstention_reason: Some(reason),
            cost_usd: 0.0,
            tool_trace: Vec::new(),
            loop_break: false,
            loop_break_reason: None,
            fallback_used: false,
            fallback_reason: None,
            primary_model: None,
            final_model: None,
            strategy: None,
            iterations: 0,
        }
    }

    /// Create an abstention result (default reason)
    pub fn abstention() -> Self {
        Self::abstention_with_reason(AbstentionReason::InsufficientResults)
    }

    /// Set retrieval time
    pub fn with_retrieval_time(mut self, ms: u64) -> Self {
        self.retrieval_time_ms = ms;
        self
    }

    /// Set answer time
    pub fn with_answer_time(mut self, ms: u64) -> Self {
        self.answer_time_ms = ms;
        self
    }

    /// Set total time
    pub fn with_total_time(mut self, ms: u64) -> Self {
        self.total_time_ms = ms;
        self
    }

    /// Add retrieved memories
    pub fn with_memories(mut self, memories: Vec<RetrievedMemoryInfo>) -> Self {
        self.retrieved_memories = memories;
        self
    }

    /// Set tool call trace
    pub fn with_tool_trace(mut self, trace: Vec<ToolTraceEntry>) -> Self {
        self.tool_trace = trace;
        self
    }

    /// Set cost
    pub fn with_cost(mut self, cost: f32) -> Self {
        self.cost_usd = cost;
        self
    }
}
