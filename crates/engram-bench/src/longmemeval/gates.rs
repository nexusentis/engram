//! Benchmark-specific agent hook that wraps the production `MemoryAgentHook`.
//!
//! `BenchmarkHook` delegates all gate logic to the production implementation
//! and adds benchmark-only oracle overrides (e.g., `_abs` question handling).
//!
//! Each benchmark question creates a fresh `BenchmarkHook` (owned, not Arc'd)
//! to avoid cross-run contamination of one-shot flags.

use serde_json::Value;

use engram::config::GateConfig;
use engram_agent::{AgentHook, LoopState, MemoryAgentHook};

use super::answerer::QuestionStrategy;
use super::benchmark_config::GateThresholds;

/// Convert bench QuestionStrategy to core QuestionStrategy.
fn to_core_strategy(
    s: &QuestionStrategy,
) -> engram::agent::QuestionStrategy {
    match s {
        QuestionStrategy::Enumeration => engram::agent::QuestionStrategy::Enumeration,
        QuestionStrategy::Update => engram::agent::QuestionStrategy::Update,
        QuestionStrategy::Temporal => engram::agent::QuestionStrategy::Temporal,
        QuestionStrategy::Preference => engram::agent::QuestionStrategy::Preference,
        QuestionStrategy::Default => engram::agent::QuestionStrategy::Default,
    }
}

/// Convert bench GateThresholds to core GateConfig.
fn to_core_gate_config(gt: &GateThresholds) -> GateConfig {
    GateConfig {
        preference_min_retrievals: gt.preference_min_retrievals,
        enumeration_min_retrievals: gt.enumeration_min_retrievals,
        update_min_retrievals: gt.update_min_retrievals,
        abstention_min_retrievals: gt.abstention_min_retrievals,
        anti_abstention_keyword_threshold: gt.anti_abstention_keyword_threshold,
        preference_keyword_threshold: gt.preference_keyword_threshold,
        loop_break_consecutive_dupes: gt.loop_break_consecutive_dupes,
    }
}

/// Benchmark-specific agent hook wrapping the production `MemoryAgentHook`.
///
/// Created fresh per question — NEVER shared across questions.
/// Adds `question_id` for benchmark logging and oracle overrides.
pub struct BenchmarkHook {
    inner: MemoryAgentHook,
    /// Benchmark question ID (for logging only — not used in gate logic).
    #[allow(dead_code)]
    question_id: String,
}

impl BenchmarkHook {
    /// Create a new benchmark hook for a single question.
    pub fn new(
        strategy: QuestionStrategy,
        gates: GateThresholds,
        question_text: String,
        question_id: String,
        tool_result_limit: usize,
    ) -> Self {
        // Oracle override: skip anti-abstention for known-abstention questions.
        // P25 post-loop override handles those — gates would fight it.
        let skip_anti_abstention = question_id.ends_with("_abs");

        Self {
            inner: MemoryAgentHook::new(
                to_core_strategy(&strategy),
                to_core_gate_config(&gates),
                question_text,
                tool_result_limit,
                skip_anti_abstention,
            ),
            question_id,
        }
    }
}

impl AgentHook for BenchmarkHook {
    fn pre_tool_execute(
        &self,
        tool_name: &str,
        args: &Value,
        state: &LoopState<'_>,
    ) -> Result<(), String> {
        self.inner.pre_tool_execute(tool_name, args, state)
    }

    fn post_tool_execute(
        &self,
        tool_name: &str,
        result: String,
        state: &LoopState<'_>,
    ) -> String {
        self.inner.post_tool_execute(tool_name, result, state)
    }

    fn validate_done(
        &self,
        done_args: &Value,
        state: &LoopState<'_>,
    ) -> Result<(), String> {
        self.inner.validate_done(done_args, state)
    }
}
