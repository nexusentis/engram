---
title: Limitations
sidebar_position: 9
description: "Known gaps, unimplemented features, and current constraints."
---

# Limitations

Known gaps and unimplemented features in the current release.

## Unimplemented API fields

The following request fields are accepted by the API (they won't cause errors) but have no effect:

| Endpoint | Field | Status |
|----------|-------|--------|
| `POST /v1/memories/search` | `include_history` | Accepted, ignored. Version history is never returned in search results. |
| `GET /v1/memories` | `include_expired` | Accepted, ignored. Expired memories are always filtered out. |

These fields exist in the API types for forward compatibility but are not yet wired to handler logic.

## Messages collection

`POST /v1/messages/search` searches the `messages` Qdrant collection. However, the ingestion endpoint (`POST /v1/memories`) only stores **extracted facts** — it does not populate the `messages` collection with raw conversation messages.

This means `POST /v1/messages/search` will return empty results unless you populate the `messages` collection through another mechanism (e.g., direct Qdrant API calls or the Rust SDK).

## Environment variable overrides

Several environment variables listed in `.env.example` are **not wired** to the server:

- `ENGRAM_HOST`, `ENGRAM_PORT` — use `--host`/`--port` CLI flags or TOML config instead
- `ENGRAM_EXTRACTION_MODE`, `ENGRAM_CONFIDENCE_THRESHOLD` — use TOML config
- `ENGRAM_RERANKING`, `ENGRAM_TOP_K`, `ENGRAM_RRF_K` — use TOML config

Only `OPENAI_API_KEY`, `ENGRAM_QDRANT_URL`, `ENGRAM_API_TOKENS`, and `RUST_LOG` are read by the server. See [Configuration](configuration) for the full truth table.

## Collection initialization

The server does not create Qdrant collections on startup. You must run `engram init` (CLI) before starting the server for the first time. If collections don't exist, the server will return 500 errors on memory operations.

The `MemorySystemBuilder` (Rust SDK) does create collections automatically via `qdrant.initialize()`.

## No multi-tenancy controls

The current server has no:

- Per-user rate limiting
- Per-user storage quotas
- Admin API for user management
- Tenant isolation beyond `user_id` filtering

All users share the same Qdrant collections and are isolated only by `user_id` payload filtering.

## No streaming

All API responses are buffered. There is no streaming support for large result sets or long-running operations.

## MCP session storage

MCP sessions are stored in-memory. They do not survive server restarts. A server restart terminates all active MCP sessions — clients must re-initialize.
