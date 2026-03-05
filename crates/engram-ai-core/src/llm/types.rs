//! LLM protocol types for tool-calling and completion results.

use serde_json::Value;

/// A single tool call from the LLM
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Tool call ID (from OpenAI API)
    pub id: String,
    /// Tool/function name
    pub name: String,
    /// Parsed arguments
    pub arguments: Value,
    /// Raw JSON from the API response (for providers like Gemini that need fields echoed back)
    pub raw_json: Option<Value>,
}

/// Response from the LLM: either tool calls or text
#[derive(Debug)]
pub enum AgentResponse {
    /// LLM wants to call tools
    ToolCalls(Vec<ToolCall>),
    /// LLM responded with text (no tool calls)
    TextResponse(String),
}

/// Result of a completion with tools
#[derive(Debug)]
pub struct CompletionResult {
    /// The response (tool calls or text)
    pub response: AgentResponse,
    /// Prompt tokens used
    pub prompt_tokens: u64,
    /// Completion tokens used
    pub completion_tokens: u64,
    /// Estimated cost in USD
    pub cost: f32,
}

/// Estimate API cost for a request.
/// Uses the global ModelRegistry pricing when available.
/// Falls back to a hardcoded pricing table for backward compatibility.
pub fn estimate_cost(model: &str, prompt_tokens: u64, completion_tokens: u64) -> f32 {
    // Try global registry first (set by benchmark harness)
    if let Some(ref registry) = *GLOBAL_MODEL_REGISTRY.lock().unwrap() {
        if let Ok(cost) = registry.estimate_cost(model, prompt_tokens, completion_tokens) {
            return cost;
        }
    }
    // Fallback: hardcoded pricing table (for non-benchmark usage / tests)
    let (prompt_price, completion_price) = match model {
        "gpt-4o" => (2.50, 10.00),
        "gpt-4o-mini" => (0.15, 0.60),
        "gpt-4-turbo" => (10.00, 30.00),
        "gpt-5" => (1.25, 10.00),
        "gpt-5-mini" => (0.25, 2.00),
        "gpt-5.2" => (1.75, 14.00),
        "o3" => (2.00, 8.00),
        "o4-mini" => (1.10, 4.40),
        "claude-3-5-sonnet" | "claude-sonnet-4" => (3.00, 15.00),
        "claude-3-opus" | "claude-opus-4" => (15.00, 75.00),
        m if m.starts_with("gemini-3") => (2.00, 12.00),
        m if m.starts_with("gemini-2.5") => (1.25, 10.00),
        _ => (1.00, 2.00),
    };

    let prompt_cost = (prompt_tokens as f32 / 1_000_000.0) * prompt_price;
    let completion_cost = (completion_tokens as f32 / 1_000_000.0) * completion_price;

    prompt_cost + completion_cost
}

/// Global model registry, set once at benchmark startup.
/// Used by `estimate_cost()` and can be shared across components.
static GLOBAL_MODEL_REGISTRY: std::sync::Mutex<
    Option<std::sync::Arc<super::config::ModelRegistry>>,
> = std::sync::Mutex::new(None);

/// Set the global model registry (called once at benchmark startup).
pub fn set_global_model_registry(registry: std::sync::Arc<super::config::ModelRegistry>) {
    *GLOBAL_MODEL_REGISTRY.lock().unwrap() = Some(registry);
}
