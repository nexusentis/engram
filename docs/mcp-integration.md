---
title: MCP Integration
sidebar_position: 4
description: "Model Context Protocol integration: stdio mode, HTTP transport, tool schemas, and session management."
---

# MCP Integration

Engram supports the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) for integration with AI assistants like Claude Desktop.

## Overview

MCP is a JSON-RPC 2.0 protocol that lets AI assistants call tools on external servers. Engram exposes four memory tools via MCP:

- **memory_add** — Extract and store memories from conversation text
- **memory_search** — Semantic search across stored memories
- **memory_get** — Retrieve a specific memory by ID
- **memory_delete** — Soft-delete a memory

## Stdio mode

For local integrations (Claude Desktop, direct piping), run the server in MCP stdio mode:

```bash
engram-server --mode mcp
```

In this mode, the server reads JSON-RPC messages from stdin and writes responses to stdout. No HTTP server is started.

### Claude Desktop configuration

Add to your Claude Desktop `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "engram": {
      "command": "/path/to/engram-server",
      "args": ["--mode", "mcp"],
      "env": {
        "OPENAI_API_KEY": "sk-..."
      }
    }
  }
}
```

## HTTP transport

For remote or multi-client scenarios, use the HTTP transport. The REST server includes MCP endpoints alongside the regular API.

### Session lifecycle

1. **Initialize**: `POST /mcp` with `method: "initialize"` (no session header needed)
2. **Use**: `POST /mcp` with `Mcp-Session-Id` header for subsequent requests
3. **Terminate**: `DELETE /mcp` with `Mcp-Session-Id` header

### Initialize

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
      "protocolVersion": "2024-11-05",
      "capabilities": {},
      "clientInfo": {"name": "my-client", "version": "1.0"}
    }
  }'
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {"tools": {}},
    "serverInfo": {"name": "engram", "version": "0.1.0"}
  }
}
```

The response includes a `Mcp-Session-Id` header. Use it for all subsequent requests.

### List tools

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id>" \
  -d '{"jsonrpc": "2.0", "id": 2, "method": "tools/list"}'
```

### Call a tool

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id>" \
  -d '{
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
      "name": "memory_search",
      "arguments": {
        "user_id": "alice",
        "query": "where does alice work?",
        "limit": 5
      }
    }
  }'
```

### Send a notification

Notifications (no `id` field) return `202 Accepted`:

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id>" \
  -d '{"jsonrpc": "2.0", "method": "notifications/initialized"}'
```

### Terminate session

```bash
curl -X DELETE http://localhost:8080/mcp \
  -H "Mcp-Session-Id: <session-id>"
```

## Tool schemas

### memory_add

```json
{
  "name": "memory_add",
  "description": "Add new memories from a conversation.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "user_id": {"type": "string", "description": "User identifier"},
      "content": {"type": "string", "description": "Conversation content to extract memories from"},
      "session_id": {"type": "string", "description": "Optional session identifier"}
    },
    "required": ["user_id", "content"]
  }
}
```

### memory_search

```json
{
  "name": "memory_search",
  "description": "Search for relevant memories using semantic search.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "user_id": {"type": "string", "description": "User identifier"},
      "query": {"type": "string", "description": "Natural language search query"},
      "limit": {"type": "integer", "default": 10, "minimum": 1, "maximum": 100},
      "min_confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
    },
    "required": ["user_id", "query"]
  }
}
```

### memory_get

```json
{
  "name": "memory_get",
  "description": "Retrieve a specific memory by its ID.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "user_id": {"type": "string", "description": "User identifier"},
      "memory_id": {"type": "string", "format": "uuid", "description": "Memory UUID"}
    },
    "required": ["user_id", "memory_id"]
  }
}
```

### memory_delete

```json
{
  "name": "memory_delete",
  "description": "Soft-delete a memory.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "user_id": {"type": "string", "description": "User identifier"},
      "memory_id": {"type": "string", "format": "uuid", "description": "Memory UUID"}
    },
    "required": ["user_id", "memory_id"]
  }
}
```

## Error handling

Tool call errors are returned as MCP tool results with `isError: true`:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [{"type": "text", "text": "Missing required argument: user_id"}],
    "isError": true
  }
}
```

Protocol-level errors use standard JSON-RPC error codes:

| Code | Meaning |
|------|---------|
| -32600 | Invalid JSON-RPC request |
| -32601 | Method not found |
| -32602 | Invalid parameters |

## Session management

- Sessions are stored in-memory on the server
- Idle sessions are reaped after `mcp_session_ttl_secs` (default: 30 minutes, configurable in TOML)
- A `POST /mcp` without `Mcp-Session-Id` header (on non-initialize requests) returns `400`
- A `DELETE /mcp` with an unknown session ID returns `404`
- MCP endpoints bypass authentication (they're in the auth skip paths)
