//! Configuration for the agent loop.

/// Configuration controlling agent loop behavior.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// LLM model identifier (e.g. "gpt-4o", "google/gemini-3.1-pro-preview").
    pub model: String,
    /// Sampling temperature.
    pub temperature: f32,
    /// Maximum tool-calling iterations before giving up (default: 25).
    pub max_iterations: usize,
    /// USD cost circuit breaker (default: 0.50).
    pub cost_limit: f32,
    /// How many consecutive all-duplicate iterations trigger a loop break (default: 3).
    pub consecutive_dupe_limit: u32,
    /// Maximum characters per tool result before truncation (default: 16000).
    pub tool_result_limit: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4o".to_string(),
            temperature: 0.0,
            max_iterations: 25,
            cost_limit: 0.50,
            consecutive_dupe_limit: 3,
            tool_result_limit: 16000,
        }
    }
}
