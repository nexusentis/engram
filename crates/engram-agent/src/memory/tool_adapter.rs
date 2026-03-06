//! Adapter bridging core's `ToolExecutor` to the `engram_agent::Tool` trait.
//!
//! Each production tool gets a `DynTool` instance that shares an `Arc<ToolExecutor>`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use engram_core::agent::{done_schema, tool_schemas};
use engram_core::agent::tools::ToolExecutor;

use crate::error::AgentError;
use crate::tool::Tool;

/// A single tool backed by a shared `ToolExecutor`.
struct DynTool {
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

/// The `done` tool — returns the answer text. Actual gate validation happens
/// in `MemoryAgentHook::validate_done`, which runs before this tool executes.
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

/// Wrap a core `ToolExecutor` into a list of `Box<dyn Tool>` for use with `Agent`.
///
/// Production tools only (8 tools, no graph tools).
pub fn wrap_tools(executor: Arc<ToolExecutor>) -> Vec<Box<dyn Tool>> {
    let schemas = tool_schemas();
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    for schema in schemas {
        let name = schema["function"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Skip "done" — handled specially below
        if name == "done" {
            continue;
        }

        tools.push(Box::new(DynTool::new(&name, schema, Arc::clone(&executor))));
    }

    // Add done tool — echoes the answer; validation is in the hook
    tools.push(Box::new(DoneTool {
        schema: done_schema(),
    }));

    tools
}
