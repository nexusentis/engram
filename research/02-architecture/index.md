---
title: "Architecture"
sidebar_position: 2
description: "Engram's pipeline: ingestion, storage, retrieval, and answering"
---

# Architecture

Engram is a four-stage pipeline that transforms raw conversation sessions into queryable long-term memory. Every design decision was driven by a single question: where does information get lost, and what is the most cost-effective way to recover it?

## Pipeline Overview

```
Raw Sessions (~24K)
       |
       v
 +-----------------+
 |   Extraction     |  gpt-4o-mini, temp=0, seed=42
 |   (Stage 1)      |  2-5 atomic facts per session
 +-----------------+
       |
       v
 +-----------------+
 |   Storage        |  Qdrant: vector (text-embedding-3-small)
 |   (Stage 2)      |  + fulltext index + structured payload
 +-----------------+
       |
       v
 +-----------------+
 |   Retrieval      |  Hybrid search: vector + BM25 + RRF
 |   (Stage 3)      |  Agentic loop: 7 tools, max 20 iterations
 +-----------------+
       |
       v
 +-----------------+
 |   Answering      |  Gemini 3.1 Pro + GPT-5.2 ensemble
 |   (Stage 4)      |  Category-aware strategy + quality gates
 +-----------------+
       |
       v
    Answer
```

## Stage Summaries

### Stage 1: Extraction

Each of the ~24,000 conversation sessions is processed by gpt-4o-mini, which extracts 2-5 atomic facts per session. The extraction prompt requests "comprehensive narrative facts" capturing who, what, when, and where. An entity-aware second pass resolves implicit references (e.g., "he" to "the user's doctor"). The output is a set of structured facts with metadata: content text, entity list, epistemic type, temporal validity, and session provenance.

This stage is where the most information is lost. A 20-turn conversation might contain 10-15 distinct pieces of information; we retain 2-5. The rationale and tradeoffs are discussed in [Pipeline Analysis](./pipeline-analysis).

### Stage 2: Storage

Extracted facts are stored in Qdrant with dual indexing: a vector embedding (text-embedding-3-small, 1536 dimensions) for semantic search, and a fulltext index on the content field for keyword matching. Each point carries a structured payload including session ID, user ID, entity list, fact type, epistemic type, and temporal validity timestamps.

Storage is the least lossy stage. Everything the extractor produces is faithfully indexed. The main limitation is what is *not* stored: we discard raw conversation turns after extraction, losing the ability to "expand" back to original context that systems like Honcho and Hindsight rely on.

### Stage 3: Retrieval

The retrieval stage evolved from single-pass hybrid search (vector + BM25 with RRF fusion) to a full agentic loop. The agent has access to 7 tools --- `search_facts`, `search_messages`, `search_entity`, `get_temporal_context`, `date_diff`, `done`, and `give_up` --- and can iteratively refine its search strategy over up to 20 iterations.

This was the single largest score improvement in the project's history. Switching from single-pass to agentic retrieval with correct tool implementations produced a +14 percentage point gain (Run #20 vs #21). The details of how we arrived at this design are in [Key Design Decisions](./key-decisions).

### Stage 4: Answering

The answering stage is integrated with retrieval in the agentic loop. The agent reasons over retrieved facts, decides whether to search again or produce a final answer, and is guided by category-specific strategy detection (Temporal, Update, MultiSession/Enumeration, Extraction, Abstention). Quality gates enforce minimum evidence thresholds before accepting an answer --- for example, the recount gate for enumeration questions and the date_diff gate for temporal arithmetic.

The answering model evolved through the project: originally gpt-4o (442/500), then Gemini 3.1 Pro (452/500), then a Gemini+GPT-4o ensemble (472/500), and finally a Gemini+GPT-5.2 ensemble (479/500 — #1 globally). The ensemble runs Gemini as primary and falls back to GPT-5.2 when Gemini abstains, hits a loop-break, exceeds the per-question cost limit, or shows enumeration uncertainty. All model configuration lives in TOML files (`config/*.toml`) — zero model-specific code in Rust. See [Phase 11: Inverted Ensemble](../journey/phase-11-inverted-ensemble) for details.

## Further Reading

- **[Pipeline Analysis: Where Information Is Lost](./pipeline-analysis)** --- A stage-by-stage breakdown of information loss, what we measured, and what the best competing systems do differently.

- **[Key Design Decisions](./key-decisions)** --- The rationale behind our major architectural choices: why Qdrant, why gpt-4o-mini for extraction, why agentic answering, and the experiments that validated each decision.
