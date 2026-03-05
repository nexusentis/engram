---
title: Agent Framework
sidebar_position: 8
description: "Building AI agents with engram-agent: Tool trait, AgentHook lifecycle, and the agent loop."
---

# Agent Framework

The `engram-agent` crate provides a reusable LLM agent loop with tool-calling, duplicate detection, cost limits, and lifecycle hooks.

## Architecture

```
engram-ai-core (owns LlmClient trait + HttpLlmClient impl)
    ^
engram-agent (owns Agent loop + Tool/AgentHook traits)
    ^
your-app (implements Tool for domain-specific tools, AgentHook for custom logic)
```

The agent is model-agnostic — it works with any LLM that implements the `LlmClient` trait from `engram-ai-core`.

## Add the dependency

```toml
[dependencies]
engram-agent = "0.1"
engram-ai-core = "0.1"
```

## Quick start

```rust
use std::sync::Arc;
use engram_agent::{Agent, AgentConfig, Tool, AgentError};
use engram_core::llm::{HttpLlmClient, LlmClientConfig};
use serde_json::{json, Value};
use async_trait::async_trait;

// 1. Define a tool
struct GreetTool;

#[async_trait]
impl Tool for GreetTool {
    fn name(&self) -> &str { "greet" }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "greet",
                "description": "Greet a person by name",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"}
                    },
                    "required": ["name"]
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        let name = args["name"].as_str().unwrap_or("world");
        Ok(format!("Hello, {}!", name))
    }
}

// 2. Create and run the agent
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AgentConfig {
        model: "gpt-4o".to_string(),
        temperature: 0.0,
        max_iterations: 10,
        ..Default::default()
    };

    let llm_config = LlmClientConfig::openai("sk-...");
    let llm: Arc<dyn engram_core::llm::LlmClient> =
        Arc::new(HttpLlmClient::new(llm_config));

    let tools: Vec<Box<dyn Tool>> = vec![Box::new(GreetTool)];

    let agent = Agent::new(config, tools, llm);

    let messages = vec![json!({
        "role": "user",
        "content": "Please greet Alice"
    })];

    let result = agent.run(messages).await?;
    println!("Answer: {}", result.answer);
    println!("Cost: ${:.4}", result.cost);
    println!("Iterations: {}", result.iterations);

    Ok(())
}
```

## AgentConfig

| Field | Default | Description |
|-------|---------|-------------|
| `model` | `"gpt-4o"` | LLM model identifier |
| `temperature` | `0.0` | Sampling temperature |
| `max_iterations` | `25` | Max tool-calling rounds before forced stop |
| `cost_limit` | `0.50` | USD cost circuit breaker |
| `consecutive_dupe_limit` | `3` | Consecutive all-duplicate iterations before loop break |
| `tool_result_limit` | `16000` | Max characters per tool result (truncated at line boundary) |

## Tool trait

Implement `Tool` to define callable tools:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (must match the schema).
    fn name(&self) -> &str;

    /// OpenAI function-calling format schema.
    fn schema(&self) -> Value;

    /// Execute with parsed arguments. Return result text or error.
    async fn execute(&self, args: Value) -> Result<String, AgentError>;
}
```

The agent automatically includes a `done` tool that the LLM calls when it has a final answer. You don't need to define it.

## AgentHook trait

Hooks customize agent behavior without modifying the core loop. All methods have no-op defaults — override only what you need.

```rust
pub trait AgentHook: Send + Sync {
    /// Before tool execution. Return Err to reject and inject message.
    fn pre_tool_execute(&self, tool_name: &str, args: &Value, state: &LoopState)
        -> Result<(), String> { Ok(()) }

    /// After tool execution. Transform the result text.
    fn post_tool_execute(&self, tool_name: &str, result: String, state: &LoopState)
        -> String { result }

    /// Validate done() calls. Return Err to reject and force agent to continue.
    fn validate_done(&self, done_args: &Value, state: &LoopState)
        -> Result<(), String> { Ok(()) }
}
```

### Hook execution order per tool call

1. If the call is `done()` → `validate_done()` (before duplicate detection)
2. Duplicate detection (skip if seen before)
3. `pre_tool_execute()` (can reject)
4. Tool execution
5. `post_tool_execute()` (can transform result)

### Attaching a hook

```rust
let agent = Agent::new(config, tools, llm)
    .with_hook(Box::new(MyCustomHook));
```

## LoopState

Hooks receive a read-only `LoopState` snapshot:

```rust
pub struct LoopState<'a> {
    pub iteration: u32,          // Current iteration (0-based)
    pub total_cost: f32,         // Cumulative USD cost
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_trace: &'a [ToolTraceEntry],  // Per-call trace
    pub tool_events: &'a [ToolEvent],      // Structured events
    pub messages: &'a [Value],             // Full message history
}

// Convenience methods
state.tool_call_count("search_facts")  // Non-duplicate call count
state.has_called("search_facts")       // Whether tool was called at least once
```

## AgentResult

Returned by `agent.run()`:

```rust
pub struct AgentResult {
    pub answer: String,                        // Final answer text
    pub cost: f32,                             // Total USD cost
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub iterations: u32,                       // Iterations executed
    pub tool_trace: Vec<ToolTraceEntry>,       // Per-call trace
    pub tool_events: Vec<ToolEvent>,           // Structured events
    pub loop_break: bool,                      // True if loop terminated early
    pub loop_break_reason: Option<LoopBreakReason>,
}
```

### Loop break reasons

| Reason | When |
|--------|------|
| `DuplicateDetection` | N consecutive iterations where all tool calls were duplicates |
| `CostLimit` | Cumulative cost exceeded `cost_limit` |
| `IterationExhaustion` | Reached `max_iterations` without a `done()` answer |

## Built-in safety features

- **Duplicate detection**: Tracks `tool_name:args` signatures. Duplicate calls return a helpful message telling the agent to try something different.
- **Cost circuit breaker**: Stops the loop when cumulative LLM cost exceeds the configured limit.
- **Iteration limit**: Hard cap on tool-calling rounds.
- **Tool result truncation**: Results exceeding `tool_result_limit` are truncated at line boundaries (keeps start by default).
