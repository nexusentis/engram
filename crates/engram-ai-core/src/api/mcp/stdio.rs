//! MCP stdio transport
//!
//! Handles JSON-RPC messages over standard input/output.

use std::io::{self, BufRead, Write};

use super::handler::McpHandler;
use super::types::{error_codes, McpRequest, McpResponse};

/// Run MCP server over stdio (synchronous)
///
/// Reads JSON-RPC requests from stdin (one per line) and writes
/// responses to stdout. This is the standard transport for CLI
/// tool integration (e.g., Claude Desktop, Cursor).
pub fn run_stdio_server(mut handler: McpHandler) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Parse request
        let response = match serde_json::from_str::<McpRequest>(line) {
            Ok(request) => handler.handle(request),
            Err(e) => McpResponse::error(
                None,
                error_codes::PARSE_ERROR,
                format!("Parse error: {}", e),
            ),
        };

        // Write response
        let output = serde_json::to_string(&response)?;
        writeln!(stdout, "{}", output)?;
        stdout.flush()?;
    }

    Ok(())
}

/// Process a single MCP message
///
/// Useful for testing or when you want to handle messages individually.
pub fn process_message(handler: &mut McpHandler, message: &str) -> String {
    let response = match serde_json::from_str::<McpRequest>(message) {
        Ok(request) => handler.handle(request),
        Err(e) => McpResponse::error(
            None,
            error_codes::PARSE_ERROR,
            format!("Parse error: {}", e),
        ),
    };

    serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Failed to serialize response"}}"#
            .to_string()
    })
}

/// Stdio transport configuration
#[derive(Debug, Clone, Default)]
pub struct StdioConfig {
    /// Whether to log messages for debugging
    pub debug_logging: bool,
    /// Maximum message size in bytes (0 = unlimited)
    pub max_message_size: usize,
}

impl StdioConfig {
    /// Create a new config
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable debug logging
    pub fn with_debug_logging(mut self, enabled: bool) -> Self {
        self.debug_logging = enabled;
        self
    }

    /// Set maximum message size
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }
}

/// Stdio server with configuration
pub struct StdioServer {
    handler: McpHandler,
    config: StdioConfig,
}

impl StdioServer {
    /// Create a new stdio server
    pub fn new(handler: McpHandler) -> Self {
        Self {
            handler,
            config: StdioConfig::default(),
        }
    }

    /// Create with configuration
    pub fn with_config(handler: McpHandler, config: StdioConfig) -> Self {
        Self { handler, config }
    }

    /// Process a single line of input
    pub fn process_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            return None;
        }

        // Check message size limit
        if self.config.max_message_size > 0 && line.len() > self.config.max_message_size {
            let response = McpResponse::error(
                None,
                error_codes::INVALID_REQUEST,
                format!(
                    "Message too large: {} bytes (max: {})",
                    line.len(),
                    self.config.max_message_size
                ),
            );
            return serde_json::to_string(&response).ok();
        }

        Some(process_message(&mut self.handler, line))
    }

    /// Run the server
    pub fn run(mut self) -> io::Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        for line in stdin.lock().lines() {
            let line = line?;

            if let Some(response) = self.process_line(&line) {
                if self.config.debug_logging {
                    eprintln!("MCP Request: {}", line.trim());
                    eprintln!("MCP Response: {}", response);
                }

                writeln!(stdout, "{}", response)?;
                stdout.flush()?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_handler() -> McpHandler {
        McpHandler::new("test-server", "1.0.0")
    }

    #[test]
    fn test_process_message_valid() {
        let mut handler = create_handler();
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;

        let response = process_message(&mut handler, message);
        let parsed: McpResponse = serde_json::from_str(&response).unwrap();

        assert!(parsed.is_success());
    }

    #[test]
    fn test_process_message_invalid_json() {
        let mut handler = create_handler();
        let message = "not valid json";

        let response = process_message(&mut handler, message);
        let parsed: McpResponse = serde_json::from_str(&response).unwrap();

        assert!(parsed.is_error());
        assert_eq!(parsed.error.unwrap().code, error_codes::PARSE_ERROR);
    }

    #[test]
    fn test_process_message_initialize() {
        let mut handler = create_handler();
        let message = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            }
        }))
        .unwrap();

        let response = process_message(&mut handler, &message);
        let parsed: McpResponse = serde_json::from_str(&response).unwrap();

        assert!(parsed.is_success());
        let result = parsed.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "test-server");
    }

    #[test]
    fn test_process_message_list_tools() {
        let mut handler = create_handler();
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

        let response = process_message(&mut handler, message);
        let parsed: McpResponse = serde_json::from_str(&response).unwrap();

        assert!(parsed.is_success());
        let result = parsed.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
    }

    #[test]
    fn test_stdio_config_default() {
        let config = StdioConfig::default();
        assert!(!config.debug_logging);
        assert_eq!(config.max_message_size, 0);
    }

    #[test]
    fn test_stdio_config_builder() {
        let config = StdioConfig::new()
            .with_debug_logging(true)
            .with_max_message_size(1024);

        assert!(config.debug_logging);
        assert_eq!(config.max_message_size, 1024);
    }

    #[test]
    fn test_stdio_server_process_empty_line() {
        let handler = create_handler();
        let mut server = StdioServer::new(handler);

        let result = server.process_line("");
        assert!(result.is_none());

        let result = server.process_line("   ");
        assert!(result.is_none());
    }

    #[test]
    fn test_stdio_server_process_valid_line() {
        let handler = create_handler();
        let mut server = StdioServer::new(handler);

        let result = server.process_line(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        assert!(result.is_some());

        let response: McpResponse = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(response.is_success());
    }

    #[test]
    fn test_stdio_server_message_too_large() {
        let handler = create_handler();
        let config = StdioConfig::new().with_max_message_size(10);
        let mut server = StdioServer::with_config(handler, config);

        let result = server.process_line(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        assert!(result.is_some());

        let response: McpResponse = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(response.is_error());
        assert!(response.error.unwrap().message.contains("too large"));
    }

    #[test]
    fn test_stdio_server_creation() {
        let handler = create_handler();
        let server = StdioServer::new(handler);

        assert!(!server.config.debug_logging);
    }

    #[test]
    fn test_stdio_server_with_config() {
        let handler = create_handler();
        let config = StdioConfig::new().with_debug_logging(true);
        let server = StdioServer::with_config(handler, config);

        assert!(server.config.debug_logging);
    }

    #[test]
    fn test_process_tools_call() {
        let mut handler = create_handler();
        let message = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "memory_search",
                "arguments": {
                    "user_id": "test-user",
                    "query": "test query"
                }
            }
        }))
        .unwrap();

        let response = process_message(&mut handler, &message);
        let parsed: McpResponse = serde_json::from_str(&response).unwrap();

        assert!(parsed.is_success());
    }
}
