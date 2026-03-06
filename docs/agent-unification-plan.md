# Agent Unification Plan

Unify the benchmark agent and production agent into a single implementation.
The benchmark becomes a thin evaluation harness on top of production code.

## Architecture

### Dependency Graph (unchanged direction)

```
engram-ai-core (lower tier — domain logic, types, storage)
        ↑
engram-agent (higher tier — Agent loop, traits, memory agent impls)
        ↑
engram-server (integration — HTTP, MCP, wires MemoryAgent)
engram-bench (evaluation — dataset loading, judge, scoring)
```

No cycles. Core never depends on agent. Server depends on both.

### Module Layout After Migration

```
engram-ai-core/src/
├── agent/                        ← NEW module
│   ├── mod.rs                    ← public exports
│   ├── tools.rs                  ← ToolExecutor: raw tool functions returning structured types
│   ├── tool_types.rs             ← FactResult, MessageResult, SessionContext, etc.
│   ├── tool_schemas.rs           ← JSON schemas for tool definitions
│   ├── date_parsing.rs           ← Date expression parser (generalized, no hardcoded year)
│   ├── strategy.rs               ← QuestionStrategy + detect_question_strategy()
│   └── prompting.rs              ← strategy-aware prompt templates
├── memory_system.rs              ← existing facade + message ingestion
├── storage/qdrant.rs             ← fix: user_id in messages, messages in init_collections()
└── (everything else unchanged)

engram-agent/src/
├── agent.rs                      ← Agent loop (UNCHANGED)
├── tool.rs                       ← Tool trait (UNCHANGED)
├── hook.rs                       ← AgentHook trait (UNCHANGED)
├── memory/                       ← NEW module (feature-gated: "memory")
│   ├── mod.rs
│   ├── tools.rs                  ← DynTool: wraps core ToolExecutor → Tool impls + string formatting
│   ├── gates.rs                  ← MemoryAgentHook: 18 gates implementing AgentHook
│   └── runner.rs                 ← MemoryAgent: strategy → prompt → loop → ensemble
└── (config, error, types unchanged)

engram-server/src/
├── routes/mcp_http.rs            ← wire memory_answer via MemoryAgent
├── routes/mcp_stdio.rs           ← wire memory_answer for stdio mode too
├── state.rs                      ← add MemoryAgent + config to AppState
└── (everything else unchanged)

engram-bench/src/longmemeval/
├── answerer/mod.rs               ← THINNED: wrapper around MemoryAgent
├── gates.rs                      ← THINNED: BenchmarkHook = MemoryAgentHook + oracle overrides
├── tools/                        ← REMOVED (re-exports or deleted)
├── tool_adapter.rs               ← REMOVED
├── harness.rs                    ← STAYS
├── judge.rs                      ← STAYS
├── ingester/                     ← STAYS (bulk dataset ingestion)
├── batch_runner.rs               ← STAYS
└── recall_harness.rs             ← STAYS
```

### Key Abstractions

**ToolExecutor** (in `engram-ai-core::agent::tools`) — raw domain logic:
```rust
pub struct ToolExecutor {
    storage: Arc<QdrantStorage>,
    embedding_provider: Arc<RemoteEmbeddingProvider>,
    reference_date: Option<DateTime<Utc>>,
    user_id: String,
}

impl ToolExecutor {
    // All return structured types, NOT formatted strings
    pub async fn search_facts(&self, query: &str, opts: SearchOpts) -> Result<Vec<FactResult>>;
    pub async fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<MessageResult>>;
    pub async fn grep_messages(&self, pattern: &str, limit: usize) -> Result<Vec<MessageResult>>;
    pub async fn get_session_context(&self, session_id: &str, include_facts: bool) -> Result<SessionContext>;
    pub async fn get_by_date_range(&self, start: &str, end: &str, limit: usize) -> Result<Vec<MessageResult>>;
    pub async fn search_entity(&self, entity: &str, limit: usize) -> Result<Vec<EntityResult>>;
    pub fn date_diff(&self, date1: &str, date2: &str, unit: &str) -> Result<DateDiffResult>;
}
```

**DynTool adapter** (in `engram-agent::memory::tools`) — formats structured results into strings:
```rust
pub struct DynTool {
    name: String,
    schema: Value,
    executor: Arc<ToolExecutor>,
}

impl Tool for DynTool {
    async fn execute(&self, args: Value) -> Result<String, AgentError> {
        // Dispatch to executor, format structured result into text
    }
}
```

**MemoryAgent** (in `engram-agent::memory::runner`):
```rust
pub struct MemoryAgent {
    config: MemoryAgentConfig,
    primary_client: Arc<dyn LlmClient>,
    fallback_client: Option<Arc<dyn LlmClient>>,
    storage: Arc<QdrantStorage>,
    embedding: Arc<dyn EmbeddingProvider>,
}

impl MemoryAgent {
    pub async fn answer(
        &self,
        user_id: &str,
        question: &str,
        reference_time: Option<DateTime<Utc>>,
    ) -> Result<MemoryAnswer>;
}

pub struct MemoryAnswer {
    pub answer: String,
    pub abstained: bool,
    pub sources: Vec<SourceReference>,
    pub cost_usd: f64,
    pub tool_trace: Vec<ToolTraceEntry>,
    pub strategy: QuestionStrategy,
    pub iterations: u32,
    pub fallback_used: bool,
}
```

**MemoryAgentHook** (in `engram-agent::memory::gates`):
```rust
pub struct MemoryAgentHook {
    strategy: QuestionStrategy,
    gates: GateConfig,
    question_text: String,
    tool_result_limit: usize,
    state: Mutex<OneShotFlags>,
}

impl AgentHook for MemoryAgentHook { ... }
```

### Gate Inventory (18 gates + 1 pre-tool guard)

All 19 checkpoints from the benchmark move to MemoryAgentHook. Classification:

| # | Gate | Type | Default |
|---|------|------|---------|
| — | Pre-tool: date_diff guard | Production | Always on |
| 1 | Temporal done-gate | Production | Always on |
| 2 | Preference done-gate | Production | Always on |
| 3 | Enumeration done-gate | Production | Always on |
| 4 | Enumeration qualifier | Configurable | Off |
| 5 | Update done-gate | Production | Always on |
| 6 | Update recency verification (A2) | Production | Always on |
| 7 | A2 latest-value check | Production | Always on |
| 8 | latest_date logging | Production | Always on (log only) |
| 9 | date_diff requirement | Production | Always on |
| 10 | date_diff consistency | Production | Always on |
| 11 | date_diff post-validator | Production | Always on |
| 12 | Enumeration recount | Production | Always on |
| 13 | Two-pass completeness | Production | Always on |
| 14 | Count/sum validator | Production | Always on |
| 15 | Abstention min-retrieval | Production | Always on |
| 16 | Anti-abstention keyword-overlap | Configurable | Off |
| 16b | Preference anti-abstention | Configurable | Off |
| 17 | Evidence-grounded comparison (A3) | Production | Always on |

- **Production (15):** Always on. These are quality features any memory agent benefits from.
- **Configurable (3):** Gates 4, 16, 16b. Off by default in production. On in benchmark
  profile. Controlled via `GateConfig` TOML. These push harder for answers (higher
  hallucination risk) but contributed to the 95.8% score.

**BenchmarkHook** (stays in `engram-bench`) wraps MemoryAgentHook and adds ONLY:
- `_abs` ID suffix override (oracle: uses question ID convention)
- `question.category == Abstention` fallback skip (oracle: uses dataset label)
- Gate 16 `_abs` skip logic (oracle: skips anti-abstention for known abstention questions)
- These are the ONLY oracle leaks. ~50 lines of wrapper code.

---

## Phased Execution

### Phase 0: Dependency + Config Setup

**Scope:** Prepare crate dependencies and config schema for migration.

**Changes:**

1. `engram-agent/Cargo.toml`:
   - Add `[features] memory = ["engram-core/default"]`
   - Currently depends on core with `default-features = false`; the `memory` feature
     enables full core (storage, embedding needed for tools)

2. `engram-server/Cargo.toml`:
   - Add `engram-agent = { path = "../engram-agent", version = "0.1.1", features = ["memory"] }`

3. `engram-ai-core/src/config/settings.rs`:
   - Add `[agent]` config section to the TOML schema: model, temperature,
     max_iterations, tool_result_limit, gates thresholds, ensemble settings
   - Same structure as current `[answerer]` + `[gates]` + `[ensemble]` in benchmark TOML

4. Version: All changes in this plan target version `0.2.0`. The `memory` feature is
   additive and doesn't break the 0.1.1 API, but the ToolExecutor and MemoryAgent are
   new public APIs warranting a minor bump. Publish all crates together.

**Validation:** `cargo check --workspace`

**Risk:** Low

---

### Phase 1: Message Ingestion in Production Path

**Scope:** Wire raw message storage into all production ingestion paths.

**Changes:**

1. `storage/qdrant.rs`:
   - Add `user_id: &str` parameter to `upsert_message()` and `upsert_messages_batch()`
   - Store `user_id` in message payload (currently missing — retrieval filters by it)
   - Merge `init_messages_collection()` into `init_collections()` so `engram init`
     creates all 5 collections (world, experience, opinion, observation, messages)
   - Update `get_collection_counts()` to include messages collection

2. `memory_system.rs`:
   - Enrich `ingest()` to also call `storage.upsert_messages_batch()` after fact extraction
   - Each message gets: user_id, session_id, role, content, embedding vector, turn_index
   - Preserve existing `ingest()` return type (add message count to response)

3. `engram-server/src/routes/ingest.rs`:
   - Refactor to call `MemorySystem::ingest()` instead of duplicating extraction + storage
   - Currently bypasses MemorySystem and calls `extractor.extract_with_context()` directly
   - Must preserve: `entities_found` in response (add to MemorySystem::ingest() return value)
   - Single code path for all ingestion

4. `engram-cli/src/main.rs`:
   - Update `init` command output to say "5 collections" instead of "4 collections"

**Validation:**
- Unit test: ingest via MemorySystem, verify messages in Qdrant with correct user_id
- Integration test: `POST /v1/memories` stores facts AND messages
- Existing tests still pass (fact storage unchanged)

**Risk:** Medium. Must not break existing fact ingestion. Server ingest refactor needs
careful API matching (`extract_with_context` returns entities that `extract` doesn't).

---

### Phase 2: Move Tool Logic to engram-ai-core (~1,800 lines)

**Scope:** Move ToolExecutor and all tool functions from bench to core.

**Files to move:**
| From (engram-bench) | To (engram-ai-core) | Lines | Notes |
|---|---|---|---|
| `tools/mod.rs` | `agent/tools.rs` | 140 | ToolExecutor struct + dispatch |
| `tools/schemas.rs` | `agent/tool_schemas.rs` | 347 | Tool JSON schema definitions |
| `tools/search.rs` | `agent/tools.rs` | 481 | search_facts, search_messages, grep_messages, search_entity |
| `tools/context.rs` | `agent/tools.rs` | 449 | get_session_context, get_by_date_range |
| `tools/date_parsing.rs` | `agent/date_parsing.rs` | ~100 | Date expression parser |
| `tools/types.rs` | `agent/tool_types.rs` | 53 | ToolExecutionResult, payload helpers |
| `tools/graph.rs` (date_diff only) | `agent/tools.rs` | ~80 | Extract `exec_date_diff()` method |

**Key changes:**
- Core tool functions return **structured types** (`Vec<FactResult>`, `SessionContext`, etc.)
  not formatted strings. String formatting is the adapter's job (Phase 4/5).
- `date_parsing.rs`: Replace hardcoded year anchor (`2023`) with configurable
  `reference_date.year()` for production safety.
- `search_entity`: Add structured return type (currently string-only).
- `ToolExecutionResult`: Generalize for production use (remove bench-specific fields if any).

**What stays in bench:**
- `tools/graph.rs` (minus date_diff) — graph tools are dead end, bench-only experiment
- `tools/tests.rs` — updated to import from core

**graph.rs ToolExecutor split:** Currently `graph.rs` has `impl ToolExecutor` for graph
methods + `exec_date_diff()`. Since ToolExecutor moves to core:
- `exec_date_diff()` moves to core with the ToolExecutor
- Graph methods stay in bench as standalone functions that take `&QdrantStorage` params
  (no longer methods on ToolExecutor)

**Contract tests:** For each tool:
- Snapshot test: fixed input → expected structured output
- Format test: structured output → formatted string matches old bench output exactly

**Validation:** `cargo test --workspace` passes. Benchmark dry-run on 10 questions
with bench tools reimplemented as thin wrappers over core — diff results.

**Risk:** Medium. Largest code move. Mitigated by contract tests on every tool.

---

### Phase 3: Move Strategy + Prompting to engram-ai-core (~450 lines)

**Scope:** Move QuestionStrategy detection and prompt templates.

**Files:**
| From | To | Lines |
|---|---|---|
| `answerer/strategy.rs` | `agent/strategy.rs` | 263 |
| `answerer/prompting.rs` | `agent/prompting.rs` | 188 |

**No logic changes.** Both files are self-contained:
- `strategy.rs`: Only uses `str` methods. Zero external deps.
- `prompting.rs`: Only references `QuestionStrategy` enum. Zero external deps.

**Validation:** Unit tests pass in new location. Snapshot test: all 500 benchmark
questions produce identical `QuestionStrategy` values.

**Risk:** Low

---

### Phase 4: Move Gates to engram-agent (4 sub-phases, ~1,150 lines)

**CRITICAL:** Gate interaction is the #1 historical source of regressions (see MEMORY.md
lessons 5, 20). Each sub-phase is independently validated.

**Architecture:**
- `MemoryAgentHook` in `engram-agent/src/memory/gates.rs` implements `AgentHook`
- All 18 gates + pre-tool guard move here
- `BenchmarkHook` in bench wraps `MemoryAgentHook` via delegation + oracle overrides
- Gate thresholds come from `GateConfig` (TOML-driven, same as current `[gates]` section)
- Configurable gates (4, 16, 16b) default to off; benchmark TOML sets them on

**Helper functions:** All gate helpers from bench `gates.rs` (lines 710-1150) move
with the gates: `extract_search_keywords`, `collect_tool_results`,
`collect_retrieval_results`, `extract_latest_dated_content`,
`extract_comparison_slots`, `is_interval_between_events`, `detect_temporal_unit`,
`extract_number_from_date_diff`, `extract_number_from_answer`,
`find_last_date_diff_result`, `extract_enumerated_items`, `deduplicate_items`,
`is_sum_question`, `extract_dollar_amounts`, `truncate_at_line_boundary_keep_end`.

#### Phase 4a: Temporal gates (~250 lines)
- Pre-tool guard: date_diff guard
- Gate 1: temporal done-gate
- Gate 9: date_diff requirement
- Gate 10: date_diff consistency
- Gate 11: date_diff post-validator

Interaction test: temporal question → search required → date_diff required → answer matches.

#### Phase 4b: Update gates (~200 lines)
- Gate 5: update done-gate
- Gate 6: update recency verification (A2)
- Gate 7: A2 latest-value check
- Gate 8: latest_date logging
- Post-tool: P12 truncation direction (keep newest for update questions)

Interaction test: update question → multiple dates → latest value used.

#### Phase 4c: Enumeration gates (~300 lines)
- Gate 3: enumeration done-gate
- Gate 4: enumeration qualifier (CONFIGURABLE)
- Gate 12: enumeration recount
- Gate 13: two-pass completeness
- Gate 14: count/sum validator

Interaction test: list question → thorough search → count matches items → math verified.

#### Phase 4d: Abstention + evidence + preference gates (~200 lines)
- Gate 2: preference done-gate
- Gate 15: abstention min-retrieval
- Gate 16: anti-abstention keyword-overlap (CONFIGURABLE)
- Gate 16b: preference anti-abstention (CONFIGURABLE)
- Gate 17: evidence-grounded comparison (A3)

Interaction test: comparison question → evidence for both sides required.

#### Phase 4 validation (after all sub-phases):
- Per-gate unit tests: each gate fires/doesn't fire on crafted inputs
- Gate-family interaction tests: gates within a family compose correctly
- Cross-family interaction test: temporal + enumeration gates don't double-fire
- Parity test with mock LlmClient: replay recorded tool sequences through both
  old BenchmarkHook and new MemoryAgentHook, verify identical gate decisions
- Fast Loop (60q) with real LLM: ±1q of baseline

**Risk:** High. Mitigated by sub-phasing, per-gate unit tests, and mock-based parity.

---

### Phase 5: MemoryAgent Runner + Ensemble (~800 lines)

**Scope:** Create `MemoryAgent` in `engram-agent/src/memory/runner.rs`.

**Extracts from bench `answerer/mod.rs` (2,244 lines):**
- `generate_answer_agentic()` → `MemoryAgent::answer()`
- `should_fallback()` + ensemble routing → `MemoryAgent` internal method
- Prefetch logic (strategy-aware initial retrieval)
- Agent construction (AgentConfig, tools, hook, LLM client)
- Prompt building (system prompt + strategy guidance + prefetch context)
- Reference-time plumbing (question date → ToolExecutor.reference_date)

**What stays in bench `answerer/mod.rs`:**
- `AnswerGenerator`: thin wrapper — construct MemoryAgent, call `agent.answer()`, map result
- Non-agentic path (legacy, `agentic=false`, for comparison testing)
- Judge integration, batch scoring
- Category-based calibration logging (uses dataset labels, bench-only telemetry)
- Oracle-dependent fallback skip (delegates to BenchmarkHook)

**Oracle removal in runner:**
- `should_fallback()` currently checks `question.category == Abstention` → replace
  with strategy-based heuristic (e.g., question text + low retrieval results)
- `question.id.ends_with("_abs")` → not used in runner, only in BenchmarkHook
- No `question.category` usage in prompt building (strategy detection is from text)

**Parity testing approach:**
- Create a `MockLlmClient` that replays recorded LLM responses from a fixture file
- Record 20 benchmark questions with the old path (capture all LLM requests + responses)
- Replay through new MemoryAgent path with MockLlmClient
- Diff: tool calls sequence, gate firings, final answer — must be identical
- This eliminates LLM non-determinism as a noise source

**Config:** `MemoryAgentConfig` in `engram-agent/src/memory/mod.rs`:
```rust
pub struct MemoryAgentConfig {
    pub model: String,
    pub temperature: f32,
    pub max_iterations: usize,
    pub tool_result_limit: usize,
    pub cost_limit: f32,
    pub gates: GateConfig,
    pub ensemble: Option<EnsembleConfig>,
    pub use_strategy: bool,
}

pub struct EnsembleConfig {
    pub enabled: bool,
    pub fallback_model: String,
    pub fallback_on_abstention: bool,
    pub fallback_on_loop_break: bool,
    pub fallback_on_enum_uncertainty: bool,
    pub enum_uncertainty_min_iterations: usize,
}
```

Loaded from TOML `[agent]` section (same keys as current `[answerer]` + `[ensemble]`).

**Validation:**
- Mock parity on 20 recorded questions: identical results
- Fast Loop (60q): ±1q of baseline
- Gate run (231q): ±2q

**Risk:** High. Core refactor. Mitigated by mock-based parity and staged validation.

---

### Phase 6: MCP + Server Integration

**Scope:** Expose MemoryAgent through both MCP transports (HTTP and stdio).

**Changes:**

1. `engram-ai-core/src/config/settings.rs`:
   - Add `agent: Option<AgentSettings>` to Config
   - `AgentSettings` maps to `MemoryAgentConfig` fields
   - Optional: server works without agent config (existing 4 tools still work)

2. `engram-server/src/state.rs`:
   - Add `memory_agent: Option<Arc<MemoryAgent>>` to `AppState`
   - Build from `config.agent` if present + LLM client from env vars
   - Graceful: if agent config absent or LLM key missing, `memory_agent = None`

3. `engram-server/src/routes/mcp_http.rs`:
   - Before forwarding to core McpHandler, intercept `tools/call` for `memory_answer`
   - If `memory_answer` and `state.memory_agent.is_some()`: call `agent.answer()`
   - Otherwise: forward to existing core handler as before
   - On `tools/list`: merge core's 4 tools + `memory_answer` schema

4. `engram-server/src/mcp.rs` (stdio mode):
   - Same interception pattern for stdio MCP transport
   - `memory_answer` available in both HTTP and stdio modes

5. Tool schema:
```json
{
  "name": "memory_answer",
  "description": "Answer a natural language question using the user's stored memories. Uses multi-strategy agentic search with quality validation.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "user_id": { "type": "string", "description": "User identifier" },
      "question": { "type": "string", "description": "Natural language question" },
      "reference_time": {
        "type": "string", "format": "date-time",
        "description": "Optional temporal anchor for relative date questions (e.g., 'how many days ago')"
      }
    },
    "required": ["user_id", "question"]
  }
}
```

**Validation:**
- `engram-server --mode mcp` → tools/list returns 5 tools
- `memory_answer` call returns structured answer
- Existing 4 tools unchanged
- Server without `[agent]` config: tools/list returns 4 tools (graceful)

**Risk:** Medium. MCP interception layer is new pattern. Must not break existing tools.

---

### Phase 7: Benchmark Cleanup + CI

**Scope:** Thin engram-bench, add production-faithful CI, remove dead code.

**Changes:**

1. **Bench answerer** → thin wrapper:
   - `AnswerGenerator::generate_answer_agentic()` → builds `MemoryAgent`, calls `.answer()`
   - Maps `MemoryAnswer` to bench `AnswerResult` type
   - BenchmarkHook wraps MemoryAgentHook (delegation) + oracle overrides (~50 lines)

2. **Remove from bench:**
   - `tools/` directory (except graph.rs if kept for experiments)
   - `tool_adapter.rs`
   - `answerer/strategy.rs`, `answerer/prompting.rs`
   - Duplicated gate logic in `gates.rs` (replaced by MemoryAgentHook delegation)
   - Dead config flags: `enable_llm_reranking`, `session_ndcg`, `enable_chain_of_note`,
     `enable_temporal_rrf`, `enable_entity_linked`, `enable_consolidation`,
     `enable_cross_encoder_rerank`, `enable_graph_retrieval`, `enable_causal_links`,
     `graph_augment.*`

3. **Graph disposition:** Remove `tools/graph.rs` entirely. Graph tools are documented
   dead end (MEMORY.md lessons 16, 17, 21). Graph augmentation config flags already
   disabled. Clean removal.

4. **CI production-faithful test:**
   - Run 60q fast loop through MemoryAgent directly (no BenchmarkHook, no oracle)
   - Configurable gates OFF (production defaults)
   - Assert score is within ±3q of full benchmark score
   - This proves: production code achieves the benchmark result

5. **Version bump:** Publish all crates as 0.2.0 with coordinated deps.
   - `engram-agent` 0.2.0: new `memory` feature
   - `engram-ai-core` 0.2.0: new `agent` module
   - Others: version-bumped deps

**Estimated reduction:** ~4,500 lines removed from engram-bench.

**Validation:** Full benchmark (500q) through MemoryAgent. Score: 479 ±2q.

**Risk:** Low (cleanup, no new logic).

---

## Risk Mitigation Summary

| Risk | Mitigation |
|------|-----------|
| Gate interaction regressions | Sub-phased by family, per-gate unit tests, mock-based parity |
| Tool output format drift | Contract tests: structured → string formatting matches old output |
| LLM non-determinism in parity tests | MockLlmClient with recorded fixtures, not live LLM |
| Ingestion behavior change | Server ingest refactored to use MemorySystem (one path) |
| crates.io publishing | Coordinated 0.2.0 release, feature-gated additions |
| MCP regression | Interception pattern preserves existing 4 tools, memory_answer is additive |
| Benchmark score regression | Per-phase validation: mock parity → FL (60q) → Gate (231q) → Truth (500q) |

## What This Achieves

1. **MCP clients get the full agent** — 8 tools, 18 quality gates, ensemble fallback,
   strategy-aware prompting. Same capabilities that achieved 95.8% on LongMemEval.

2. **Benchmark is credible** — thin harness over production code. Oracle leakage
   isolated to ~50 lines in BenchmarkHook. CI enforces production-faithful mode.

3. **One source of truth** — tool logic in core, gates/runner in agent. No duplication.

4. **Backward compatible** — MemorySystem facade unchanged. Existing 4 MCP tools unchanged.
   REST API unchanged. `memory_answer` is additive.

5. **Zero benchmark regression** — validated per-phase against 479/500 baseline.

## Decisions Made

1. **engram-agent gets memory code behind `memory` feature.** Generic Agent/Tool/AgentHook
   remain available without the feature. The crate name stays `engram-agent`.

2. **All 18 gates + pre-tool guard move to agent.** 15 always-on, 3 configurable (off
   by default). No gates are "bench-only" — they're just configured differently.

3. **BenchmarkHook is purely oracle logic (~50 lines).** Wraps MemoryAgentHook via
   delegation. Only adds question-ID and category-based overrides.

4. **Graph tools removed.** Dead end per lessons 16, 17, 21. Clean removal in Phase 7.

5. **Version 0.2.0** for the coordinated release with new APIs.
