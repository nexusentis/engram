---
title: Getting Started
sidebar_position: 1
description: "Install, configure, and run your first ingest and search with Engram."
---

# Getting Started

## Prerequisites

- **Rust** 1.84+ (for building from source)
- **Qdrant** vector database (v1.17+)
- **OpenAI API key** (for embeddings and fact extraction)

## 1. Start Qdrant

The fastest way is Docker:

```bash
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant:v1.17.0
```

Or use the included docker-compose (starts both Qdrant and the server):

```bash
docker compose up -d qdrant
```

## 2. Build from source

```bash
git clone https://github.com/your-org/engram.git
cd engram
cargo build --release
```

This produces two binaries:

- `target/release/engram` — CLI tool
- `target/release/engram-server` — REST/MCP server

## 3. Configure

Copy the example environment file and set your API key:

```bash
cp .env.example .env
# Edit .env — set OPENAI_API_KEY
```

The server reads configuration from `config/engram.toml` by default. The included config works out of the box with a local Qdrant instance.

## 4. Initialize

Create Qdrant collections and the SQLite database:

```bash
./target/release/engram init
```

This creates four Qdrant collections (`world`, `experience`, `opinion`, `observation`) and a SQLite database for audit logging.

**Important:** The server does not create collections on startup — you must run `engram init` first.

## 5. Start the server

```bash
OPENAI_API_KEY=sk-... ./target/release/engram-server
```

The server listens on `0.0.0.0:8080` by default. Verify it's running:

```bash
curl http://localhost:8080/health
```

```json
{"status": "healthy", "qdrant": true, "sqlite": true, "version": "0.1.0"}
```

## 6. Ingest a conversation

```bash
curl -X POST http://localhost:8080/v1/memories \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "alice",
    "messages": [
      {"role": "user", "content": "I just started a new job at Anthropic on the safety team."},
      {"role": "assistant", "content": "Congratulations! How are you finding it so far?"},
      {"role": "user", "content": "It is great, I moved to San Francisco for it."}
    ]
  }'
```

Response:

```json
{
  "memory_ids": ["019...", "019..."],
  "facts_extracted": 2,
  "entities_found": ["alice", "anthropic", "san_francisco"],
  "processing_time_ms": 1234
}
```

The server extracts structured facts from the conversation (e.g., "Alice works at Anthropic on the safety team", "Alice moved to San Francisco") and stores them as vector embeddings in Qdrant.

## 7. Search memories

```bash
curl -X POST http://localhost:8080/v1/memories/search \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "alice",
    "query": "where does alice work?",
    "limit": 5
  }'
```

Response:

```json
{
  "memories": [
    {
      "id": "019...",
      "content": "Alice works at Anthropic on the safety team",
      "confidence": 0.95,
      "score": 0.89,
      "fact_type": "State",
      "entities": ["alice", "anthropic"],
      "t_valid": "2025-01-15T10:30:00Z",
      "t_created": "2025-01-15T10:30:00Z"
    }
  ],
  "total_found": 1,
  "search_time_ms": 45,
  "abstained": false
}
```

## Next steps

- [API Reference](api-reference) — all endpoints with full request/response schemas
- [Concepts](concepts) — understand the memory model (fact types, entities, temporal)
- [Configuration](configuration) — tune extraction, retrieval, and security settings
- [MCP Integration](mcp-integration) — connect MCP clients
- [Deployment](deployment) — run in production with Docker and auth
