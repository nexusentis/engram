---
title: "Restructuring Plan: Crate + API + MCP"
sidebar_position: 10
---

# Engram Restructuring Plan

**Status**: In progress — Phase 1 largely complete

## Problem Statement

Engram is 54.8K LOC across 102 files. Core modules (32K LOC) are well-abstracted with traits (`EmbeddingProvider`, `SearchChannel`, `Worker`, `Extractor`), but the benchmark module (20.4K LOC, 37% of codebase) is a monolith that leaks into everything. Three files are critical hotspots:

| File | LOC | Problem |
|------|-----|---------|
| `answerer.rs` | 6,944 | Agent loop + post-processing + gates + strategies — all in one file |
| `tools.rs` | 2,327 | Hardcoded tool schemas + Qdrant-specific execution |
| `ingester.rs` | 1,845 | Benchmark session loading mixed with core ingest pipeline |
| `benchmark_config.rs` | 875 | Model/gate config that should be shared |
| `integration_benchmark.rs` | 3,425 | Test harness with config coupling |

There's no way to use Engram as a library, REST API, or MCP server today.

---

## Implementation Progress

| Phase | Status | Notes |
|-------|--------|-------|
| 0: Golden tests | Not started | No golden traces, smoke set, or contract tests |
| 1.0: Move bench to engram-bench | **Done** | Answerer split into 7 submodules |
| 1.1: Create engram-core | **Done** | 20.8K LOC, clean module structure |
| 1.1 addendum: `MemorySystem` facade | **Done** | `MemorySystem` + `MemorySystemBuilder` in `engram-core::memory_system` |
| 1.1 addendum: Panic removal | **Done** | `HttpLlmClient::new()` and `RemoteEmbeddingProvider::new()` return `Result` |
| 1.1 addendum: Config validation | **Done** | `Config::load_strict()` + `Config::validate()` with semantic checks |
| 1.1 addendum: Re-exports | **Done** | Top-level re-exports for LLM, retrieval, storage, extraction types |
| 1.2: Create engram-agent | **Partial** | Agent loop + Tool trait extracted, but no ToolContext, no built-in memory tools |
| 1.3: Compatibility facade | **Done** | `pub use engram_core::*` + example at `crates/engram/examples/basic_usage.rs` |
| 1.4: engram-server | **Partial** | REST routes wired, MCP is stub only |
| 2: Split monoliths | **Partial** | Answerer split, tools split, but still benchmark-specific |
| 3: Code quality | **Partial** | ONNX reranker removed, LoCoMo data gitignored (never implemented) |
| 4: Documentation | **Partial** | This file updated; further docs pending |

### What the `MemorySystem` facade provides (vs. planned `Engram` struct)

The plan called for an `Engram` facade struct backed by `FactStore`/`MessageStore` traits. Since we don't have a second storage backend, trait abstraction is premature. Instead, `MemorySystem` works directly with concrete types:

```rust
pub struct MemorySystem {
    qdrant: Arc<QdrantStorage>,
    embedder: Arc<dyn EmbeddingProvider>,
    extractor: Arc<ApiExtractor>,
}
```

**Public API**: `ingest`, `ingest_turns`, `search`, `store_fact`, `store_memory`, `get_memory`, `delete_memory`, plus escape hatches (`storage()`, `embedder()`, `extractor()`).

**Builder** defaults: Qdrant at `localhost:6334`, 1536-dim vectors (matching `text-embedding-3-small`), `gpt-4o-mini` for extraction. Builder explicitly sets vector size on `QdrantConfig` to avoid dimension mismatch with the default (768).

**Usage**: `cargo run --example basic_usage -p engram` (requires Qdrant + `OPENAI_API_KEY`).

### Deferred to future work

- Storage traits (`FactStore`, `MessageStore`) — premature without second backend
- Built-in memory tools in `engram-agent` — needs tool extraction from bench
- Golden tests / behavioral freeze — separate effort
- MCP server wiring — stub works, full wiring is separate
- Feature flag matrix CI, monolith splitting

---

## Target Architecture

```
engram (workspace)
├── crates/
│   ├── engram/               # Compatibility facade (re-exports, deprecated over time)
│   ├── engram-ai-core/          # Pure library (types, storage, extraction, retrieval)
│   ├── engram-agent/         # Reusable agentic loop + tool system
│   ├── engram-server/        # axum REST API + MCP stdio server binary
│   └── engram-bench/         # Benchmark harness (depends on core + agent)
```

### Consumption Modes

1. **Crate**: `engram-ai-core` + optionally `engram-agent` as Cargo dependencies
2. **REST API**: Run `engram-server` binary
3. **MCP**: Run `engram-server --mcp` over stdio
4. **Benchmark**: `engram-bench` with `cargo test --test integration_benchmark`

### Key Design Decision: Compatibility Facade

**Recommendation (adopted)**: Keep a temporary `engram-ai` crate that re-exports from `engram-ai-core` and `engram-agent`. This prevents breaking `engram-cli` and existing tests during migration. Deprecate after migration completes.

---

## Phase 0: Behavior Freeze + Golden Tests

**Before any restructuring**, establish behavioral baselines.

### 0.1 Golden Trace Tests
Record agent traces for a fixed set of ~20 questions (deterministic seed) as golden files. Any refactor that changes these traces is a regression.

### 0.2 Benchmark Smoke Set
Create a 10-question deterministic subset that runs in <60 seconds with mocked LLM responses. Use as CI gate throughout migration.

### 0.3 Contract Tests
- `Engram` facade API: ingest → search round-trip
- MCP JSON-RPC: initialize → tool_call → response schema validation
- REST: request/response DTO serialization

**Exit criteria**: All golden tests pass, smoke set runs green in CI.

---

## Phase 1: Crate Boundaries (Move First, Refactor Later)

### 1.0 Move `bench/` intact to `engram-bench`

**Key insight**: Move bench first as a complete unit before any internal refactoring. This prevents mixing architectural changes with behavioral changes.

1. Create `crates/engram-bench/` with its own `Cargo.toml`
2. Move `src/bench/` directory wholesale
3. Move `tests/integration_benchmark.rs`
4. Have `engram-bench` depend on `engram-ai` (compatibility crate)
5. Verify: `cargo test --test integration_benchmark` still passes

### 1.1 Create `engram-ai-core` crate

Extract from current `crates/engram/src/`:
- `types/` — Memory, Session, Entity, enums
- `error.rs` — Unified error types (split out `BenchmarkError`)
- `storage/` — QdrantStorage, SurrealDB GraphStore, Database, SessionStore
- `embedding/` — EmbeddingProvider trait, Remote/Local/Cache impls
- `extraction/` — ExtractionPipeline, ApiExtractor, EntityRegistry, TemporalParser
- `retrieval/` — RetrievalEngine, SearchChannel trait, channels, QueryAnalyzer, ConfidenceScorer
- `temporal/` — Timeline, Parser, Resolver, TSM
- `conflict/` — Detector, Classifier, History, Supersession
- `workers/` — Worker trait, Consolidator, Reflector, DagBuilder, Scheduler
- `config/` — Core Config (server, embedding, retrieval settings)

**Public facade** (`engram-ai-core/src/memory_system.rs`) — **Implemented** as `MemorySystem` (not `Engram`, to avoid name collision with the crate):

```rust
pub struct MemorySystem {
    qdrant: Arc<QdrantStorage>,
    embedder: Arc<dyn EmbeddingProvider>,
    extractor: Arc<ApiExtractor>,
}

impl MemorySystem {
    pub fn builder() -> MemorySystemBuilder { ... }
    pub async fn ingest(&self, conversation: Conversation) -> Result<Vec<Uuid>>;
    pub async fn ingest_turns(&self, user_id: &str, turns: &[ConversationTurn]) -> Result<Vec<Uuid>>;
    pub async fn search(&self, user_id: &str, query: &str, limit: usize) -> Result<Vec<(Memory, f32)>>;
    pub async fn store_fact(&self, user_id: &str, content: &str) -> Result<Uuid>;
    pub async fn store_memory(&self, user_id: &str, memory: &Memory) -> Result<Uuid>;
    pub async fn get_memory(&self, user_id: &str, memory_id: Uuid) -> Result<Option<Memory>>;
    pub async fn delete_memory(&self, user_id: &str, memory_id: Uuid) -> Result<bool>;
    pub fn storage(&self) -> &Arc<QdrantStorage>;     // escape hatch
    pub fn embedder(&self) -> &Arc<dyn EmbeddingProvider>; // escape hatch
    pub fn extractor(&self) -> &Arc<ApiExtractor>;    // escape hatch
}
```

> **Note**: The planned `Engram` name and trait-based storage (`FactStore`/`MessageStore`) are deferred until a second storage backend exists. The concrete `MemorySystem` provides identical functionality with less indirection.

**Correction (adopted)**: Builder accepts traits, not concrete `QdrantStorage`. Use split storage traits:

```rust
pub trait FactStore: Send + Sync {
    async fn upsert(&self, memory: &Memory, embedding: Vec<f32>) -> Result<()>;
    async fn search(&self, query_embedding: Vec<f32>, filters: SearchFilters, limit: usize) -> Result<Vec<ScoredMemory>>;
    async fn get(&self, id: &str) -> Result<Option<Memory>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

pub trait MessageStore: Send + Sync {
    async fn upsert_batch(&self, messages: Vec<MessagePoint>) -> Result<()>;
    async fn search(&self, query_embedding: Vec<f32>, filters: SearchFilters, limit: usize) -> Result<Vec<ScoredMessage>>;
}

// Optional — behind `graph` feature flag
pub trait GraphStore: Send + Sync {
    async fn upsert_entity(&self, entity: &GraphEntity) -> Result<()>;
    async fn find_related(&self, entity_id: &str, hops: usize) -> Result<Vec<GraphEntity>>;
}
```

**Feature flags** (introduce early):
```toml
[features]
default = ["embedding-remote"]
embedding-remote = ["async-openai", "reqwest"]
embedding-local = ["ort", "ndarray", "tokenizers"]
graph = ["surrealdb"]
full = ["embedding-remote", "embedding-local", "graph"]
```

### 1.2 Create `engram-agent` crate

Extract the reusable agentic loop from `answerer.rs`:

```rust
/// Tool trait — with execution context
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult>;
}

pub struct ToolContext {
    pub user_id: String,
    pub budget_remaining: Option<f64>,
    pub cancellation: CancellationToken,
}

pub struct ToolResult {
    pub content: String,
    pub structured: Option<serde_json::Value>,  // Evidence/session IDs
    pub is_terminal: bool,  // Replaces DoneTool — control channel, not tool
}

pub struct ToolRegistry { ... }

/// LLM client — abstract over providers
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;
    async fn chat_with_tools(&self, request: ChatRequest, tools: &[ToolSchema]) -> Result<ChatResponse>;
    fn model_name(&self) -> &str;
    fn pricing(&self) -> TokenPricing;
}

/// Agent loop — domain-agnostic
pub struct Agent {
    config: AgentConfig,
    tools: ToolRegistry,
    llm: Arc<dyn LlmClient>,
}

impl Agent {
    pub async fn run(&self, user_message: &str, ctx: ToolContext) -> Result<AgentResult>;
}
```

**Corrections (adopted)**:
- `ToolContext` carries user scope, budget, and cancellation — not just args
- `ToolResult.is_terminal` replaces `DoneTool` as a control channel
- `ToolResult.structured` carries evidence/session IDs for downstream use
- `LlmClient` includes `pricing()` for cost tracking

**Built-in memory tools** (thin adapter over `Engram`):

```rust
pub struct SearchFactsTool { engram: Arc<Engram> }
pub struct SearchMessagesTool { engram: Arc<Engram> }
pub struct GetSessionContextTool { engram: Arc<Engram> }
pub struct DateDiffTool;  // Stateless utility

pub fn default_memory_tools(engram: Arc<Engram>) -> ToolRegistry { ... }
```

**What moves to engram-agent** (from answerer.rs):
- Generic agent loop (~800 LOC)
- Duplicate detection (comparing last N tool results)
- Cost tracking and circuit breaker
- Loop break detection
- Message history management
- Tool call parsing and dispatch

**What stays in engram-bench**:
- `QuestionStrategy` enum and routing
- Post-processing kernels (count/sum/date-diff reducers)
- Gate logic (preference, enumeration, update, abstention)
- Benchmark-specific system prompts

### 1.3 Compatibility facade

```rust
// crates/engram/src/lib.rs — re-exports only
pub use engram_core::*;
pub use engram_agent as agent;
```

`engram-cli` and tests import from `engram` — no breakage during migration.

### 1.4 Create `engram-server` crate

Binary crate:

```rust
#[tokio::main]
async fn main() {
    let args = Args::parse();
    let engram = Engram::builder()
        .qdrant_url(&args.qdrant_url)
        .embedding_model(&args.embedding_model)
        .build().await?;

    match args.mode {
        Mode::Rest => serve_rest(engram, args.port).await,
        Mode::Mcp  => serve_mcp(engram).await,
    }
}
```

**REST routes** (axum, backed by `Engram` facade):
```
POST   /v1/memories              → engram.ingest()
POST   /v1/memories/search       → engram.search()
GET    /v1/memories/:id          → engram.get()
DELETE /v1/memories/:id          → engram.delete()
POST   /v1/messages/search       → engram.search_messages()
POST   /v1/agent/ask             → Agent with default_memory_tools()
GET    /health                   → health check
GET    /metrics                  → prometheus
```

**MCP server**: Wire existing `api/mcp/handler.rs` to real `Engram` instance.

**Additions (adopted)**:
- Define auth + tenant scoping for all routes/tools up front
- Add `user_id` to MCP tool schemas (currently missing)
- Error contract, pagination, idempotency strategy
- API contract tests before wiring handlers

**Exit criteria**: All golden tests pass, smoke set green, `cargo test` for all crates passes.

---

## Phase 2: Break Up the Monoliths

*Only after Phase 1 is stable and benchmarks are parity.*

**Key insight (adopted)**: Extract by seam, not by estimated LOC. Start with pure helper seams (schemas, parsers, gates), then loop body. The intertwined gate states in answerer.rs (~lines 2100-2900) require careful extraction.

### 2.1 Split `answerer.rs` (6,944 → ~8 files)

| New File | Crate | LOC est. | Responsibility |
|----------|-------|----------|----------------|
| `agent.rs` | engram-agent | ~800 | Generic agent loop |
| `tools.rs` | engram-agent | ~200 | Tool trait + ToolRegistry |
| `llm_client.rs` | engram-agent | ~300 | LlmClient trait + OpenAI/Gemini impls |
| `cost.rs` | engram-agent | ~150 | Cost tracking, circuit breaker |
| `answer_generator.rs` | engram-bench | ~1,500 | Wraps Agent + benchmark gates |
| `strategies.rs` | engram-bench | ~800 | QuestionStrategy routing |
| `reducers.rs` | engram-bench | ~600 | Count/sum/date-diff post-processing |
| `gates.rs` | engram-bench | ~500 | Preference/enumeration/update/abstention |

### 2.2 Split `tools.rs` (2,327 → trait impls)

| New File | Crate | LOC est. | Responsibility |
|----------|-------|----------|----------------|
| `memory_tools/search_facts.rs` | engram-agent | ~300 | SearchFactsTool impl |
| `memory_tools/search_messages.rs` | engram-agent | ~250 | SearchMessagesTool impl |
| `memory_tools/session_context.rs` | engram-agent | ~200 | GetSessionContextTool impl |
| `memory_tools/date_diff.rs` | engram-agent | ~100 | DateDiffTool impl |
| `memory_tools/formatting.rs` | engram-agent | ~300 | Result formatting, date grouping, truncation |
| `tools/graph_tools.rs` | engram-bench | ~400 | Graph-specific tools (behind feature) |

### 2.3 Split `ingester.rs` (1,845 → 4 files)

| New File | Crate | LOC est. | Responsibility |
|----------|-------|----------|----------------|
| `ingest.rs` | engram-ai-core | ~400 | Core: Conversation → extract → embed → store |
| `ingest/batch.rs` | engram-ai-core | ~400 | Batch API polling, JSONL generation |
| `ingester.rs` | engram-bench | ~600 | BenchmarkSession deser, stats, progress |
| `ingester/graph.rs` | engram-bench | ~300 | SurrealDB graph population |

---

## Phase 3: Code Quality

### 3.1 LLM Client Unification

Currently `answerer.rs` and `api_extractor.rs` both make raw HTTP calls. Unify into `LlmClient` trait (defined in Phase 1.2):

- `OpenAiClient` — wraps async-openai
- `GeminiClient` — wraps Vertex AI REST + token refresh
- `MockClient` — for testing (critical for golden tests)

### 3.2 Configuration Consolidation

Merge two config systems into one:

```toml
# engram.toml
[server]
host = "0.0.0.0"
port = 8080

[storage]
qdrant_url = "http://localhost:6334"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"

[extraction]
model = "gpt-4o-mini"

[agent]
default_model = "google/gemini-3.1-pro-preview"
max_iterations = 25

[[models]]
name = "gpt-4o"
api_url = "https://api.openai.com/v1/chat/completions"
api_key_env = "OPENAI_API_KEY"

[[models]]
name = "google/gemini-3.1-pro-preview"
api_url = "https://us-central1-aiplatform.googleapis.com/..."
token_cmd_env = "GEMINI_TOKEN_CMD"

# Benchmark-only section
[benchmark]
answer_concurrency = 7
[benchmark.gates]
preference_threshold = 0.7
```

### 3.3 Dead Code Removal

| Dead Code | LOC | Action | Status |
|-----------|-----|--------|--------|
| ONNX reranker (`reranker.rs`) | 1,127 | Delete — libonnxruntime never installed | **Done** |
| LoCoMo benchmark (`bench/locomo/`) | ~1,000 | Delete or archive | **N/A** — was never implemented, data gitignored |
| Regression tracker (`bench/regression/`) | ~1,000 | Keep if used in CI, otherwise delete | Pending |

### 3.4 Feature Flag CI Matrix

**Recommendation (adopted)**: Run CI with `default`, `no-default-features`, and `all-features` to catch conditional compilation issues early.

---

## Migration Order

**Revised sequence (adopted)**:

| Step | What | Exit Gate |
|------|------|-----------|
| 0 | Golden tests + smoke set + contract tests | CI green |
| 1.0 | Move `bench/` intact to `engram-bench` | `cargo test --test integration_benchmark` passes |
| 1.1 | Extract `engram-ai-core` modules | `cargo build -p engram-core` compiles |
| 1.1b | Add feature flags + optional deps | Feature matrix CI green |
| 1.2a | Extract shared `LlmClient` abstraction | Both answerer and extractor use it |
| 1.2b | Extract agent loop + tool system to `engram-agent` | Golden tests pass |
| 1.3 | Compatibility facade in `engram` crate | `engram-cli` and all tests pass |
| 2 | Split monolith files internally | Golden tests pass, no file >1,000 LOC in core |
| 1.4 + 4 | Create `engram-server`, wire REST + MCP | Contract tests pass |
| 3 | Quality: config consolidation, dead code, storage traits | Feature matrix CI green |

**Key principle**: Each step has an explicit exit gate. No proceeding until gate passes.

---

## Effort Estimates

| Phase | Description | Estimate A | Estimate B | Adopted |
|-------|-------------|-------------|------------|---------|
| 0 | Golden tests + CI | 1-2 days | 2-3 days | 2-3 days |
| 1.0 | Move bench intact | 1 day | 1-2 days | 1-2 days |
| 1.1 | engram-ai-core + facade | 2-3 days | 3-5 days | 3-5 days |
| 1.1b | Feature flags | 1 day | 1-2 days | 1-2 days |
| 1.2 | engram-agent + LLM trait | 2-3 days | 4-6 days | 4-6 days |
| 1.3 | Compatibility facade | 0.5 day | 1 day | 1 day |
| 2 | Split monoliths | 2-3 days | 3-5 days | 3-5 days |
| 1.4 + 4 | engram-server (REST + MCP) | 2-3 days | 3-5 days | 3-5 days |
| 3 | Quality improvements | 2-3 days | 3-5 days | 3-5 days |
| **Total** | | **12-17 days** | **25-40 days** | **~20-35 days** |

**Note**: 12-17 days is achievable if you accept temporary regressions and reduced test coverage. 25-40 days for safe migration with parity checks at each step. The adopted estimate splits the difference — aggressive but with exit gates.

---

## Success Criteria

1. **Crate route**: `engram-ai-core = "0.1"` → `MemorySystem::builder().build().ingest().search()` in <5 lines (**Done** — see `crates/engram/examples/basic_usage.rs`)
2. **API route**: `engram-server` serves REST on a port
3. **MCP route**: `engram-server --mcp` works for Claude Desktop / Cursor
4. **Benchmark parity**: `cargo test --test integration_benchmark` produces identical results to pre-refactor
5. **No file >1,000 LOC** in `engram-ai-core` or `engram-agent`
6. **Feature flags**: `cargo build --no-default-features -p engram-core` compiles without ONNX/SurrealDB
7. **Golden tests**: All 20-question traces match pre-refactor baselines

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Benchmark regression during split | High | High | Golden tests + smoke set as CI gates |
| Config migration breaks env var flow | Medium | High | Test all TOML + env override combos |
| MCP `user_id` scoping gap | High | Medium | Fix schemas before wiring handlers |
| Feature flags break conditional compilation | Medium | Medium | Feature matrix CI from day 1 |
| Agent loop extraction breaks gate interplay | High | High | Extract seams first, loop body last |
| Effort underestimate | High | Medium | Exit gates prevent runaway phases |

---

## Consensus

**Agreed on**:
- 4-crate split is the right granularity (+ compatibility facade)
- Move bench first, refactor second
- Feature flags early, not late
- Golden tests as behavioral freeze
- `ToolContext` with user scope + budget + cancellation
- `is_terminal` control channel instead of `DoneTool`
- Split storage traits (`FactStore`, `MessageStore`, `GraphStore`)
- Dead code removal (ONNX reranker, LoCoMo)

**Disagreed on**:
- Effort: Initial estimate was 12-17 days, an independent review estimated 25-40 days. Adopted: ~20-35 days depending on test coverage tolerance.
- Sequencing: One approach had `engram-ai-core` first → bench last. The review recommended bench first → core second. Adopted the review's order (lower risk).
