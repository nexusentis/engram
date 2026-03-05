# Engram Server Productionization Roadmap

Tracks all server work from crate restructuring through SaaS-readiness.

**Current state**: Phase 6b complete. Next: Phase 7.

---

## Phase Overview

| Phase | Name | Status |
|-------|------|--------|
| 0 | Behavior Freeze + Golden Tests | **Done** |
| 1 | Crate Boundaries (Move First, Refactor Later) | **Done** |
| 2 | Break Up the Monoliths | **Done** |
| 3 | Observability (Make It Debuggable) | **Done** |
| 4 | Auth Hardening (Make It Safe) | **Done** |
| 5 | HTTP Server Gaps (Make REST Complete) | **Done** |
| 6 | CI/CD Hardening (Make It Shippable) | **Done** |
| 6b | OpenAPI + MCP Streamable HTTP (API Surface) | **Done** |
| 7 | Multi-Tenant Controls (Make It SaaS-Ready) | Not started |

---

## Phase 0: Behavior Freeze + Golden Tests — DONE

Baseline snapshot before refactoring.

---

## Phase 1: Crate Boundaries — DONE

Split monolithic crate into 6-crate workspace:
- [`engram-ai-core`](https://crates.io/crates/engram-ai-core) (foundation: types, storage, extraction, embedding, LLM client)
- [`engram-agent`](https://crates.io/crates/engram-agent) (generic reusable agent loop)
- `engram-bench` (benchmark-specific logic, not published)
- [`engram-server`](https://crates.io/crates/engram-server) (REST + MCP server binary)
- [`engram-cli`](https://crates.io/crates/engram-cli) (CLI binary)
- [`engram-ai`](https://crates.io/crates/engram-ai) (re-export facade)

---

## Phase 2: Break Up the Monoliths — DONE

Clean module splits. Removed 12,000+ LOC of dead code (ONNX reranker, temporal solver, background workers, conflict resolution).

---

## Phase 3: Observability (Make It Debuggable) — DONE

- [x] Prometheus `/metrics` endpoint with domain metrics
- [x] `TraceLayer` for structured per-request tracing spans
- [x] Request metrics middleware: method, path, status, duration
- [x] Domain metrics: `record_ingestion()` and `record_retrieval()`

---

## Phase 4: Auth Hardening (Make It Safe) — DONE

- [x] `--require-auth` CLI flag — fails fast before Qdrant/OpenAI init
- [x] Startup warning when auth is disabled
- [x] `AuthState` propagated into request extensions
- [x] `WWW-Authenticate: Bearer` header on 401 (RFC 6750)
- [x] Auth failures logged at `debug` (prevents log-flood DoS)
- [x] `SecurityConfig` in TOML: `body_limit_bytes`, `cors_origins`
- [x] `CorsLayer` — configurable origins (same-origin / wildcard / explicit)
- [x] `DefaultBodyLimit` — configurable, default 2 MiB
- [x] Layer ordering fixed: cors -> trace -> metrics -> auth -> body_limit -> handler

**Deferred to Phase 7**: token-to-user binding, per-user quotas, rate limiting, admin API.

---

## Phase 5: HTTP Server Gaps (Make REST Complete) — DONE

- [x] Wire `UserMemoriesRequest`/`UserMemoriesResponse` to `GET /v1/memories` list endpoint with pagination
- [x] Add input validation layer (empty `user_id`, empty `query`, empty `messages`)
- [x] Enable response compression (`CompressionLayer` with gzip)
- [x] Add graceful shutdown (`tokio::signal` for SIGTERM/SIGINT with connection draining)
- [x] Add request timeouts (`TimeoutLayer`, 60s)
- [x] Add security headers (`X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`)
- [x] Add `user_id` field to `UserMemoriesRequest` (with `serde(default)`)

**Deferred**: SSE MCP transport (separate PR), OpenAPI spec via `utoipa` (separate PR).

---

## Phase 6: CI/CD Hardening (Make It Shippable) — DONE

Split into 6 (CI/CD + container) and 6b (API surface) per Codex review: OpenAPI and SSE are feature additions, not infrastructure hardening.

### CI hardening
- [x] Add `cargo audit` to CI
- [x] Fix the `tee` SIGPIPE issue in the regression workflow (redirect-then-cat pattern)
- [x] Add Qdrant service container to CI + run `#[ignore]` integration tests

### Container hardening
- [x] Fix `ENGRAM_QDRANT_URL` env var override in REST path (`AppState::from_config`)
- [x] Update Dockerfile: REST mode default, `EXPOSE 8080`, `HEALTHCHECK`, install `curl`
- [x] Update `docker-compose.yml`: REST mode, port 8080, remove `stdin_open`
- [x] Add readiness semantics to `/health` (return 503 when degraded, not 200)

### CD
- [x] Add CD workflow (`cd.yml`): build Docker image on tag push, push to GHCR

---

## Phase 6b: OpenAPI + MCP Streamable HTTP (API Surface) — DONE

API surface expansion.

- [x] Add `utoipa` v5 dependency with `chrono`/`uuid` features
- [x] `#[derive(ToSchema)]` on all 19 API types (16 in engram-ai-core, 3 in engram-server)
- [x] `#[derive(IntoParams)]` on query param types (`GetMemoryQuery`, `DeleteQuery`, `UserMemoriesRequest`)
- [x] `#[utoipa::path]` annotations on all 7 API handlers
- [x] `#[derive(OpenApi)]` with full schema registry and tag definitions
- [x] `GET /openapi.json` endpoint (unauthenticated)
- [x] MCP Streamable HTTP transport (`POST /mcp` + `DELETE /mcp`)
  - Session management via `Mcp-Session-Id` header
  - Per-session `McpHandler` instances with `Arc<Mutex<McpHandler>>`
  - Handler calls run on `spawn_blocking` to avoid blocking Tokio runtime
  - Reuses existing `MemorySystem` backend (shared Qdrant/embedder/extractor)
- [x] Auth skip paths: `/openapi.json` and `/mcp` added to defaults

**Deferred**:
- GET /mcp SSE stream for server-initiated messages (no use case yet)
- HTTP handler tests (requires `lib` target or test harness; moved to Phase 7)

---

## Phase 7: Multi-Tenant Controls (Make It SaaS-Ready) — NOT STARTED

Currently any authenticated client can read/write any `user_id`.

- [ ] Per-token `user_id` scoping (token -> allowed user_ids)
- [ ] Per-user ingestion quotas
- [ ] Admin API (list users, memory counts, purge user data)
- [ ] Rate limiting via Tower middleware
- [ ] Request ID middleware (correlation IDs for distributed tracing)
- [ ] HTTP handler tests (deferred from Phase 6b; needs `lib` target or mock AppState)

---

## Feature Inventory (cross-cutting)

Quick reference for what's done vs missing across all phases.

| # | Feature | Status | Phase |
|---|---------|--------|-------|
| 1 | REST endpoints (7 routes) | Done | 1 |
| 2 | MCP server (stdio) | Done | 1 |
| 3 | Bearer token auth (SHA-256, constant-time) | Done | 1 |
| 4 | Health check (Qdrant + SQLite) | Done | 1 |
| 5 | Error handling (consistent `AppError`) | Done | 1 |
| 6 | Docker + Compose | Done | 1 |
| 7 | Prometheus metrics | Done | 3 |
| 8 | Structured tracing (`TraceLayer`) | Done | 3 |
| 9 | Request metrics middleware | Done | 3 |
| 10 | `--require-auth` fail-fast | Done | 4 |
| 11 | `AuthState` in request extensions | Done | 4 |
| 12 | CORS (configurable) | Done | 4 |
| 13 | Body size limit (configurable) | Done | 4 |
| 14 | `WWW-Authenticate` header | Done | 4 |
| 15 | List endpoint with pagination | Done | 5 |
| 16 | MCP Streamable HTTP transport | Done | 6b |
| 17 | OpenAPI spec (`/openapi.json`) | Done | 6b |
| 18 | Input validation | Done | 5 |
| 19 | Response compression (gzip) | Done | 5 |
| 20 | Graceful shutdown | Done | 5 |
| 21 | Request timeouts | Done | 5 |
| 22 | Security headers | Done | 5 |
| 23 | `cargo audit` in CI | Done | 6 |
| 24 | Integration tests (Qdrant in CI) | Done | 6 |
| 25 | CD workflow (Docker build + push) | Done | 6 |
| 26 | HTTP handler tests | Deferred | 7 |
| 32 | `ENGRAM_QDRANT_URL` env override in REST | Done | 6 |
| 33 | Health readiness (503 when degraded) | Done | 6 |
| 34 | SIGPIPE fix in regression workflow | Done | 6 |
| 27 | Per-token user scoping | Missing | 7 |
| 28 | Per-user quotas | Missing | 7 |
| 29 | Admin API | Missing | 7 |
| 30 | Rate limiting | Missing | 7 |
| 31 | Request ID / correlation | Missing | 7 |
