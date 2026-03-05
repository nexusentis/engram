---
title: "Pipeline Analysis: Where Information Is Lost"
sidebar_position: 1
---

# Pipeline Analysis: Where Information Is Lost

Early in the project, we conducted a systematic audit of each pipeline stage to understand where information was being lost and how that loss mapped to benchmark failures. This analysis --- originally performed against a 50-question validation split at 74% accuracy (Run #15) --- shaped every subsequent engineering decision. We revisit it here with the benefit of hindsight, noting which gaps we closed and which remain.

*Note: This analysis was conducted during Phase 2 (Week 1). By Phase 11, Engram reached 479/500 (95.8%) — #1 globally. The extraction-stage information loss described below remains the primary architectural gap — it is now the focus of the [Hybrid Observation Memory proposal](../lessons/single-model-architecture#2-hybrid-observation-memory-brief).*

## Stage 1: Extraction --- The Biggest Loss

**Input**: Full multi-turn conversation sessions (typically 10-30 turns each).
**Output**: 2-5 atomic facts per session.
**Estimated information retention**: 10-20%.

The extraction stage is where the pipeline loses the most information, by a wide margin. Our prompt instructs gpt-4o-mini to "extract 2-5 comprehensive narrative facts" from each session. This hard cap on output density means that a session containing 8-15 distinct pieces of information is compressed to at most 5 facts. The compression ratio is roughly 50-100x when measured against the raw token count of the conversation.

Several specific failure modes stem from this stage:

- **Detail truncation.** Questions like "What did the user say about X?" require the specific phrasing or detail that was present in the conversation but not captured in any extracted fact. The extractor summarizes; the benchmark demands specifics.
- **Count loss.** Questions like "How many X did the user mention?" fail when the extractor produces "User discussed several trips" instead of enumerating each trip individually. This is the root cause behind a significant fraction of our 13 aggregation failures at the final score.
- **Context erasure.** Facts like "User likes Italian food" lose the conversational context of *why* (a specific restaurant experience) and *when* (last Tuesday after a work event). The extractor strips the narrative frame.
- **Implicit reference loss.** While an entity-aware second pass resolves pronouns and implicit references, subtlety is still lost. A conversation where the user obliquely references a past event through a joke or callback rarely survives extraction.

### What the best systems do differently

The top three systems on the leaderboard handle this stage in fundamentally different ways:

- **Mastra OM (94.87%)** uses an "Observer" that compresses full conversations into prioritized observations at roughly 5-40x compression --- an order of magnitude less aggressive than our 50-100x. More importantly, the observations preserve contextual framing, not just atomic facts.
- **Honcho (92.6%)** stores raw messages alongside extracted observations, with `message_ids` linking back. This means the answering agent can always drill down from a summary to the original conversation turns.
- **Hindsight (91.4%)** extracts 5-dimension facts (what/when/where/who/why) and retains raw conversation chunks accessible via an `expand` tool. The answering agent can retrieve a fact and then request the surrounding context.

The common thread: every system above 90% retains access to raw conversation data in some form. We do not. Once extraction runs, the original turns are gone.

## Stage 2: Storage --- Relatively Lossless

**Input**: Extracted facts with embeddings.
**Output**: Qdrant points with vector + payload.

This stage is the least lossy in the pipeline. Each extracted fact receives:

- A vector embedding (text-embedding-3-small, 1536 dimensions)
- A fulltext index on the content field
- Structured payload: content, session_id, user_id, fact_type, epistemic_type, entity list, and temporal validity timestamps (`t_valid`)

For what it receives, storage is faithful. The problems are not about what Qdrant stores but about what it *cannot* store because we never gave it the data:

- **No raw messages.** Once facts are extracted, original conversation turns are discarded. There is no "expand" capability.
- **No entity graph.** Entities are stored as string arrays in the payload, not as nodes in a graph. There are no edges connecting "User" to "User's doctor" to "Doctor's recommendation." We later built SurrealDB graph infrastructure (P17-lite, P20), but the graph facts turned out to overlap heavily with what vector search already retrieved.
- **No inter-fact links.** Facts exist as independent points. There are no semantic, temporal, or causal links between them. A fact about the user changing their car and a fact about the user's previous car are not explicitly connected.
- **No session-level retrieval.** Session IDs are stored in the payload but were not used for session-level grouping or expansion in the retrieval stage. When one fact from a session matches a query, we do not automatically retrieve sibling facts from the same session.

## Stage 3: Retrieval --- Single-Pass vs. Multi-Strategy

**Input**: User question.
**Output**: Top-k facts (default k=20, later expanded to k=40 with RRF fusion).

At the time of this analysis, retrieval was a single-pass operation: one query, one hybrid search (vector + BM25 with RRF), one set of results. The problems were structural:

- **No query reformulation.** If the initial query missed relevant facts, there was no mechanism to try alternative phrasings.
- **No graph traversal.** We could not follow entity connections to discover indirectly related facts.
- **No temporal channel.** Despite having temporal validity timestamps, we did not filter by date range. We later confirmed that temporal filtering was fragile --- the code used numeric `Condition::range` against datetime payloads instead of proper `Condition::datetime_range`.
- **No session expansion.** Finding one relevant fact did not trigger retrieval of other facts from the same session.

### What the best systems do differently

The gap between single-pass and multi-strategy retrieval is the second largest differentiator on the leaderboard:

- **Hindsight** runs 4-way parallel retrieval: semantic search, BM25 keyword search, graph traversal (Meta-Path Fused Propagation), and temporal filtering. Results are merged via RRF.
- **Honcho** gives its agent 7 retrieval tools (`search_memory`, `search_messages`, `grep_messages`, `get_observation_context`, `get_messages_by_date_range`, `search_messages_temporal`, `get_reasoning_chain`) and allows up to 20 iterations of tool-calling.
- **Mastra OM** sidesteps retrieval entirely by keeping all observations in the context window.

We eventually moved to agentic retrieval with 7 tools and up to 20 iterations, which closed much of this gap. But the single-pass architecture at the time of this analysis was responsible for the majority of MultiSession and Extraction category failures.

## Stage 4: Answering --- Generic Prompt vs. Category-Aware

**Input**: Top-k facts + question + temporal guidance.
**Output**: Short answer string.

The original answering stage was a single LLM call with a generic ~30-line prompt. The same prompt handled all five question categories: Extraction, MultiSession, Temporal, Update, and Abstention. The prompt included basic temporal guidance ("Today's date is X"), date prefixes on facts when temporal validity was available, and a rule to "say 'I don't have enough information' if unsure."

The limitations were apparent:

- **No category-specific strategies.** Enumeration questions ("list all X") need different handling than temporal arithmetic questions ("how many days between X and Y") or update questions ("what is the user's current Y?"). A single prompt cannot optimize for all of these.
- **No iterative refinement.** If the first retrieval missed key facts, the answerer had no mechanism to request more.
- **No structured reasoning.** No chain-of-thought, no evidence tables, no explicit reasoning steps before committing to an answer.
- **No confidence calibration.** Abstention was binary --- either the prompt's "no relevant information" heuristic triggered, or it did not.

### What the best systems do differently

- **Honcho** uses a 237-line category-specific prompt system with different strategies per question type. Enumeration questions get deduplication tables. Temporal questions get date-range tools. Update questions get explicit search for "changed", "updated", "supersedes" keywords.
- **Hindsight** uses a hierarchical search strategy: mental models first, then observations, then raw facts, then expand. The agent builds understanding layer by layer.
- **Mastra OM** does not need category-specific retrieval because everything is already in context. The LLM reasons directly over all observations.

We eventually implemented category-aware strategy detection, quality gates per category, and the full agentic loop. These changes collectively accounted for the largest score gains in the project.

## The Three Critical Gaps

This analysis identified three structural gaps, ranked by estimated impact:

### 1. Raw Message Preservation (estimated +10-15pp)

We discard 80-90% of information at extraction. Every system scoring above 80% on the benchmark retains raw messages in some form. This gap is the single largest architectural difference between Engram and the top three systems.

We never closed this gap. Our final architecture still relies entirely on extracted atomic facts. The impact estimate turned out to be difficult to validate directly, because we found alternative ways to improve (agentic answering, quality gates) that partially compensated. But the ceiling imposed by extraction-only storage remains: when the answer to a benchmark question depends on a detail that was not extracted, no amount of retrieval or reasoning improvement can recover it.

### 2. Agentic Answering (estimated +8-12pp)

Single-pass retrieve-then-answer has a hard ceiling. This gap was the one we most successfully closed. The transition from single-pass to agentic answering --- combined with fixing the tool implementations that the agent depends on --- produced the single largest score gain in the project: +14 percentage points between Run #20 (non-agentic, 80%) and Run #21 (agentic, 84%) on the 50-question validation split, using identical data and the same 6 correctness fixes.

### 3. Rich Extraction (estimated +3-5pp)

Our 2-5 atomic facts per session are sparse compared to Hindsight's 5-dimension facts, Honcho's multi-level observations, or Mastra OM's contextual observations. Richer extraction could capture the "why" and "when" alongside the "what."

We partially addressed this through entity-aware extraction and temporal validity stamps, but never moved to a fundamentally richer fact model. Experiments with 5-dimensional extraction (Run #4) actually performed worse (68% vs 72% baseline), likely because the added complexity confused the extraction model without a corresponding improvement in retrieval.

## Per-Category Impact

The pipeline analysis also mapped each benchmark category to its primary failure stage. These estimates were made at the 50-question scale and evolved significantly as the project progressed, but the directional insights held:

| Category | Early Score | Primary Loss Stage | What Was Missing |
|----------|-------------|-------------------|-----------------|
| Extraction | 46.7% (7/15) | Stage 1 (Extraction) | Specific details not captured in atomic facts |
| MultiSession | 75.0% (9/12) | Stage 3 (Retrieval) | Cross-session entity traversal, multi-query search |
| Updates | 85.7% (6/7) | Stage 1+3 (Extraction + Retrieval) | "Changed from X to Y" observations, latest-value resolution |
| Temporal | 84.6% (11/13) | Stage 3+4 (Retrieval + Answering) | Date-range filtering, temporal arithmetic tools |
| Abstention | 100% (3/3) | None | Prompt heuristic worked well from the start |

By the final score of 88.4% (442/500), Extraction had improved to 93.3%, MultiSession to 85.1%, Updates to 88.9%, Temporal to 86.6%, and Abstention to 83.3%. The relative ordering of difficulty categories shifted --- Abstention went from our strongest to weakest category as we improved elsewhere --- but the fundamental insight held: extraction loss and retrieval strategy are the two stages that most constrain overall accuracy.
