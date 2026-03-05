# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Repository Overview

Engram is a project template/framework repository containing AI-first SDLC protocols for systematic software development.

## Project Structure

```
engram/
├── docs/
│   └── protocols/           # AI-first SDLC workflow protocols
│       ├── README.md
│       ├── epic-planning-protocol.md
│       ├── task-creation-protocol.md
│       ├── task-review-protocol.md
│       ├── task-execution-protocol.md
│       └── task-documentation-guide.md
└── epics/                   # Epic directories (created per project)
```

## Workflow Protocols

This repository uses an AI-first SDLC structure to organize large initiatives.

### Complete Workflow

```
1. Epic Planning Protocol
   Input: PRD (PDF/DOCX/MD)
   Output: epic.md, architecture.md, database-schema.md, feature.md files
   Tracker: planning_progress.md

   ↓

2. Task Creation Protocol
   Input: feature.md files
   Output: task-01-*.md, task-02-*.md, etc.
   Tracker: task_creation_progress.md

   ↓

3. Task Review Protocol
   Input: All task-*.md files
   Output: tasks_review.md (with fixes applied)

   ↓

4. Task Execution Protocol
   Input: Reviewed task-*.md files
   Output: Code, commits/PRs
   Tracker: tasks_execution.md

   ↓

5. DONE: Epic Complete
```

### Protocol Execution Rules

**CRITICAL**: When executing workflow protocols, follow these STRICT rules:

#### Rule 1: Create ONLY Specified Files

✅ **CREATE**: Files explicitly listed in the protocol's "Output" section
❌ **DO NOT CREATE**: Summary documents, helper scripts, extra documentation

#### Rule 2: Complete ALL Items Sequentially

✅ **DO**: Create ALL tasks for ALL features, update tracker after EACH item
❌ **DO NOT**: Create a subset and leave TODO lists, sample items and extrapolate

#### Rule 3: Handle Context Limits Correctly

1. First, use `/compact` to compress message history
2. If still approaching limit, use `/clean` to reset context
3. If limit reached AFTER both: Update tracker, tell user to resume, STOP

#### Rule 4: Update epics/epics_status.md

Update at end of each phase:
- Epic planning complete → Planning (Tasks Pending)
- Task creation complete → Planning (Review Pending)
- Task review complete → Ready for Implementation
- Task execution complete → Complete

#### Rule 5: No "Helpful" Deviations

Follow the protocol step-by-step literally. Create exactly what the protocol specifies, nothing more.

#### Rule 6: Check Existing Code Before Creating New

**CRITICAL**: Before implementing any new functionality:

1. **Search the codebase** for existing implementations that do similar things
2. **Enhance existing code** rather than creating parallel/duplicate systems
3. **Use existing modules** - check `mod.rs` files to see what's already exported
4. **Avoid duplication** - if similar functionality exists, extend it instead of recreating

❌ **BAD**: Creating `llm_entity_extractor.rs` when `context.rs` already has LLM entity extraction
✅ **GOOD**: Enhancing `context.rs` to use new typed structures from another task

### Enforcement

If Claude deviates: User says **"STOP. Re-read CLAUDE.md Workflow Protocol Execution Rules and start over."**

---

## Epic/Feature/Task Organization

### Directory Structure

```
epics/
├── epics_status.md                    # Master tracker for all epics
├── 001-epic-name/
│   ├── prd.md                         # Product requirements
│   ├── epic.md                        # Epic overview, metrics
│   ├── architecture.md                # Technical architecture
│   ├── database-schema.md             # Database design
│   ├── planning_progress.md           # Planning tracker
│   ├── task_creation_progress.md      # Task creation tracker
│   ├── tasks_review.md                # Review tracker
│   ├── tasks_execution.md             # Execution tracker
│   └── features/
│       ├── 001-feature-name/
│       │   ├── feature.md
│       │   └── tasks/
│       │       ├── task-01-*.md
│       │       └── ...
│       └── ...
└── ...
```

### Hierarchy

1. **Epic**: Large initiative (weeks to months)
2. **Feature**: Major component within an epic (days to weeks)
3. **Task**: Specific implementation work (hours to days)

### How to Use

**For AI (Claude)**:
```
User: "Work on Feature 001"
Claude: [Reads epic.md → feature.md → relevant tasks automatically]

User: "Start Task 03"
Claude: [Loads task + feature + epic context, begins implementation]

User: "Continue Epic 001"
Claude: [Finds next in-progress task, resumes work with full context]
```

**Always read "Context for AI" sections** in epic/feature/task documents first.

---

## TODO Policy

**CRITICAL**: Every TODO comment must reference a future task.

**Format**: `// TODO(Task XX-YY): Description`

**Bad (FORBIDDEN)**:
```
// TODO: implement this later
```

**Good**:
```
// TODO(Task 03-05): Replace with actual implementation
```

---

## Tracker Discipline Rules

**CRITICAL**: Execution trackers MUST be kept up-to-date at ALL times.

Update when:
1. Starting a task → Status: "In Progress"
2. Major milestone → Add progress note
3. Completing a task → Full update to Task Inventory + Execution Log
4. Context filling up → Update before using /compact or /clean

---

## Git Commit Guidelines

**Keep commit messages professional and concise.**

### What NOT to Include
- ❌ **NO AI tool attributions** (e.g., "Co-authored by Claude", "Generated by AI")
- ❌ **NO marketing language** for AI tools
- ❌ **NO unnecessary metadata** about how code was written

### Good Examples
```bash
git commit -m "Add user authentication endpoint"
git commit -m "Fix token expiration validation"
```

---

---

## LongMemEval Benchmark Configuration

All model/threshold configuration lives in TOML files under `config/`. **Zero model-specific code in Rust.**

### Config Files

| File | Purpose |
|------|---------|
| `config/benchmark.toml` | Default config with all models and settings |
| `config/ensemble.toml` | P22 Ensemble: Gemini primary + GPT-4o fallback |
| `config/gemini-primary.toml` | Gemini-only (current best: 452/500) |
| `config/gpt4o-primary.toml` | GPT-4o only (previous best: 442/500) |

Select config via `BENCHMARK_CONFIG` env var. Default: `config/benchmark.toml`.

### Auth

- **OpenAI models**: `OPENAI_API_KEY` env var (referenced by `api_key_env` in TOML)
- **Gemini (Vertex AI)**: OAuth token via `GEMINI_TOKEN_CMD` env var (referenced by `token_cmd_env` in TOML)
  - One-time setup: `gcloud auth activate-service-account vertex-account@solance-477915.iam.gserviceaccount.com --key-file=<path>`
  - Token generation: `GEMINI_TOKEN_CMD="gcloud auth print-access-token"` in `.env`
  - Auto-refresh: proactive at 45min, reactive on 401
- **Vertex AI model naming**: Must use `publisher/model` format, e.g., `google/gemini-3.1-pro-preview`
- All secrets stay in `.env` — TOML only references env var **names**, never values

### Running the Benchmark

```bash
# Load env vars (.env has OPENAI_API_KEY and GEMINI_TOKEN_CMD)
export $(cat .env | xargs)

# Full benchmark (500 questions) — Truth run (~$30 Gemini, ~$106 GPT-4o)
BENCHMARK_CONFIG=config/benchmark.toml INGESTION=skip FULL_BENCHMARK=1 \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Ensemble (Gemini + GPT-4o fallback)
BENCHMARK_CONFIG=config/ensemble.toml INGESTION=skip QUESTION_IDS=@data/longmemeval/fast_60.txt \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Gate run (231 questions)
BENCHMARK_CONFIG=config/benchmark.toml INGESTION=skip QUESTION_IDS=@data/longmemeval/gate_231.txt \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Fast loop (60 questions)
BENCHMARK_CONFIG=config/benchmark.toml INGESTION=skip QUESTION_IDS=@data/longmemeval/fast_60.txt \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Custom question set
BENCHMARK_CONFIG=config/ensemble.toml INGESTION=skip QUESTION_IDS=@data/longmemeval/p22_ensemble_test_24.txt \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture
```

### Ingestion Modes

Control ingestion behavior with the `INGESTION` env var:

| Mode | Env Var | Behavior |
|------|---------|----------|
| `full` | `INGESTION=full` (default) | Clear all Qdrant data, re-ingest everything |
| `skip` | `INGESTION=skip` | Skip ingestion, run pre-flight check to verify data exists |
| `incremental` | `INGESTION=incremental` | Keep existing data, only ingest sessions for users missing from Qdrant |

**Safety guards** (automatic):
- **Pre-flight check**: Before answering (on `skip`/`incremental`), samples user_ids and verifies they have data in Qdrant. Aborts with clear error if data is missing.
- **Ingestion manifest**: After ingestion, writes `data/longmemeval/.qdrant_manifest.json` with metadata. On `skip`/`incremental`, warns if current question set differs from what was ingested.

```bash
# Ingestion only (no answering)
export $(cat .env | xargs) && INGESTION_ONLY=1 FULL_BENCHMARK=1 cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Deterministic ingestion with extraction cache (P8)
export $(cat .env | xargs) && EXTRACTION_CACHE_DIR=data/longmemeval/.extraction_cache INGESTION_ONLY=1 FULL_BENCHMARK=1 cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture
```

### Env Var Overrides

Env vars override TOML values when set:

| Env Var | Overrides |
|---------|-----------|
| `ANSWER_MODEL` | `answerer.model` |
| `ANSWER_TEMP` | `answerer.temperature` |
| `MAX_ITERATIONS` | `answerer.max_iterations` |
| `ANSWER_CONCURRENCY` | `benchmark.answer_concurrency` |
| `TOOL_RESULT_LIMIT` | `answerer.tool_result_limit` |
| `INGESTION_MODEL` | `ingester.model` |
| `INGESTION_CONCURRENCY` | `ingester.concurrency` |

### 3-Tier Testing Strategy

**CRITICAL**: Follow this testing discipline for ALL benchmark changes.

| Tier | Set | Size | Cost (Gemini) | When to Run |
|------|-----|------|---------------|-------------|
| **Fast Loop** | 30 fails + 30 passes (stratified) | 60q | ~$4 | Every tweak |
| **Gate** | FixCheck-81 + RegCheck-150 | 231q | ~$15 | Before promoting a change |
| **Truth** | Full 500q | 500q | ~$30 | After accumulating multiple wins |

#### Set Files

| File | Contents | Source |
|------|----------|--------|
| `data/longmemeval/fixcheck_81.txt` | 81 failing question IDs from Run F1 | F1 failures |
| `data/longmemeval/regcheck_150.txt` | 150 passing question IDs, stratified | F1 passes |
| `data/longmemeval/fast_60.txt` | 30 fails + 30 passes (subset of above) | Stratified subset |
| `data/longmemeval/gate_231.txt` | Union of fixcheck_81 + regcheck_150 | Combined |

#### Cost Warning

**CRITICAL**: Before running any benchmark, estimate and report the expected API cost.

**MANDATORY**: ALWAYS get explicit user approval before running ANY benchmark, regardless of cost. Never run benchmarks autonomously — even a $4 Fast Loop adds up. When a plan has a fixed budget, every run must be approved against that budget. State the cost, the remaining budget, and ask "OK to proceed?"

---

**When in doubt, read the epic.md first.** It contains the "why", success metrics, and architectural principles that guide all implementation decisions.
