---
title: "Phase 1: Building the Baseline"
sidebar_position: 1
description: "Non-agentic retrieval takes us from 0% to ~78% (Week 1)"
---

# Phase 1: Building the Baseline (Week 1)

Phase 1 covers the first four days of benchmark work, from the very first run to establishing a non-agentic baseline of 78%. This phase taught three foundational lessons: data presentation matters more than model sophistication, agentic is not automatically better than non-agentic, and smaller extraction models can outperform larger ones.

**Terminology**: A *non-agentic* pipeline retrieves context in a single pass and generates one answer. An *agentic* pipeline gives the LLM access to tools (search, temporal queries, etc.) and lets it iteratively plan its own retrieval strategy across multiple turns.

All runs in this phase used a 50-question validation subset. Runs before Run #17 (Day 5) did **not** have the numeric answer parsing fix, so scores are artificially lower -- numeric gold answers like `3`, `120`, `15` were silently dropped to empty strings, making correct answers appear as false positives. The relative ordering of runs is still meaningful.

## Run #1-2: First Signs of Life (Day 1)

The very first benchmark runs used a non-agentic pipeline: query analysis, parallel vector + fulltext + message retrieval, RRF fusion, single-pass temperature-0 answering. Fresh ingestion with temperature=0.1.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #1 | non-agentic | Timestamp fix | 9/12 | 3/3 | 6/7 | 11/13 | 7/15 | **36/50 (72%)** |
| #2 | non-agentic | + Raw message storage | 9/12 | 2/3 | 6/7 | 9/13 | 11/15 | **37/50 (74%)** |

Run #2 added raw message storage alongside extracted facts, giving the answerer access to original conversation text. This improved Extraction from 7/15 to 11/15 (+4 questions) -- the raw messages contained details that the extraction pipeline had discarded.

These runs established that a basic RAG pipeline with Qdrant vector search, OpenAI embeddings, and gpt-4o answering could reach the low-70s on this benchmark. Not terrible for a first attempt, but far from competitive.

## Run #3-4: First Agentic Attempts (Day 2)

With a non-agentic baseline in hand, the natural next step was to try an agentic approach: give the LLM access to tools (search_facts, search_messages, search_entity, etc.) and let it plan its own retrieval strategy. The hypothesis was that an agent could handle complex multi-step questions -- temporal reasoning, cross-session synthesis -- better than a single retrieval pass.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #3 | **agentic** | + Agentic loop (7 tools, max_iter=10) | 7/12 | 2/3 | 5/7 | 9/13 | 12/15 | **35/50 (70%)** |
| #4 | **agentic** | + Enhanced extraction (5-dim) | 6/12 | 2/3 | 5/7 | 10/13 | 11/15 | **34/50 (68%)** |

The results were sobering. Agentic mode was **worse** than non-agentic across the board. Run #3 scored 70% versus the non-agentic 74% on the same data. Run #4, with enhanced 5-dimensional extraction, dropped further to 68%.

The diagnostic report (Day 6) would later identify the structural reasons:

1. **Retrieval asymmetry**: Non-agentic used a unified pipeline (query analysis + parallel vector/fulltext/message retrieval + RRF fusion). Agentic used weaker, separate tool calls.
2. **Tool correctness gaps**: `search_facts` did not filter by `user_id`, `search_entity` queried a non-existent collection `"memories"`, and message hybrid search was append-then-truncate rather than real fusion.
3. **Loop control problems**: Duplicate-call skipping, forced abstention after 3 duplicate iterations, and per-question cost caps caused premature exits.

At this point, agentic looked like a dead end. As we will see in [Phase 2](./phase-2-agentic), that conclusion was premature -- the problem was not the agentic approach itself, but the bugs in its tools.

## Run #5: The Date-Grouped Format Breakthrough (Day 2)

Run #5 introduced date-grouped formatting: instead of presenting retrieved facts as a flat list, they were organized chronologically with date headers. This single change to data *presentation* -- not retrieval, not the model, not the prompt -- produced the biggest single improvement of Phase 1.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #5 | non-agentic | + Date-grouped format | 8/12 | 3/3 | 5/7 | 11/13 | 12/15 | **39/50 (78%)** |

**+4 percentage points from formatting alone.** Temporal reasoning jumped to 84.6% (11/13). The chronological grouping gave gpt-4o the context it needed to reason about time -- "this happened before that" becomes obvious when facts are sorted by date rather than by retrieval score.

This was the first of several lessons about the importance of context presentation over retrieval sophistication. The same data, presented differently, yielded dramatically different results.

## Run #6a-6b: The LLM Reranking Disaster (Day 2)

Encouraged by the formatting win, the next experiment tried LLM reranking: using a language model to re-score and reorder retrieved results before presenting them to the answerer. This is a technique with strong theoretical motivation -- the reranker can assess relevance more holistically than embedding cosine similarity.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #6a | non-agentic | + LLM reranking + MMR | 5/12 | 3/3 | 4/7 | 8/13 | 11/15 | **31/50 (62%)** |
| #6b | non-agentic | + LLM reranking only | 4/12 | 3/3 | 6/7 | 10/13 | 9/15 | **32/50 (64%)** |

LLM reranking was catastrophically harmful. Run #6a dropped to 62% (from 78%), a **16 percentage point loss**. Even without MMR diversity scoring (Run #6b), reranking still scored 64% -- worse than the very first baseline.

The reranker destroyed the temporal signal. MultiSession collapsed from 8/12 to 4-5/12, and Temporal dropped from 11/13 to 8-10/13. The reranker optimized for topical relevance but disrupted the chronological ordering that temporal reasoning depends on. It also introduced a latency and cost penalty for no benefit.

Both runs were immediately reverted. LLM reranking remained on the "confirmed dead ends" list for the rest of the project.

## Run #7: Enhanced Agentic -- Still Worse (Day 2)

One more attempt at making agentic work, this time with enhanced prefetch (15/10/20 facts by category), strategy detection, and a 6K token truncation limit. The idea was that if the agent started with better initial context, it could reason more effectively.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #7 | **agentic** | Enhanced agentic (prefetch, strategy, 6K trunc) | 7/12 | 2/3 | 6/7 | 6/13 | 12/15 | **33/50 (66%)** |

Still worse than non-agentic. Temporal reasoning was devastated: 6/13 (46%) versus 11/13 (85%) for non-agentic Run #5 on the same data. The agentic approach fragmented what should have been a single coherent temporal reasoning task into multiple disconnected tool calls, losing context at each step.

At the end of Day 2, the scorecard was clear: non-agentic at 78% was the undisputed leader. Agentic was consistently 8-12 points behind.

## Run #8a-8b: Extraction Model Comparison (Day 4)

The final Phase 1 experiment compared extraction models: gpt-4o-mini versus gpt-5-nano for the ingestion pipeline (extracting structured facts from conversation sessions).

| Run | Mode | Extraction Model | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------------|-------|------|-----|------|------|-----------|
| #8a | non-agentic | gpt-4o-mini | 8/12 | 3/3 | 5/7 | 11/13 | 12/15 | **38/50 (76%)** |
| #8b | non-agentic | gpt-5-nano | 7/12 | 3/3 | 4/7 | 10/13 | 12/15 | **36/50 (72%)** |

gpt-4o-mini outperformed gpt-5-nano by +2 questions. More importantly, gpt-4o-mini was approximately 50x faster for ingestion. The "bigger model = better" assumption did not hold for extraction -- the task is structured enough that the smaller model handles it well, and its speed advantage allows higher concurrency and faster iteration.

gpt-4o-mini became the permanent extraction model.

## Phase 1 Summary

| Milestone | Score | Key Insight |
|-----------|-------|-------------|
| First non-agentic baseline | 72-74% | Basic RAG works, raw messages help |
| First agentic attempt | 68-70% | Agentic worse due to tool bugs (not yet diagnosed) |
| Date-grouped formatting | **78%** | +4pp from presentation alone -- format matters |
| LLM reranking | 62-64% | Catastrophically harmful, destroyed temporal signal |
| Enhanced agentic | 66% | Still worse, temporal reasoning at 46% |
| gpt-4o-mini vs gpt-5-nano | 76% vs 72% | Smaller extraction model wins on both speed and quality |

### Key Lessons

1. **Data presentation matters more than retrieval sophistication.** Date-grouped formatting (+4pp) outperformed every retrieval experiment in Phase 1. The same facts, organized differently, yield dramatically different reasoning quality.

2. **Agentic is not automatically better.** Over 6 head-to-head comparisons in Phase 1, non-agentic won 5 of 6 (average delta: -4pp for agentic). The agentic approach had fundamental tool correctness issues that would not be diagnosed until Phase 2.

3. **Smaller extraction models can outperform larger ones.** gpt-4o-mini beat gpt-5-nano on both quality (+2q) and speed (50x faster). For structured extraction tasks, the speed advantage of a smaller model enables better engineering iteration.

4. **LLM reranking can be actively harmful.** It destroyed the chronological ordering that temporal reasoning depends on. Not every technique with strong theoretical motivation survives contact with a real benchmark.

The non-agentic pipeline at 78% would remain the baseline until a diagnostic report early in Week 2 identified the specific tool bugs holding agentic back. Fixing those bugs would change everything -- as described in [Phase 2](./phase-2-agentic).
