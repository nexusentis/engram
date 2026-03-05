//! MCP (Model Context Protocol) message types
//!
//! JSON-RPC 2.0 based protocol for AI tool integration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP JSON-RPC request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpRequest {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,
    /// Request ID for correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Method name to invoke
    pub method: String,
    /// Method parameters
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl McpRequest {
    /// Create a new request
    pub fn new(method: impl Into<String>, id: Option<Value>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }

    /// Create an initialize request
    pub fn initialize(id: Value, client_name: &str, client_version: &str) -> Self {
        Self::new(
            "initialize",
            Some(id),
            Some(serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": client_name,
                    "version": client_version
                }
            })),
        )
    }

    /// Create a tools/list request
    pub fn list_tools(id: Value) -> Self {
        Self::new("tools/list", Some(id), None)
    }

    /// Create a tools/call request
    pub fn call_tool(id: Value, name: &str, arguments: Option<Value>) -> Self {
        Self::new(
            "tools/call",
            Some(id),
            Some(serde_json::json!({
                "name": name,
                "arguments": arguments
            })),
        )
    }
}

/// MCP JSON-RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,
    /// Request ID for correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Success result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

impl McpResponse {
    /// Create a success response
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Create an error response with data
    pub fn error_with_data(
        id: Option<Value>,
        code: i32,
        message: impl Into<String>,
        data: Value,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: message.into(),
                data: Some(data),
            }),
        }
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        self.error.is_none() && self.result.is_some()
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// MCP error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    /// Error code
    pub code: i32,
    /// Human-readable error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Standard JSON-RPC 2.0 error codes
pub mod error_codes {
    /// Invalid JSON was received
    pub const PARSE_ERROR: i32 = -32700;
    /// The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// Current protocol version
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Tools capability
    pub tools: ToolsCapability,
}

/// Tools capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsCapability {
    /// Whether the tools list can change during a session
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

impl Tool {
    /// Create a new tool definition
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Result content
    pub content: Vec<ToolContent>,
    /// Whether this result represents an error
    #[serde(rename = "isError")]
    pub is_error: bool,
}

impl ToolResult {
    /// Create a text result
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(content)],
            is_error: false,
        }
    }

    /// Create an error result
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(message)],
            is_error: true,
        }
    }

    /// Create a result with multiple content items
    pub fn with_content(content: Vec<ToolContent>, is_error: bool) -> Self {
        Self { content, is_error }
    }
}

/// Tool content item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContent {
    /// Content type (text, image, etc.)
    #[serde(rename = "type")]
    pub content_type: String,
    /// Text content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// MIME type for non-text content
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Data for non-text content (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

impl ToolContent {
    /// Create text content
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content_type: "text".to_string(),
            text: Some(content.into()),
            mime_type: None,
            data: None,
        }
    }

    /// Create image content
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            content_type: "image".to_string(),
            text: None,
            mime_type: Some(mime_type.into()),
            data: Some(data.into()),
        }
    }
}

/// Initialize request params
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InitializeParams {
    /// Protocol version the client supports
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    /// Client capabilities
    pub capabilities: Value,
    /// Client information
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

/// Client information
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientInfo {
    /// Client name
    pub name: String,
    /// Client version
    pub version: String,
}

/// Initialize result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the server supports
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
    /// Server information
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
}

impl InitializeResult {
    /// Create a new initialize result
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: ServerInfo {
                name: name.into(),
                version: version.into(),
            },
        }
    }
}

/// Server information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
}

/// Tool call params
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CallToolParams {
    /// Tool name
    pub name: String,
    /// Tool arguments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// List tools result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// Available tools
    pub tools: Vec<Tool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_request_new() {
        let request = McpRequest::new("test/method", Some(serde_json::json!(1)), None);
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "test/method");
        assert!(request.params.is_none());
    }

    #[test]
    fn test_mcp_request_initialize() {
        let request = McpRequest::initialize(serde_json::json!(1), "test-client", "1.0.0");
        assert_eq!(request.method, "initialize");
        assert!(request.params.is_some());

        let params = request.params.unwrap();
        assert_eq!(params["clientInfo"]["name"], "test-client");
    }

    #[test]
    fn test_mcp_request_list_tools() {
        let request = McpRequest::list_tools(serde_json::json!(2));
        assert_eq!(request.method, "tools/list");
        assert!(request.params.is_none());
    }

    #[test]
    fn test_mcp_request_call_tool() {
        let request = McpRequest::call_tool(
            serde_json::json!(3),
            "memory_search",
            Some(serde_json::json!({"query": "test"})),
        );
        assert_eq!(request.method, "tools/call");

        let params = request.params.unwrap();
        assert_eq!(params["name"], "memory_search");
    }

    #[test]
    fn test_mcp_response_success() {
        let response = McpResponse::success(
            Some(serde_json::json!(1)),
            serde_json::json!({"status": "ok"}),
        );
        assert!(response.is_success());
        assert!(!response.is_error());
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_mcp_response_error() {
        let response = McpResponse::error(
            Some(serde_json::json!(1)),
            error_codes::INVALID_PARAMS,
            "Missing required parameter",
        );
        assert!(response.is_error());
        assert!(!response.is_success());

        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_PARAMS);
        assert!(error.message.contains("Missing"));
    }

    #[test]
    fn test_mcp_response_error_with_data() {
        let response = McpResponse::error_with_data(
            Some(serde_json::json!(1)),
            error_codes::INTERNAL_ERROR,
            "Internal error",
            serde_json::json!({"details": "stack trace"}),
        );

        let error = response.error.unwrap();
        assert!(error.data.is_some());
    }

    #[test]
    fn test_tool_definition() {
        let tool = Tool::new(
            "memory_search",
            "Search for memories",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        );

        assert_eq!(tool.name, "memory_search");
        assert!(tool.input_schema["required"].as_array().is_some());
    }

    #[test]
    fn test_tool_result_text() {
        let result = ToolResult::text("Hello, world!");
        assert!(!result.is_error);
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].content_type, "text");
        assert_eq!(result.content[0].text.as_ref().unwrap(), "Hello, world!");
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("Something went wrong");
        assert!(result.is_error);
        assert_eq!(
            result.content[0].text.as_ref().unwrap(),
            "Something went wrong"
        );
    }

    #[test]
    fn test_tool_content_text() {
        let content = ToolContent::text("Test content");
        assert_eq!(content.content_type, "text");
        assert!(content.text.is_some());
        assert!(content.mime_type.is_none());
    }

    #[test]
    fn test_tool_content_image() {
        let content = ToolContent::image("base64data", "image/png");
        assert_eq!(content.content_type, "image");
        assert!(content.data.is_some());
        assert_eq!(content.mime_type.as_ref().unwrap(), "image/png");
    }

    #[test]
    fn test_initialize_result() {
        let result = InitializeResult::new("engram", "0.1.0");
        assert_eq!(result.protocol_version, PROTOCOL_VERSION);
        assert_eq!(result.server_info.name, "engram");
        assert!(!result.capabilities.tools.list_changed);
    }

    #[test]
    fn test_request_serialization() {
        let request = McpRequest::new("test", Some(serde_json::json!(1)), None);
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("jsonrpc"));
        assert!(json.contains("2.0"));

        let parsed: McpRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.method, "test");
    }

    #[test]
    fn test_response_serialization() {
        let response = McpResponse::success(Some(serde_json::json!(1)), serde_json::json!({}));
        let json = serde_json::to_string(&response).unwrap();

        // Error should not be present when None
        assert!(!json.contains("error"));

        let parsed: McpResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_success());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::INVALID_REQUEST, -32600);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::INVALID_PARAMS, -32602);
        assert_eq!(error_codes::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_call_tool_params_serialization() {
        let params = CallToolParams {
            name: "memory_add".to_string(),
            arguments: Some(serde_json::json!({"user_id": "test"})),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: CallToolParams = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "memory_add");
        assert!(parsed.arguments.is_some());
    }

    #[test]
    fn test_list_tools_result_serialization() {
        let result = ListToolsResult {
            tools: vec![
                Tool::new("tool1", "First tool", serde_json::json!({})),
                Tool::new("tool2", "Second tool", serde_json::json!({})),
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: ListToolsResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.tools.len(), 2);
    }
}
