//! Adapter bridging `ToolExecutor` to the `engram_agent::Tool` trait.
//!
//! Each benchmark tool gets a `DynTool` instance that shares an `Arc<ToolExecutor>`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use engram_agent::{AgentError, Tool};

use super::tools::ToolExecutor;

/// A single tool backed by a shared `ToolExecutor`.
pub struct DynTool {
    name: String,
    schema: Value,
    executor: Arc<ToolExecutor>,
}

impl DynTool {
    fn new(name: &str, schema: Value, executor: Arc<ToolExecutor>) -> Self {
        Self {
            name: name.to_string(),
            schema,
            executor,
        }
    }
}

#[async_trait]
impl Tool for DynTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        self.executor
            .execute(&self.name, &args)
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))
    }
}

/// Wrap a `ToolExecutor` into a list of `Box<dyn Tool>` for use with `Agent`.
///
/// The `include_graph` parameter controls whether graph tools are included.
pub fn wrap_tools(
    executor: Arc<ToolExecutor>,
    include_graph: bool,
) -> Vec<Box<dyn Tool>> {
    use super::tools::{tool_schemas, done_schema};

    let schemas = tool_schemas(include_graph);
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    for schema in schemas {
        let name = schema["function"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Skip "done" — the agent loop handles it natively via validate_done hook
        if name == "done" {
            continue;
        }

        tools.push(Box::new(DynTool::new(&name, schema, Arc::clone(&executor))));
    }

    // Add done tool — it just echoes the answer (actual validation is in the hook)
    tools.push(Box::new(DoneTool {
        schema: done_schema(),
    }));

    tools
}

/// The `done` tool — returns the answer text. Actual gate validation happens
/// in `BenchmarkHook::validate_done`, which runs before this tool executes.
/// This tool exists so the agent sees the done schema in its tool list.
struct DoneTool {
    schema: Value,
}

#[async_trait]
impl Tool for DoneTool {
    fn name(&self) -> &str {
        "done"
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        Ok(args["answer"].as_str().unwrap_or("").to_string())
    }
}
