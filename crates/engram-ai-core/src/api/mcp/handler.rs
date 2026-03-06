//! MCP request handler
//!
//! Handles JSON-RPC requests for the Model Context Protocol.

use std::sync::Arc;

use serde_json::{json, Value};
use uuid::Uuid;

use super::types::*;
use crate::extraction::{Conversation, ConversationTurn};
use crate::memory_system::MemorySystem;

/// Tool definitions for memory operations
pub struct ToolDefinitions;

impl ToolDefinitions {
    /// Get all available tools
    pub fn all() -> Vec<Tool> {
        vec![
            Self::memory_add(),
            Self::memory_search(),
            Self::memory_get(),
            Self::memory_delete(),
        ]
    }

    /// memory_add tool definition
    pub fn memory_add() -> Tool {
        Tool::new(
            "memory_add",
            "Add new memories from a conversation. Extracts facts, preferences, and other memorable information.",
            json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string",
                        "description": "User identifier for the memories"
                    },
                    "content": {
                        "type": "string",
                        "description": "Conversation content to extract memories from"
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session identifier for grouping related memories"
                    }
                },
                "required": ["user_id", "content"]
            }),
        )
    }

    /// memory_search tool definition
    pub fn memory_search() -> Tool {
        Tool::new(
            "memory_search",
            "Search for relevant memories using semantic search and temporal filtering.",
            json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string",
                        "description": "User identifier to scope the search"
                    },
                    "query": {
                        "type": "string",
                        "description": "Natural language search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return",
                        "default": 10,
                        "minimum": 1,
                        "maximum": 100
                    },
                    "min_confidence": {
                        "type": "number",
                        "description": "Minimum confidence score for results",
                        "minimum": 0.0,
                        "maximum": 1.0
                    }
                },
                "required": ["user_id", "query"]
            }),
        )
    }

    /// memory_get tool definition
    pub fn memory_get() -> Tool {
        Tool::new(
            "memory_get",
            "Retrieve a specific memory by its ID.",
            json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string",
                        "description": "User identifier who owns the memory"
                    },
                    "memory_id": {
                        "type": "string",
                        "description": "UUID of the memory to retrieve",
                        "format": "uuid"
                    }
                },
                "required": ["user_id", "memory_id"]
            }),
        )
    }

    /// memory_delete tool definition
    pub fn memory_delete() -> Tool {
        Tool::new(
            "memory_delete",
            "Soft-delete a memory. The memory will be marked as deleted but can be recovered.",
            json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string",
                        "description": "User identifier who owns the memory"
                    },
                    "memory_id": {
                        "type": "string",
                        "description": "UUID of the memory to delete",
                        "format": "uuid"
                    }
                },
                "required": ["user_id", "memory_id"]
            }),
        )
    }
}

/// MCP request handler
///
/// Handles JSON-RPC requests for the MCP protocol. This handler processes
/// protocol-level messages and routes tool calls to the appropriate handlers.
/// Type alias for an external tool handler function.
///
/// Receives `(tool_name, arguments)` and returns a `ToolResult`.
/// Used by the server layer to inject tools (e.g., `memory_answer`)
/// that live outside of core.
type ExternalToolHandler = Box<dyn Fn(&str, &Value) -> ToolResult + Send + Sync>;

pub struct McpHandler {
    /// Server name
    server_name: String,
    /// Server version
    server_version: String,
    /// Whether the handler has been initialized
    initialized: bool,
    /// Backend memory system (None = stub mode for tests)
    backend: Option<Arc<MemorySystem>>,
    /// Tokio runtime handle for blocking on async calls
    rt_handle: Option<tokio::runtime::Handle>,
    /// Additional tool definitions provided by the server layer.
    extra_tools: Vec<Tool>,
    /// Handler for extra tools. Takes (tool_name, args) -> ToolResult.
    extra_tool_handler: Option<ExternalToolHandler>,
}

impl McpHandler {
    /// Create a new MCP handler (stub mode — no backend).
    pub fn new(server_name: impl Into<String>, server_version: impl Into<String>) -> Self {
        Self {
            server_name: server_name.into(),
            server_version: server_version.into(),
            initialized: false,
            backend: None,
            rt_handle: None,
            extra_tools: Vec::new(),
            extra_tool_handler: None,
        }
    }

    /// Create a handler wired to a real `MemorySystem` backend.
    ///
    /// The `handle` must come from a tokio runtime (use `Handle::current()`).
    /// Tool calls will use `handle.block_on()` to execute async methods,
    /// which is safe because the handler runs on a `spawn_blocking` thread.
    pub fn with_backend(
        server_name: impl Into<String>,
        server_version: impl Into<String>,
        backend: Arc<MemorySystem>,
        handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            server_name: server_name.into(),
            server_version: server_version.into(),
            initialized: false,
            backend: Some(backend),
            rt_handle: Some(handle),
            extra_tools: Vec::new(),
            extra_tool_handler: None,
        }
    }

    /// Register additional tools provided by the server layer.
    ///
    /// The handler function is called for any `tools/call` matching an extra tool name.
    /// It runs on a blocking thread, so it may call `Handle::block_on()` for async work.
    pub fn with_extra_tools(
        mut self,
        tools: Vec<Tool>,
        handler: impl Fn(&str, &Value) -> ToolResult + Send + Sync + 'static,
    ) -> Self {
        self.extra_tools = tools;
        self.extra_tool_handler = Some(Box::new(handler));
        self
    }

    /// Create a handler with default server info (stub mode).
    pub fn default_server() -> Self {
        Self::new("engram", env!("CARGO_PKG_VERSION"))
    }

    /// Check if the handler has been initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Handle an incoming MCP request
    pub fn handle(&mut self, request: McpRequest) -> McpResponse {
        // Validate JSON-RPC version
        if request.jsonrpc != "2.0" {
            return McpResponse::error(
                request.id,
                error_codes::INVALID_REQUEST,
                format!("Invalid JSON-RPC version: {}", request.jsonrpc),
            );
        }

        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id, request.params),
            "initialized" => self.handle_initialized(request.id),
            "ping" => McpResponse::success(request.id, json!({})),
            "tools/list" => self.handle_list_tools(request.id),
            "tools/call" => self.handle_call_tool(request.id, request.params),
            _ => McpResponse::error(
                request.id,
                error_codes::METHOD_NOT_FOUND,
                format!("Method not found: {}", request.method),
            ),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, id: Option<Value>, params: Option<Value>) -> McpResponse {
        // Parse params to validate (optional)
        if let Some(p) = params {
            if let Err(e) = serde_json::from_value::<InitializeParams>(p) {
                return McpResponse::error(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid initialize params: {}", e),
                );
            }
        }

        let result = InitializeResult::new(&self.server_name, &self.server_version);

        McpResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle initialized notification
    fn handle_initialized(&mut self, id: Option<Value>) -> McpResponse {
        self.initialized = true;
        McpResponse::success(id, json!({}))
    }

    /// Handle tools/list request
    fn handle_list_tools(&self, id: Option<Value>) -> McpResponse {
        let mut tools = ToolDefinitions::all();
        tools.extend(self.extra_tools.clone());
        let result = ListToolsResult { tools };

        McpResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle tools/call request
    fn handle_call_tool(&self, id: Option<Value>, params: Option<Value>) -> McpResponse {
        let params: CallToolParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return McpResponse::error(
                        id,
                        error_codes::INVALID_PARAMS,
                        format!("Invalid call params: {}", e),
                    );
                }
            },
            None => {
                return McpResponse::error(
                    id,
                    error_codes::INVALID_PARAMS,
                    "Missing params for tools/call",
                );
            }
        };

        // Check extra tools first (e.g., memory_answer from server layer)
        if let Some(ref handler) = self.extra_tool_handler {
            if self.extra_tools.iter().any(|t| t.name == params.name) {
                let args = params.arguments.unwrap_or_default();
                let result = handler(&params.name, &args);
                return McpResponse::success(
                    id,
                    serde_json::to_value(result).unwrap_or_default(),
                );
            }
        }

        // Validate tool exists in built-in tools
        let valid_tools = ["memory_add", "memory_search", "memory_get", "memory_delete"];
        if !valid_tools.contains(&params.name.as_str()) {
            let result = ToolResult::error(format!("Unknown tool: {}", params.name));
            return McpResponse::success(id, serde_json::to_value(result).unwrap_or_default());
        }

        // Validate required arguments
        let validation_result = self.validate_tool_arguments(&params.name, &params.arguments);
        if let Some(error) = validation_result {
            return McpResponse::success(id, serde_json::to_value(error).unwrap_or_default());
        }

        // Tool implementation would go here
        // For now, return a placeholder result
        let result = self.execute_tool(&params.name, params.arguments);
        McpResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    /// Validate tool arguments
    fn validate_tool_arguments(
        &self,
        tool_name: &str,
        arguments: &Option<Value>,
    ) -> Option<ToolResult> {
        let args = match arguments {
            Some(a) => a,
            None => {
                return Some(ToolResult::error("Missing arguments"));
            }
        };

        match tool_name {
            "memory_add" => {
                if args.get("user_id").and_then(|v| v.as_str()).is_none() {
                    return Some(ToolResult::error("Missing required argument: user_id"));
                }
                if args.get("content").and_then(|v| v.as_str()).is_none() {
                    return Some(ToolResult::error("Missing required argument: content"));
                }
                None
            }
            "memory_search" => {
                if args.get("user_id").and_then(|v| v.as_str()).is_none() {
                    return Some(ToolResult::error("Missing required argument: user_id"));
                }
                if args.get("query").and_then(|v| v.as_str()).is_none() {
                    return Some(ToolResult::error("Missing required argument: query"));
                }
                None
            }
            "memory_get" | "memory_delete" => {
                if args.get("user_id").and_then(|v| v.as_str()).is_none() {
                    return Some(ToolResult::error("Missing required argument: user_id"));
                }
                if args.get("memory_id").and_then(|v| v.as_str()).is_none() {
                    return Some(ToolResult::error("Missing required argument: memory_id"));
                }
                // Validate UUID format
                if let Some(id) = args.get("memory_id").and_then(|v| v.as_str()) {
                    if uuid::Uuid::parse_str(id).is_err() {
                        return Some(ToolResult::error("Invalid UUID format for memory_id"));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Execute a tool against the backend (or return stubs if no backend).
    fn execute_tool(&self, tool_name: &str, arguments: Option<Value>) -> ToolResult {
        let args = arguments.unwrap_or_default();
        tracing::info!(tool = tool_name, "MCP tool call");

        // If no backend, return stubs (preserves existing test behavior).
        let (backend, handle) = match (&self.backend, &self.rt_handle) {
            (Some(b), Some(h)) => (b.clone(), h.clone()),
            _ => return self.execute_tool_stub(tool_name, &args),
        };

        match tool_name {
            "memory_add" => {
                let user_id = args.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let session_id = args.get("session_id").and_then(|v| v.as_str());

                let mut conversation = Conversation::new(
                    user_id,
                    vec![ConversationTurn::user(content)],
                );
                if let Some(sid) = session_id {
                    conversation = conversation.with_session(sid);
                }

                match handle.block_on(backend.ingest(conversation)) {
                    Ok(result) => ToolResult::text(format!(
                        "Extracted {} memories: {}",
                        result.memory_ids.len(),
                        result.memory_ids.iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    Err(e) => ToolResult::error(format!("Ingestion failed: {e}")),
                }
            }
            "memory_search" => {
                let user_id = args.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
                let min_confidence = args.get("min_confidence").and_then(|v| v.as_f64());

                match handle.block_on(backend.search(user_id, query, limit)) {
                    Ok(results) => {
                        let results: Vec<_> = if let Some(min) = min_confidence {
                            results.into_iter().filter(|(_, s)| (*s as f64) >= min).collect()
                        } else {
                            results
                        };

                        if results.is_empty() {
                            return ToolResult::text("No memories found.");
                        }

                        let mut output = format!("Found {} memories:\n\n", results.len());
                        for (i, (mem, score)) in results.iter().enumerate() {
                            output.push_str(&format!(
                                "{}. [score: {:.3}] (id: {})\n   {}\n\n",
                                i + 1,
                                score,
                                mem.id,
                                mem.content
                            ));
                        }
                        ToolResult::text(output.trim_end())
                    }
                    Err(e) => ToolResult::error(format!("Search failed: {e}")),
                }
            }
            "memory_get" => {
                let user_id = args.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                let memory_id = args.get("memory_id").and_then(|v| v.as_str()).unwrap_or("");
                let uuid = match Uuid::parse_str(memory_id) {
                    Ok(u) => u,
                    Err(e) => return ToolResult::error(format!("Invalid UUID: {e}")),
                };

                match handle.block_on(backend.get_memory(user_id, uuid)) {
                    Ok(Some(mem)) => ToolResult::text(format!(
                        "ID: {}\nContent: {}\nCreated: {}\nConfidence: {:.2}",
                        mem.id, mem.content, mem.t_created, mem.confidence
                    )),
                    Ok(None) => ToolResult::text("Memory not found."),
                    Err(e) => ToolResult::error(format!("Get failed: {e}")),
                }
            }
            "memory_delete" => {
                let user_id = args.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                let memory_id = args.get("memory_id").and_then(|v| v.as_str()).unwrap_or("");
                let uuid = match Uuid::parse_str(memory_id) {
                    Ok(u) => u,
                    Err(e) => return ToolResult::error(format!("Invalid UUID: {e}")),
                };

                match handle.block_on(backend.delete_memory(user_id, uuid)) {
                    Ok(true) => ToolResult::text("Memory deleted."),
                    Ok(false) => ToolResult::text("Memory not found."),
                    Err(e) => ToolResult::error(format!("Delete failed: {e}")),
                }
            }
            _ => ToolResult::error(format!("Tool not implemented: {}", tool_name)),
        }
    }

    /// Stub tool execution for tests (no backend configured).
    fn execute_tool_stub(&self, tool_name: &str, args: &Value) -> ToolResult {
        match tool_name {
            "memory_add" => {
                let user_id = args.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                ToolResult::text(format!(
                    "Memory extraction requested for user '{}' with {} characters of content. \
                     (Note: Full implementation requires storage integration)",
                    user_id,
                    content.len()
                ))
            }
            "memory_search" => {
                let user_id = args.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
                ToolResult::text(format!(
                    "Search requested for user '{}': '{}' (limit: {}). \
                     (Note: Full implementation requires retrieval integration)",
                    user_id, query, limit
                ))
            }
            "memory_get" => {
                let memory_id = args.get("memory_id").and_then(|v| v.as_str()).unwrap_or("");
                ToolResult::text(format!(
                    "Memory retrieval requested for ID: {}. \
                     (Note: Full implementation requires storage integration)",
                    memory_id
                ))
            }
            "memory_delete" => {
                let memory_id = args.get("memory_id").and_then(|v| v.as_str()).unwrap_or("");
                ToolResult::text(format!(
                    "Memory deletion requested for ID: {}. \
                     (Note: Full implementation requires storage integration)",
                    memory_id
                ))
            }
            _ => ToolResult::error(format!("Tool not implemented: {}", tool_name)),
        }
    }
}

impl Default for McpHandler {
    fn default() -> Self {
        Self::default_server()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_handler() -> McpHandler {
        McpHandler::new("test-server", "1.0.0")
    }

    #[test]
    fn test_handler_creation() {
        let handler = create_handler();
        assert!(!handler.is_initialized());
    }

    #[test]
    fn test_default_server() {
        let handler = McpHandler::default_server();
        assert_eq!(handler.server_name, "engram");
    }

    #[test]
    fn test_handle_initialize() {
        let mut handler = create_handler();

        let request = McpRequest::initialize(json!(1), "test-client", "1.0.0");
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "test-server");
    }

    #[test]
    fn test_handle_initialized() {
        let mut handler = create_handler();
        assert!(!handler.is_initialized());

        let request = McpRequest::new("initialized", Some(json!(1)), None);
        let response = handler.handle(request);

        assert!(response.is_success());
        assert!(handler.is_initialized());
    }

    #[test]
    fn test_handle_ping() {
        let mut handler = create_handler();

        let request = McpRequest::new("ping", Some(json!(1)), None);
        let response = handler.handle(request);

        assert!(response.is_success());
    }

    #[test]
    fn test_handle_list_tools() {
        let mut handler = create_handler();

        let request = McpRequest::list_tools(json!(1));
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);

        // Verify tool names
        let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(tool_names.contains(&"memory_add"));
        assert!(tool_names.contains(&"memory_search"));
        assert!(tool_names.contains(&"memory_get"));
        assert!(tool_names.contains(&"memory_delete"));
    }

    #[test]
    fn test_handle_unknown_method() {
        let mut handler = create_handler();

        let request = McpRequest::new("unknown/method", Some(json!(1)), None);
        let response = handler.handle(request);

        assert!(response.is_error());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::METHOD_NOT_FOUND);
    }

    #[test]
    fn test_handle_invalid_jsonrpc_version() {
        let mut handler = create_handler();

        let request = McpRequest {
            jsonrpc: "1.0".to_string(),
            id: Some(json!(1)),
            method: "ping".to_string(),
            params: None,
        };
        let response = handler.handle(request);

        assert!(response.is_error());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_REQUEST);
    }

    #[test]
    fn test_handle_call_tool_missing_params() {
        let mut handler = create_handler();

        let request = McpRequest::new("tools/call", Some(json!(1)), None);
        let response = handler.handle(request);

        assert!(response.is_error());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_PARAMS);
    }

    #[test]
    fn test_handle_call_tool_unknown_tool() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(json!(1), "unknown_tool", None);
        let response = handler.handle(request);

        // Unknown tool returns success with error in result
        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn test_handle_call_memory_search() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_search",
            Some(json!({
                "user_id": "test-user",
                "query": "work projects"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(!result["isError"].as_bool().unwrap());
    }

    #[test]
    fn test_handle_call_memory_add() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_add",
            Some(json!({
                "user_id": "test-user",
                "content": "I work at Acme Corp as a software engineer."
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(!result["isError"].as_bool().unwrap());
    }

    #[test]
    fn test_handle_call_memory_get() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_get",
            Some(json!({
                "user_id": "test-user",
                "memory_id": "01234567-89ab-cdef-0123-456789abcdef"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
    }

    #[test]
    fn test_handle_call_memory_delete() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_delete",
            Some(json!({
                "user_id": "test-user",
                "memory_id": "01234567-89ab-cdef-0123-456789abcdef"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
    }

    #[test]
    fn test_validate_memory_add_missing_user_id() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_add",
            Some(json!({
                "content": "test content"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("user_id"));
    }

    #[test]
    fn test_validate_memory_search_missing_query() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_search",
            Some(json!({
                "user_id": "test-user"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("query"));
    }

    #[test]
    fn test_validate_memory_get_missing_user_id() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_get",
            Some(json!({
                "memory_id": "01234567-89ab-cdef-0123-456789abcdef"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("user_id"));
    }

    #[test]
    fn test_validate_memory_get_invalid_uuid() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_get",
            Some(json!({
                "user_id": "test-user",
                "memory_id": "not-a-uuid"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("UUID"));
    }

    #[test]
    fn test_validate_memory_delete_missing_user_id() {
        let mut handler = create_handler();

        let request = McpRequest::call_tool(
            json!(1),
            "memory_delete",
            Some(json!({
                "memory_id": "01234567-89ab-cdef-0123-456789abcdef"
            })),
        );
        let response = handler.handle(request);

        assert!(response.is_success());
        let result = response.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("user_id"));
    }

    #[test]
    fn test_tool_definitions_all() {
        let tools = ToolDefinitions::all();
        assert_eq!(tools.len(), 4);
    }

    #[test]
    fn test_tool_definitions_memory_add() {
        let tool = ToolDefinitions::memory_add();
        assert_eq!(tool.name, "memory_add");
        assert!(tool.description.contains("memories"));

        let required = tool.input_schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("user_id")));
        assert!(required.contains(&json!("content")));
    }

    #[test]
    fn test_tool_definitions_memory_search() {
        let tool = ToolDefinitions::memory_search();
        assert_eq!(tool.name, "memory_search");

        let props = &tool.input_schema["properties"];
        assert!(props["limit"]["default"].as_i64().is_some());
    }
}
