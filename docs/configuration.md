---
title: Configuration
sidebar_position: 5
description: "TOML config file, environment variables, CLI flags, and validation rules."
---

# Configuration

Engram uses a TOML config file, environment variables, and CLI flags. Precedence (highest wins): CLI flags > environment variables > TOML file > defaults.

## Config file

Default path: `config/engram.toml` (override with `--config`).

```toml
data_dir = "./data"

[server]
host = "0.0.0.0"
port = 8080

[server.security]
body_limit_bytes = 2097152        # 2 MiB
cors_origins = []                 # empty = same-origin only; ["*"] = allow all
request_timeout_secs = 60
mcp_session_ttl_secs = 1800       # 30 minutes

[qdrant]
mode = "external"                 # "external" (requires url) or "embedded" (requires path)
url = "http://localhost:6334"
vector_size = 1536

[sqlite]
path = "./data/engram.db"
wal_mode = true
busy_timeout_ms = 5000

[extraction]
mode = "api-sota"                 # "local-fast", "local-accurate", or "api-sota"
api_model = "gpt-4o-mini"
confidence_threshold = 0.5

[retrieval]
rrf_k = 60                        # Reciprocal Rank Fusion k parameter
top_k = 20                        # Results per collection before fusion
abstention_threshold = 0.4        # Below this, search abstains
```

## Environment variables

Only the following environment variables are actually wired to server behavior:

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENAI_API_KEY` | Yes | OpenAI API key for embeddings and extraction |
| `ENGRAM_QDRANT_URL` | No | Overrides `qdrant.url` from TOML |
| `ENGRAM_API_TOKENS` | No | Comma-separated bearer tokens for auth (e.g., `token1,token2`) |
| `RUST_LOG` | No | Tracing filter (e.g., `info`, `engram_server=debug`) |

### Setting up auth

```bash
# Generate random tokens
TOKEN1=$(openssl rand -hex 32)
TOKEN2=$(openssl rand -hex 32)
export ENGRAM_API_TOKENS="$TOKEN1,$TOKEN2"

# Start server with auth enforcement
engram-server --require-auth
```

Tokens are hashed with SHA-256 before being stored in memory. The server never logs plaintext tokens.

### Variables in `.env.example` that are NOT wired

The following variables appear in `.env.example` but are **not currently read** by the server. They are used by the benchmark harness or are planned for future implementation:

| Variable | Status |
|----------|--------|
| `ENGRAM_HOST` | Not wired — use `--host` flag or TOML `server.host` |
| `ENGRAM_PORT` | Not wired — use `--port` flag or TOML `server.port` |
| `ENGRAM_EXTRACTION_MODE` | Not wired — use TOML `extraction.mode` |
| `ENGRAM_CONFIDENCE_THRESHOLD` | Not wired — use TOML `extraction.confidence_threshold` |
| `ENGRAM_RERANKING` | Not wired |
| `ENGRAM_TOP_K` | Not wired — use TOML `retrieval.top_k` |
| `ENGRAM_RRF_K` | Not wired — use TOML `retrieval.rrf_k` |
| `ANTHROPIC_API_KEY` | Not used by server |
| `RERANKER_*` | Benchmark-only |
| `BENCHMARK_CONFIG`, `FULL_BENCHMARK`, etc. | Benchmark harness |

## CLI flags

### engram-server

```
engram-server [OPTIONS]

Options:
  --mode <MODE>        Server mode: rest (default) or mcp
  --host <HOST>        Bind address (overrides TOML)
  --port <PORT>        Listen port (overrides TOML)
  --config <PATH>      Config file path (default: config/engram.toml)
  --require-auth       Fail at startup if ENGRAM_API_TOKENS is not set
```

### engram (CLI)

```
engram [OPTIONS] <COMMAND>

Commands:
  init     Initialize the memory system (Qdrant collections + SQLite)
  status   Show system health and collection counts
  config   View or update configuration

Options:
  -c, --config <PATH>   Config file (default: config.toml)
  -v, --verbose          Increase log verbosity (-v debug, -vv trace)

Examples:
  engram init                           # Initialize with defaults
  engram init --data-dir ./my-data      # Custom data directory
  engram init --force                   # Re-initialize
  engram status                         # Text output
  engram status -o json                 # JSON output
  engram config --list                  # Show all settings
  engram config server.port             # Get a value
  engram config server.port 9000        # Set a value
```

## Config validation

The `Config::validate()` method enforces these constraints:

| Constraint | Error |
|-----------|-------|
| `qdrant.vector_size > 0` | "qdrant.vector_size must be > 0" |
| `qdrant.mode = "external"` requires `qdrant.url` | "qdrant.url is required when mode = external" |
| `qdrant.mode = "embedded"` requires `qdrant.path` | "qdrant.path is required when mode = embedded" |
| `retrieval.rrf_k > 0` | "retrieval.rrf_k must be > 0" |
| `retrieval.top_k > 0` | "retrieval.top_k must be > 0" |
| `retrieval.abstention_threshold` in [0.0, 1.0] | "retrieval.abstention_threshold must be in [0.0, 1.0]" |
| `extraction.confidence_threshold` in [0.0, 1.0] | "extraction.confidence_threshold must be in [0.0, 1.0]" |
| `server.security.body_limit_bytes > 0` | "server.security.body_limit_bytes must be > 0" |
| `server.security.request_timeout_secs > 0` | "server.security.request_timeout_secs must be > 0" |
| `server.security.mcp_session_ttl_secs > 0` | "server.security.mcp_session_ttl_secs must be > 0" |

Use `Config::load_strict(path)` to enforce validation on load (returns errors instead of silently falling back to defaults).

## CORS configuration

The `cors_origins` field in `[server.security]` controls CORS behavior:

| Value | Behavior |
|-------|----------|
| `[]` (empty) | Same-origin only (no CORS headers) |
| `["*"]` | Permissive (allow all origins) |
| `["https://app.example.com"]` | Allow specific origins |

When specific origins are configured, the server allows `GET`, `POST`, `DELETE` methods and exposes `Authorization`, `Content-Type`, `Mcp-Session-Id`, and `X-Request-Id` headers.
