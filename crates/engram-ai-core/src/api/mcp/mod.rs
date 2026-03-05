//! MCP (Model Context Protocol) server implementation
//!
//! Provides JSON-RPC 2.0 based protocol support for AI tool integration.
//! Supports stdio transport for CLI tools (Claude Desktop, Cursor) and
//! SSE transport for web integration.
//!
//! ## Supported Methods
//!
//! - `initialize` - Protocol handshake
//! - `initialized` - Confirm initialization complete
//! - `ping` - Health check
//! - `tools/list` - List available tools
//! - `tools/call` - Execute a tool
//!
//! ## Available Tools
//!
//! - `memory_add` - Add memories from conversation content
//! - `memory_search` - Search for relevant memories
//! - `memory_get` - Retrieve a specific memory by ID
//! - `memory_delete` - Soft-delete a memory
//!
//! ## Usage
//!
//! ```rust,ignore
//! use engram_core::api::mcp::{McpHandler, run_stdio_server};
//!
//! let handler = McpHandler::default_server();
//! run_stdio_server(handler).expect("Server failed");
//! ```

mod handler;
mod stdio;
mod types;

pub use handler::{McpHandler, ToolDefinitions};
pub use stdio::{process_message, run_stdio_server, StdioConfig, StdioServer};
pub use types::{
    error_codes, CallToolParams, ClientInfo, InitializeParams, InitializeResult, ListToolsResult,
    McpError, McpRequest, McpResponse, ServerCapabilities, ServerInfo, Tool, ToolContent,
    ToolResult, ToolsCapability, PROTOCOL_VERSION,
};

// SSE transport requires axum which is not yet a dependency
// TODO(Task 007-03): Add SSE transport when axum is added
// mod sse;
// pub use sse::{mcp_message_handler, mcp_sse_handler, SseParams};
