---
title: "Phase 8: Productionization"
sidebar_position: 9
description: "Monolith to multi-crate workspace, REST/MCP server, 12,000+ LOC of dead code removed (Weeks 3-4)"
---

# Phase 8: Productionization (Weeks 3-4)

**Period**: Weeks 3-4, Days 19-22
**Score**: 467/500 (unchanged — no benchmark changes)
**Focus**: Architecture, not accuracy

## Context

After Phase 7's ensemble router landed at 467/500, the codebase was a single `engram` crate with monolithic source files: `answerer.rs` (5,183 LOC), `tools.rs` (2,294 LOC), `ingester.rs` (1,845 LOC). The benchmark worked, but the code was a tangled mass of benchmark-specific logic, dead prototyping stubs, and tightly coupled modules that could only be consumed as a whole. Making Engram usable beyond benchmarks — as a server, a library, an agent framework — required restructuring.

Phase 8 is the pivot from "benchmark research tool" to "shippable system." No score changes, no experiments. Just surgery. This matters because a benchmark-only system has no practical value — productionization ensures the engineering investment translates into a usable library, server, and agent framework.

## The Monolith Problem

The `engram` crate had accumulated layers of dead code from phases 1-7:

- **ONNX reranker** (1,127 LOC) — abandoned after LLM reranking proved harmful in Phase 2
- **ONNX embedding stubs** (619 LOC) — local embedding experiment that never shipped
- **Temporal solver** (3,534 LOC) — parser, resolver, timeline, recency, TSM scoring from the Phase 1 attempt that was net harmful
- **Background workers** (3,421 LOC) — consolidator, reflector, scheduler, DAG builder for a worker framework never used
- **Conflict resolution** (1,391 LOC) — history tracking and supersession logic never integrated
- **Extraction pipeline** (1,664 LOC) — coverage analysis and validation from early prototyping

Total dead code: ~12,000+ LOC. Almost as much dead code as live code.

Beyond dead code, the architecture had a layering problem. The agentic loop, the tool implementations, the benchmark gates, the LLM client, the storage layer, and the API types all lived in the same crate with no separation. You couldn't use the agent loop without pulling in the benchmark harness. You couldn't embed the storage layer without getting the entire extraction pipeline.

## The Restructuring (Day 19)

### Step 1: Multi-Crate Workspace

The first commit began the split, with subsequent commits completing it into six crates:

```
engram-ai-core   (21K LOC)  — Foundation: types, storage, extraction, embedding, LLM client
engram-agent       (735 LOC)  — Generic agent loop with Tool/AgentHook traits
engram-bench     (17K LOC)  — Benchmark-specific: answerer, judge, gates, tools, ingester
engram-server      (820 LOC)  — REST and MCP server binary
engram-cli         (285 LOC)  — CLI binary
engram-ai            (6 LOC)  — Re-export facade
```

The dependency graph flows one direction:

```
engram-ai-core
  ├── engram-ai (facade, re-exports core)
  │     └── engram-bench (+ engram-agent)
  ├── engram-agent
  ├── engram-server
  └── engram-cli
```

No circular dependencies. `engram-ai-core` is the foundation; everything else specializes. `engram-bench` depends on the `engram-ai` facade (for feature-flag forwarding of `graph` and `metrics`) plus `engram-agent` directly.

The key extraction was `HttpLlmClient`. Previously embedded inside `answerer.rs`, it was lifted to `engram-ai-core/src/llm/` as a trait-based interface (`LlmClient`) with concrete implementation. This let both the benchmark answerer and the server share the same HTTP client without depending on each other.

### Step 2: Shared LlmClient (Day 19)

Three wiring commits connected the existing extraction and embedding code to the newly shared `HttpLlmClient`:

- `context.rs` and `api_extractor.rs` — switched from inline HTTP calls to `HttpLlmClient`
- `embedding/remote.rs` — same treatment for the embedding provider
- An independent code review caught 4 issues: UTF-8 truncation safety, empty response handling, granular error mapping, missing hour unit in temporal parsing

### Step 3: Reusable Agent Loop (Day 19)

The agentic loop — iterate over tool calls until done, with cost limits, duplicate detection, and lifecycle hooks — was deeply entangled with the benchmark answerer. A dedicated commit extracted it into `engram-agent`:

- **`Agent` struct** — generic over `Tool` and `AgentHook` traits
- **`Tool` trait** — name, schema, execute. Any tool implementation, not just benchmark tools.
- **`AgentHook` trait** — `pre_tool_execute()`, `post_tool_execute()`, `validate_done()`. Lets callers inject behavior (gates, logging, cost limits) without modifying the loop.
- **`done()` handling** — special handling for the agent's "I'm done" signal, with the done schema visible to the agent but validation delegated to the hook's `validate_done()` method

The 17 benchmark gates migrated from inline `if` chains in the answerer to a `BenchmarkHook` implementation of `AgentHook`. Same behavior, clean separation.

### Step 4: Module Splits (Day 20)

The three largest files were split into directory modules:

**`tools.rs` (2,294 LOC) → `tools/` (8 files)**:
`mod.rs`, `types.rs`, `schemas.rs`, `search.rs`, `context.rs`, `graph.rs`, `date_parsing.rs`, `tests.rs`

**`answerer.rs` (5,183 LOC) → `answerer/` (7 files)**:
`mod.rs`, `types.rs`, `config.rs`, `strategy.rs`, `prompting.rs`, `reduction.rs`, `tests.rs`

**`ingester.rs` (1,845 LOC) → `ingester/` (4 files)**:
`mod.rs`, `config.rs`, `stats.rs`, `batch.rs`

Pure code moves with `pub(super)` for internal APIs. No logic changes, all tests continued to pass.

### Step 5: Dead Code Removal (Day 20)

Four cleanup commits removed 12,000+ LOC of dead code:

| What | LOC Removed |
|------|-------------|
| ONNX reranker | -611 |
| Temporal solver, workers, conflict resolution, extraction stubs | -11,550 |
| ONNX embedding stubs (local.rs, cache.rs, provider pruning) | -614 |
| Config loader/validator, dead health module, layering fixes | -1,174 |

The layering fix was important: `BenchmarkError` had been defined in `engram-ai-core` even though it was only used by `engram-bench`. Moving it to the right crate enforced the rule that the core library knows nothing about benchmarks.

## The Server (Day 20)

With the crate structure clean, adding a REST server was straightforward.

### Architecture

```
engram-server
  ├── main.rs          — Mode selection (REST / MCP), state initialization
  ├── state.rs         — AppState: Arc-wrapped Qdrant, embedder, extractor, SQLite
  ├── error.rs         — AppError → HTTP status code mapping
  ├── middleware/auth.rs — Bearer token middleware (SHA256-hashed tokens)
  └── routes/
      ├── health.rs    — GET /health (unauthenticated)
      ├── ingest.rs    — POST /v1/memories
      ├── search.rs    — POST /v1/memories/search
      ├── get_memory.rs — GET /v1/memories/{id}
      ├── delete.rs    — DELETE /v1/memories/{id}
      └── messages.rs  — POST /v1/messages/search
```

All routes are user-scoped (queries include `user_id`). Authentication via `ENGRAM_API_TOKENS` env var with SHA256 hashing. Config lives in `config/engram.toml`.

### SearchFilters Wiring

The final commit of this phase wired `SearchFilters` to Qdrant. Previously, `POST /v1/memories/search` accepted filter parameters (`fact_types`, `entity_ids`, `time_range`, `min_confidence`) but silently dropped all except `min_confidence`. Now:

- `fact_types` → Qdrant `should` filter (OR over types) on `fact_type` field
- `entity_ids` → Qdrant `should` filter (OR over entities) on `entity_ids` field
- `time_range` → `DatetimeRange` on `t_valid` (start inclusive, end exclusive)
- `min_confidence` → Rust-side post-filter (not worth a Qdrant index)

The handler builds a full `Filter` and calls `search_memories_with_filter()` instead of the basic `search_memories()`.

## What Changed, By the Numbers

| Metric | Before (Phase 7) | After (Phase 8) |
|--------|-------------------|------------------|
| Crates | 1 | 6 |
| Dead code | ~12,000 LOC | Removed (some `dead_code` warnings remain in unused retrieval modules) |
| Largest file | 5,183 LOC (answerer.rs) | ~2,210 LOC (answerer/mod.rs) |
| Server | None | REST + MCP, 6 endpoints |
| Agent loop | Embedded in answerer | Reusable crate with traits |
| LLM client | Answerer-internal | Shared across crates |
| Feature flags | None | `graph`, `metrics` (optional) |
| Tests | All passing | All passing (652 unit + doc tests, 31 ignored integration) |
| Score | 467/500 | 467/500 |

## Key Insight

**Refactoring is not a detour from shipping — it's a prerequisite.** The monolithic crate couldn't become a product. The dead code wasn't just ugly; it created false coupling that made every change harder and every new feature require understanding 30,000+ LOC of context. The restructuring took two days and changed zero behavior, but it turned a benchmark harness into a system with a server, a library, and a reusable agent framework. The remaining benchmark work (P25, P23, P26) will be easier too — the module boundaries make it possible to change the answerer strategy without touching the agent loop, or add a new tool without rebuilding the ingester.

## Production-Ready Library

A follow-up session addressed the remaining gaps needed to use Engram as a `cargo add` dependency:

1. **`MemorySystem` facade** (`engram-ai-core/src/memory_system.rs`) — high-level API: `ingest`, `search`, `store_fact`, `store_memory`, `get_memory`, `delete_memory`. Plus `MemorySystemBuilder` with sensible defaults (Qdrant localhost, 1536-dim vectors, `text-embedding-3-small`, `gpt-4o-mini`).

2. **Panic removal** — `HttpLlmClient::new()` and `RemoteEmbeddingProvider::new()` changed from panicking to returning `Result`. All callers updated.

3. **Config validation** — `Config::load_strict()` and `Config::validate()` catch semantic errors (zero vector size, missing URLs, invalid thresholds, NaN) that deserialization alone can't enforce.

4. **Re-exports** — Top-level re-exports added for LLM types, retrieval types, storage types, and extraction extras.

5. **Example** — `crates/engram/examples/basic_usage.rs` demonstrates the full workflow.

See `research/appendices/restructuring-plan.md` for the updated status table.

## Commit Log

| Date | Description |
|------|-------------|
| Day 19 | Crate restructuring: multi-crate layout, feature flags, shared LlmClient |
| Day 19 | Wire context.rs and api_extractor.rs to shared HttpLlmClient |
| Day 19 | Wire embedding/remote.rs to shared HttpLlmClient |
| Day 19 | Fix review findings: granular error mapping, add hour unit |
| Day 19 | Extract reusable agent loop into engram-agent crate |
| Day 20 | Split answerer, tools, ingester monoliths into directory modules |
| Day 20 | Add engram-server binary crate with REST and MCP endpoints |
| Day 20 | Remove dead ONNX reranker code (-611 LOC) |
| Day 20 | Add config/engram.toml for engram-server |
| Day 20 | Remove dead temporal, workers, conflict modules and extraction stubs (-11,550 LOC) |
| Day 20 | Remove dead ONNX embedding stubs (-614 LOC) |
| Day 20 | Clean up dead code, fix layering, tighten public API (-1,174 LOC) |
| Day 20 | Wire SearchFilters to Qdrant in search endpoint |
