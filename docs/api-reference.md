---
title: API Reference
sidebar_position: 3
description: "REST API endpoints, request/response schemas, authentication, and error handling."
---

# API Reference

Base URL: `http://localhost:8080`

All endpoints return JSON. Errors use the standard format:

```json
{"error": "Description", "code": "ERROR_CODE"}
```

Error codes: `VALIDATION_ERROR`, `NOT_FOUND`, `INTERNAL_ERROR`, `UNAUTHORIZED`.

## Authentication

When auth is enabled (see [Configuration](configuration)), all `/v1/*` endpoints require a bearer token:

```
Authorization: Bearer <token>
```

Skip paths (no auth required): `/health`, `/metrics`, `/openapi.json`, `/mcp`.

See the [auth setup section in Deployment](deployment#authentication) for generating tokens.

---

## POST /v1/memories

Ingest a conversation and extract memories from it.

**Auth required:** Yes

### Request

```json
{
  "user_id": "alice",
  "messages": [
    {"role": "user", "content": "I work at Anthropic"},
    {"role": "assistant", "content": "That's interesting!"}
  ],
  "session_id": "session-123",
  "metadata": {"source": "chat"}
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `user_id` | string | Yes | User identifier (1-256 chars) |
| `messages` | array | Yes | Conversation messages (at least 1) |
| `messages[].role` | string | Yes | `"user"`, `"assistant"`, or `"system"` |
| `messages[].content` | string | Yes | Message text |
| `messages[].timestamp` | string | No | ISO 8601 datetime |
| `session_id` | string | No | Group related conversations |
| `metadata` | object | No | Arbitrary JSON metadata |

### Response (200)

```json
{
  "memory_ids": ["019...", "019..."],
  "facts_extracted": 2,
  "entities_found": ["alice", "anthropic"],
  "processing_time_ms": 1234
}
```

### Errors

| Status | When |
|--------|------|
| 400 | Empty `user_id`, `user_id` > 256 chars, empty `messages` array |
| 401 | Missing or invalid auth token (when auth enabled) |
| 500 | Extraction or storage failure |

### What happens during ingestion

1. The LLM extractor (`gpt-4o-mini` by default) analyzes the conversation
2. Structured facts are extracted with types, entities, and confidence scores
3. Each fact is embedded using OpenAI `text-embedding-3-small`
4. Vectors are upserted into the appropriate Qdrant collection based on epistemic type
5. The response returns the IDs of all stored memories

**Important:** Only extracted facts are stored, not the raw messages. See [Limitations](limitations).

---

## POST /v1/memories/search

Semantic search across stored memories.

**Auth required:** Yes

### Request

```json
{
  "user_id": "alice",
  "query": "where does alice work?",
  "limit": 10,
  "include_history": false,
  "filters": {
    "fact_types": ["State"],
    "entity_ids": ["anthropic"],
    "min_confidence": 0.5,
    "time_range": {
      "start": "2025-01-01T00:00:00Z",
      "end": "2025-12-31T23:59:59Z"
    }
  }
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `user_id` | string | Yes | — | User to search (1-256 chars) |
| `query` | string | Yes | — | Natural language query (max 10,000 chars) |
| `limit` | integer | No | 10 | Max results (1-100) |
| `include_history` | bool | No | false | **Unimplemented** — accepted but ignored |
| `filters.fact_types` | string[] | No | — | Filter by fact type (`State`, `Event`, `Preference`, `Relation`) |
| `filters.entity_ids` | string[] | No | — | Filter by entity normalized IDs |
| `filters.min_confidence` | float | No | — | Minimum confidence (0.0-1.0) |
| `filters.time_range.start` | string | No | — | Inclusive start (ISO 8601) |
| `filters.time_range.end` | string | No | — | Exclusive end (ISO 8601). Must be after `start`. |

### Response (200)

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
      "t_created": "2025-01-15T10:30:00Z",
      "supersedes_id": null
    }
  ],
  "total_found": 1,
  "search_time_ms": 45,
  "abstained": false,
  "abstention_reason": null
}
```

### Errors

| Status | When |
|--------|------|
| 400 | Empty `user_id`, `user_id` > 256 chars, empty `query`, `query` > 10k, `limit` = 0, `min_confidence` out of range, `time_range.start >= end` |
| 401 | Missing or invalid auth token |
| 500 | Embedding or search failure |

---

## GET /v1/memories

List memories for a user (paginated).

**Auth required:** Yes

### Query parameters

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `user_id` | string | Yes | — | User identifier (1-256 chars) |
| `limit` | integer | No | 10 | Max results (min: 1) |
| `offset` | integer | No | 0 | Pagination offset |
| `include_expired` | bool | No | false | **Unimplemented** — accepted but ignored |

### Example

```bash
curl "http://localhost:8080/v1/memories?user_id=alice&limit=20&offset=0"
```

### Response (200)

```json
{
  "memories": [...],
  "total": 42,
  "limit": 20,
  "offset": 0
}
```

### Errors

| Status | When |
|--------|------|
| 400 | Empty `user_id`, `user_id` > 256 chars, `limit` = 0 |

---

## GET /v1/memories/:id

Retrieve a single memory by ID.

**Auth required:** Yes

### Query parameters

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `user_id` | string | Yes | User identifier (whitespace-only is rejected) |

### Example

```bash
curl "http://localhost:8080/v1/memories/019abc...?user_id=alice"
```

### Response (200)

```json
{
  "memory": {
    "id": "019abc...",
    "user_id": "alice",
    "content": "Alice works at Anthropic",
    "confidence": 0.95,
    "source_type": "UserExplicit",
    "fact_type": "State",
    "epistemic_type": "World",
    "entities": ["alice", "anthropic"],
    "t_valid": "2025-01-15T10:30:00Z",
    "t_created": "2025-01-15T10:30:00Z",
    "t_expired": null,
    "supersedes_id": null,
    "derived_from_ids": [],
    "is_latest": true
  }
}
```

### Errors

| Status | When |
|--------|------|
| 400 | Missing or whitespace-only `user_id` |
| 404 | Memory not found for this user |

---

## DELETE /v1/memories/:id

Soft-delete a memory (marks as expired, sets `is_latest = false`).

**Auth required:** Yes

### Query parameters

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `user_id` | string | Yes | User identifier (whitespace-only is rejected) |

### Example

```bash
curl -X DELETE "http://localhost:8080/v1/memories/019abc...?user_id=alice"
```

### Response (200)

```json
{
  "id": "019abc...",
  "deleted": true,
  "deleted_at": "2025-06-15T12:00:00Z"
}
```

### Errors

| Status | When |
|--------|------|
| 400 | Missing or whitespace-only `user_id` |
| 404 | Memory not found for this user |

---

## POST /v1/messages/search

Search raw conversation messages. Uses the `messages` Qdrant collection.

**Auth required:** Yes

### Request

```json
{
  "user_id": "alice",
  "query": "what did we discuss about the project?",
  "limit": 10
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `user_id` | string | Yes | — | User to search (1-256 chars) |
| `query` | string | Yes | — | Search query (max 10,000 chars) |
| `limit` | integer | No | 10 | Max results (min: 1) |

**Note:** This endpoint depends on the `messages` collection being populated. The current ingestion pipeline (`POST /v1/memories`) does not store raw messages — see [Limitations](limitations).

### Errors

| Status | When |
|--------|------|
| 400 | Empty `user_id`, `query` > 10k, `limit` = 0 |

---

## GET /health

Health check. No authentication required.

### Response (200)

```json
{
  "status": "healthy",
  "qdrant": true,
  "sqlite": true,
  "version": "0.1.0"
}
```

Returns 503 if Qdrant or SQLite is unreachable:

```json
{
  "status": "degraded",
  "qdrant": false,
  "sqlite": true,
  "version": "0.1.0"
}
```

---

## GET /metrics

Prometheus-compatible metrics. No authentication required.

Returns text/plain metrics for request counts, latencies, and memory operation statistics.

---

## GET /openapi.json

OpenAPI 3.0 specification. No authentication required.

Returns the full API specification including all endpoint schemas. Useful for generating client SDKs or viewing in Swagger UI.

---

## Response headers

All responses include:

| Header | Description |
|--------|-------------|
| `x-request-id` | Unique request ID (echoed back if provided, generated if not) |
| `x-content-type-options` | `nosniff` |
| `x-frame-options` | `DENY` |
