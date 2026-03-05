---
title: "Deep Dive: Hindsight (91.4%)"
sidebar_position: 5
---

# Deep Dive: Hindsight (91.4%)

**Source**: [arxiv.org/abs/2512.12818](https://arxiv.org/abs/2512.12818)
**Code**: [github.com/vectorize-io/hindsight](https://github.com/vectorize-io/hindsight) (Python, MIT license)
**Benchmarks**: [github.com/vectorize-io/hindsight-benchmarks](https://github.com/vectorize-io/hindsight-benchmarks)

---

## Per-Category Scores (LongMemEval-S, 500 questions)

| Category | OSS-20B | OSS-120B | Gemini-3 |
|----------|---------|----------|----------|
| SS-User | 95.7% | 100% | 97.1% |
| SS-Assistant | 94.6% | 98.2% | 96.4% |
| SS-Preference | 66.7% | 86.7% | 80.0% |
| Knowledge Update | 84.6% | 92.3% | 94.9% |
| Temporal | 79.7% | 85.7% | 91.0% |
| Multi-Session | 79.7% | 81.2% | 87.2% |
| **Overall** | **83.6%** | **89.0%** | **91.4%** |

---

## Architecture Overview

Four logical memory networks stored in PostgreSQL (not a specialized graph DB):
1. **World facts**: About the user's life, people, events
2. **Experiences**: Interactions with the assistant
3. **Entity summaries**: Auto-consolidated knowledge
4. **Beliefs**: Evolving opinions/preferences

Three core operations: **Retain** (ingest), **Recall** (retrieve), **Reflect** (answer).

---

## Entity Graph Construction

### Entity Resolution (`entity_resolver.py`)

Entities extracted by LLM during fact extraction. Resolution uses 3 signals:
- **Name similarity** (0-0.5 weight): SequenceMatcher ratio
- **Co-occurring entities** (0-0.3 weight): overlap with entities previously co-occurring with candidate
- **Temporal proximity** (0-0.2 weight): within 7-day window
- Resolution threshold: 0.6
- Each entity: `canonical_name`, `mention_count`, `first_seen`, `last_seen`

### Link Types (stored in `memory_links` table)

1. **Entity links**: Two facts sharing the same entity
2. **Temporal links**: Facts close in time
3. **Semantic links**: Facts with high embedding similarity
4. **Causal links**: Explicitly extracted by LLM. Types: `causes`, `caused_by`, `enables`, `prevents`

---

## 4-Way Parallel Retrieval (TEMPR)

All four channels run in parallel for each fact type:

### Channel 1: Semantic Retrieval (vector similarity)
- Standard cosine similarity via pgvector
- Threshold: >= 0.3 similarity

### Channel 2: BM25 Retrieval (keyword/full-text)
- PostgreSQL `tsvector` with `ts_rank_cd` scoring
- Query tokens OR'd for flexible matching

### Channel 3: Graph Retrieval (3 implementations)

**MPFP (Meta-Path Forward Push)** -- the default:
- Runs predefined meta-path patterns from seeds
- Patterns from semantic seeds: `[semantic, semantic]`, `[entity, temporal]`, `[semantic, causes]`, `[semantic, caused_by]`, `[entity, semantic]`
- Patterns from temporal seeds: `[temporal, semantic]`, `[temporal, entity]`
- All patterns run hop-synchronized (one DB query per hop)
- Forward Push with alpha=0.15, threshold=1e-6
- Results fused via RRF

**BFS Spreading Activation** (original):
- Find entry points via semantic similarity (threshold 0.5)
- BFS traversal with 0.8 decay per hop
- Causal links boosted 1.5-2x

**Link Expansion** (simple/fast):
- Expand from seeds via entity links (filtered by frequency < 500) and causal links
- Fallback to semantic/temporal/entity links

### Channel 4: Temporal Retrieval
- Only activated when temporal constraint detected in query
- Uses `dateparser` + rule-based fallbacks for temporal extraction
- Finds entry points in date range, spreads through temporal/causal links
- Scores combine temporal proximity + semantic similarity

---

## Fusion

**Reciprocal Rank Fusion (RRF)** with k=60:
```
score(d) = sum_over_lists(1 / (k + rank(d)))
```

Optional cross-encoder reranking:
- Model: `cross-encoder/ms-marco-MiniLM-L-6-v2`
- Prepends date info: `[Date: June 5, 2022] fact_text`

---

## Fact Extraction (Retain)

### Pipeline (10 steps)
1. Extract facts from content via LLM
2. Generate embeddings (augmented with formatted dates)
3. Store original text chunks
4. Deduplication check
5. Insert facts into `memory_units`
6. Process entities (resolve + link)
7. Create temporal links
8. Create semantic links
9. Insert entity links
10. Create causal links

### Fact Structure (5 dimensions)
Each fact: what/when/where/who/why
Combined as: `"what | Involving: who | why"`

This is significantly richer than bare atomic facts. Each fact preserves temporal, spatial, and relational context within its structure.

### Fact Types
- `world`: User's life, other people, events
- `experience`: Interactions with assistant
- `opinion`: Preferences
- `observation`: Auto-consolidated knowledge

### Raw Text Storage
Hindsight stores BOTH raw text (chunks/documents) AND extracted facts. The `expand()` tool retrieves original context for any fact.

---

## Answering (Reflect Agent)

Tool-calling agentic loop with 5 tools, up to 10 iterations:

1. **`search_mental_models`** -- Search user-curated summaries (tried first)
2. **`search_observations`** -- Search auto-consolidated knowledge
3. **`recall`** -- Search raw facts via 4-way retrieval
4. **`expand`** -- Get chunk/document context for specific memory IDs
5. **`done`** -- Signal completion with answer

### Hierarchical Strategy
1. Try mental models first (highest quality)
2. Then observations (consolidated)
3. Then raw facts via recall
4. Expand for more context if needed

### Answering Prompt Key Rules
- "NEVER just echo the user's question -- decompose it into targeted searches"
- Budget levels: low/mid/high controlling exploration thoroughness
- Anti-hallucination: "MUST ONLY use information from retrieved tool results"
- Synthesis: "You SHOULD synthesize, infer, and reason from retrieved memories"

---

## Consolidation Engine

Handles knowledge updates by creating observations with temporal markers:
- "Used to X, now Y"
- "Changed from X to Y"
- Prompt: "CONTRADICTION: Opposite information about same topic -> update with temporal markers"

---

## Relevance to Engram

Hindsight was the system we studied most closely because it shares our database-backed retrieval architecture (as opposed to Mastra OM's no-retrieval approach). Several design elements directly influenced our work:

**What we adopted:**
- Hybrid search (vector + full-text), though Hindsight adds graph and temporal channels on top
- Agentic answering with iterative tool use
- Date-augmented embeddings and date-grouped context formatting

**What we attempted but found neutral or harmful:**
- Entity graph construction with SurrealDB (P20: behind-the-scenes graph prefetch --- implemented, but graph facts overlapped almost entirely with vector search results, producing a neutral outcome on the fast loop)
- Spreading activation traversal (built as infrastructure but did not move the score)
- Exposing graph tools to the agent (P18/P18.1: regressions from tool schema bloat alone, with zero graph tool calls made by the agent)

**What we did not attempt:**
- 5-dimensional fact structure (what/when/where/who/why) --- would require re-architecting extraction
- Causal link extraction during ingestion
- Cross-encoder reranking (our earlier experiments with LLM reranking were harmful at -14pp; did not test lightweight cross-encoders)
- Raw text chunk storage alongside extracted facts

The most important lesson from Hindsight was that **graph retrieval adds value only when it surfaces facts that vector search misses**. In our pipeline, with recall at 498/500, vector search already finds virtually everything the graph would surface. This is why our P20 graph prefetch infrastructure was technically correct but produced zero improvement --- there was no retrieval gap for the graph to fill.
