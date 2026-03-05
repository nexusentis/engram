---
title: "Deep Dive: Honcho (92.6%)"
sidebar_position: 4
---

# Deep Dive: Honcho (92.6%)

**Source**: [blog.plasticlabs.ai/research/Benchmarking-Honcho](https://blog.plasticlabs.ai/research/Benchmarking-Honcho)
**Code**: [github.com/plastic-labs/honcho](https://github.com/plastic-labs/honcho) (Python/FastAPI)
**Benchmarks**: [github.com/plastic-labs/honcho-benchmarks](https://github.com/plastic-labs/honcho-benchmarks)
**Live Results**: [evals.honcho.dev](https://evals.honcho.dev/)

---

## Per-Category Scores

| Category | Haiku 4.5 (90.4%) | Gemini 3 Pro (92.6%) |
|----------|-------------------|---------------------|
| SS-User | 94.3% | 98.6% |
| SS-Assistant | 96.4% | 98.2% |
| SS-Preference | 90.0% | 90.0% |
| Knowledge Update | 94.9% | 96.2% |
| Temporal | 88.7% | 91.7% |
| Multi-Session | 85.0% | 86.5% |

---

## Architecture: Dual Storage

### A. Raw Messages (stored AND embedded)

Every message stored in PostgreSQL with:
- Full text content
- `created_at` timestamps
- Session name, peer name
- `seq_in_session` ordering
- Token count
- Separate `MessageEmbedding` table for vector search

Enables: `search_messages()` -- semantic search directly over raw conversation messages.

### B. Extracted Observations (facts)

Extracted by "deriver" module using `gemini-2.5-flash-lite`:
- Explicit atomic facts with absolute dates/times
- Each observation embedded and stored in `Document` table
- Carries `message_ids` linking back to source messages
- 4 levels: `explicit`, `deductive`, `inductive`, `contradiction`
- Deduplication at creation time
- Batch config: `REPRESENTATION_BATCH_MAX_TOKENS: 16384`

Extraction prompt key rules:
- "Transform statements into one or multiple conclusions"
- "Each conclusion must be self-contained with enough context"
- "Use absolute dates/times when possible (e.g. 'June 26, 2025' not 'yesterday')"

---

## Retrieval: Agentic Tool-Calling Loop

This is the biggest architectural difference between Honcho and a simple RAG system like Engram's initial design. Rather than a single query-then-answer pass, Honcho gives its answering agent a suite of tools and lets it iteratively gather evidence.

### 7 Tools Available to the Answering Agent

1. **`search_memory`** -- Semantic search over extracted observations (pgvector)
2. **`search_messages`** -- Semantic search over raw conversation messages (pgvector)
3. **`grep_messages`** -- Case-insensitive text search (ILIKE on PostgreSQL)
4. **`get_observation_context`** -- Given observation message_ids, retrieve surrounding conversation
5. **`get_messages_by_date_range`** -- Time-bounded message retrieval
6. **`search_messages_temporal`** -- Semantic search + date filtering
7. **`get_reasoning_chain`** -- Traverse reasoning tree (premises/conclusions)

### Prefetching

Before tool calls, the system automatically runs two semantic searches:
- Top 25 explicit observations
- Top 25 deductive/inductive/contradiction observations

Results are included in the initial prompt. The agent then uses tools to drill deeper.

Key design choice: Searches split by observation level to prevent "retrieval dilution" --- mixing high-confidence explicit facts with speculative deductions would push relevant results down the ranking.

### Agent Configuration (Benchmark)

- Max output tokens: 8192
- Max input tokens: 100,000
- Thinking budget: 4096 tokens
- **Max tool iterations: 20**

---

## Prompt Engineering

The system prompt is massive and highly structured (~237 lines). Key elements:

### Workflow

1. Analyze the query
2. Check for user preferences FIRST
3. Strategic information gathering (search memory, then messages)
4. Special handling for ENUMERATION/AGGREGATION (multi-grep + semantic)
5. Special handling for SUMMARIZATION
6. Ground answers using reasoning chains
7. Synthesize response

### Category-Specific Strategies

- **Knowledge Updates**: "Search for deductive observations containing 'updated', 'changed', 'supersedes'" + "Always search for update language"
- **Enumeration**: Mandated: "START WITH GREP... THEN USE SEMANTIC SEARCH... do at least 3 search_memory or search_messages calls with different phrasings"
- **Enumeration dedup**: "Create deduplication table, compare items, mark duplicates"
- **Temporal**: Uses `search_messages_temporal` and `get_messages_by_date_range`
- **Anti-hallucination**: "Did I find this EXACT information in my search results, or am I inferring/inventing it?"

---

## Temporal Handling

1. Absolute timestamps on everything (enforced in extraction prompt)
2. Messages carry `created_at`, formatted via `format_new_turn_with_timestamp()`
3. Dedicated temporal tools: `get_messages_by_date_range`, `search_messages_temporal`
4. Knowledge update detection in deriver: finds same fact with different values, creates update observation, DELETES outdated one

---

## "Dreaming" System (Background Processing)

Runs asynchronously via `/src/dreamer/`:
1. Surprisal sampling (finds novel observations)
2. Deduction specialist (creates deductive observations, handles knowledge updates, finds contradictions, deletes outdated)
3. Induction specialist (behavioral patterns, preferences across observations)

**Important note**: Dreaming was DISABLED for the LongMemEval-S benchmark. The 92.6% score is achieved without it --- the agentic retrieval loop and dual storage alone account for the result.

---

## Model Configuration

| Component | Model |
|-----------|-------|
| Ingestion/Deriver | gemini-2.5-flash-lite |
| Answering (90.4%) | claude-haiku-4.5 |
| Answering (92.6%) | gemini-3-pro-preview |
| Embeddings | text-embedding-3-small |

---

## Relevance to Engram

Honcho's architecture influenced our thinking in several ways:

**What we adopted (in spirit):**
- Agentic tool-calling answerer with iterative search (our agent loop with `search_facts`, `search_by_date`, `recount`, and `done`)
- Category-aware prompting strategies (enumeration handling, anti-abstention gates)
- Timestamp enforcement during extraction

**What we could not adopt:**
- Raw message storage and retrieval (would require fundamental re-architecture of our Qdrant-based pipeline)
- Grep/text search over raw messages (we only have extracted facts in the index)
- Observation level splitting (we extract facts at a single level)
- Reasoning chain traversal (requires a deductive observation graph we do not build)

The key takeaway from Honcho was that the **agentic answering loop is the single largest architectural lever** --- estimated at +10-12pp over single-pass answering. This motivated our transition from single-pass retrieve-then-answer to a multi-iteration tool-calling agent, which proved to be one of our most impactful changes.
