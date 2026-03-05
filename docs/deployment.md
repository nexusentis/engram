---
title: Deployment
sidebar_position: 6
description: "Docker, docker-compose, authentication setup, health monitoring, and production checklist."
---

# Deployment

## Docker (single container)

Build:

```bash
docker build -t engram-server .
```

Run:

```bash
docker run -p 8080:8080 \
  -e OPENAI_API_KEY=sk-... \
  -e ENGRAM_QDRANT_URL=http://host.docker.internal:6334 \
  engram-server
```

The Dockerfile uses a multi-stage build (rust:1.84-bookworm builder, debian:bookworm-slim runtime) and runs as a non-root `engram` user.

Default entrypoint: `engram-server --mode rest`.

## Docker Compose

The included `docker-compose.yml` starts both Qdrant and the server:

```yaml
services:
  qdrant:
    image: qdrant/qdrant:v1.17.0
    ports:
      - "6333:6333"   # REST API
      - "6334:6334"   # gRPC
    volumes:
      - qdrant_data:/qdrant/storage
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:6333/healthz"]
      interval: 5s
      timeout: 3s
      retries: 3

  engram-server:
    build: .
    ports:
      - "8080:8080"
    depends_on:
      qdrant:
        condition: service_healthy
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ENGRAM_QDRANT_URL=http://qdrant:6334

volumes:
  qdrant_data:
```

Start:

```bash
# Set your API key
export OPENAI_API_KEY=sk-...

# Start everything
docker compose up -d

# Initialize collections (first time only)
# Option 1: Run CLI against the containerized Qdrant
cargo run -p engram-cli -- init

# Option 2: Use the builder's init (via MemorySystemBuilder)
# The builder calls qdrant.initialize() automatically
```

## Authentication

### Generating tokens

```bash
# Generate a random 32-byte hex token
openssl rand -hex 32
```

### Enabling auth

Set `ENGRAM_API_TOKENS` with one or more comma-separated tokens:

```bash
export ENGRAM_API_TOKENS="abc123def456,xyz789ghi012"
```

Use `--require-auth` to enforce auth is configured at startup:

```bash
engram-server --require-auth
```

Without `--require-auth`, the server starts with a warning if `ENGRAM_API_TOKENS` is not set but does not fail.

### How it works

- Tokens are SHA-256 hashed before being stored in memory
- Comparison uses constant-time equality to prevent timing attacks
- Skip paths: `/health`, `/metrics`, `/openapi.json`, `/mcp` always bypass auth
- All `/v1/*` endpoints require `Authorization: Bearer <token>` when auth is enabled
- Invalid/missing tokens return `401 Unauthorized` with `WWW-Authenticate: Bearer`

## Health monitoring

### /health

```bash
curl http://localhost:8080/health
```

Returns 200 with `{"status": "healthy"}` when both Qdrant and SQLite are reachable. Returns 503 with `{"status": "degraded"}` and per-component status if either is down.

### /metrics

```bash
curl http://localhost:8080/metrics
```

Returns Prometheus-compatible text metrics.

### Docker healthcheck

The Dockerfile includes a built-in healthcheck:

```dockerfile
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1
```

## Production checklist

- [ ] **Auth enabled**: `ENGRAM_API_TOKENS` set, `--require-auth` flag used
- [ ] **CORS configured**: `cors_origins` set to specific domains (not `["*"]`)
- [ ] **Timeouts set**: `request_timeout_secs` appropriate for your workload (default: 60s)
- [ ] **Body limit set**: `body_limit_bytes` appropriate (default: 2 MiB)
- [ ] **MCP session TTL**: `mcp_session_ttl_secs` appropriate (default: 1800s = 30 min)
- [ ] **Collections initialized**: Run `engram init` before first server start
- [ ] **Qdrant persistence**: Volume mounted for Qdrant data (`qdrant_data:/qdrant/storage`)
- [ ] **OPENAI_API_KEY**: Set and valid
- [ ] **Logging**: `RUST_LOG` configured (e.g., `RUST_LOG=info,engram_server=debug`)
- [ ] **Health monitoring**: Health check endpoint monitored by your infrastructure

## CI/CD

The repository includes GitHub Actions workflows:

- **ci.yml**: Runs `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt --check` on every push. Includes a Qdrant service container for integration tests.
- **cd.yml**: Builds and pushes Docker images on tagged releases.

Integration tests are gated by `ENGRAM_TEST_QDRANT_URL`. In CI, this is set automatically by the Qdrant service container. Locally:

```bash
ENGRAM_TEST_QDRANT_URL=http://localhost:6334 cargo test --package engram-server
```

## Graceful shutdown

The server handles `SIGINT` (Ctrl+C) and `SIGTERM` gracefully, draining in-flight requests before exiting. This works with Docker's stop signal and Kubernetes pod termination.
