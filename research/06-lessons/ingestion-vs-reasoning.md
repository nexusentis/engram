---
title: "The Retrieval vs. Reasoning Gap"
sidebar_position: 2
---

# The Retrieval vs. Reasoning Gap

The single most important strategic insight from this project is that retrieval quality is not the bottleneck. At 498/500 retrieval recall (99.6%), our system finds the relevant evidence for virtually every question. At 442/500 (Phase 4), the 58 failures were reasoning and strategy errors. At 479/500 (Phase 11), the remaining 21 failures are **representation** errors — facts that are found but poorly structured, poorly connected, or lacking provenance.

This finding has evolved through the project: at 88%, the gap was reasoning quality; at 95.8%, the gap is representation quality. In both cases, retrieval is not the bottleneck — and the SOTA leaderboard corroborates this (we reached #1 globally with the same retrieval system we had at 88%).

## Retrieval Recall: 498/500

P9 (the recall diagnostic run) measured how often relevant evidence appeared in the agent's retrieved context. Of 500 questions, 498 had at least one relevant fact surfaced by the retrieval pipeline. Only 2 questions had genuine retrieval failures --- cases where the answer existed in the dataset but no retrieval query could surface it.

This means that for 498 out of 500 questions, the system has access to the information needed to answer correctly.

At **442/500 (Phase 4)**, the 58 failures broke down as:

| Failure Mode | Count | Description |
|-------------|-------|-------------|
| Aggregation errors | 13 | Agent finds wrong or incomplete items when counting across sessions |
| Temporal reasoning | 17 | Wrong temporal ordering, date arithmetic errors, or premature abstention |
| Preference recall | 6 | Generic answers instead of personalized preferences |
| Date difference computation | 6 | Agent does not call date_diff tool, or calls it with wrong dates |
| Update lookup (stale values) | 5 | Agent finds old value instead of latest |
| Cross-session lookup | 5 | Wrong session chosen, entity conflation |
| False positive (should abstain) | 5 | Hallucination, entity conflation |
| Fact recall | 3 | Wrong specific facts despite evidence present |

At **479/500 (Phase 11, #1 globally)**, model diversity (Gemini+GPT-5.2 ensemble) and quick wins reduced failures to 21:

| Category | Failures | Notes |
|----------|----------|-------|
| MultiSession | 10 (48%) | Aggregation under-counts, stochastic — dominant remaining failure mode |
| Temporal | 8 (38%) | Wrong dates, relative temporal, wrong computation |
| Updates | 3 | Persistent stale values |
| Extraction | 0 | **100% (150/150)** — first time |
| Abstention | 0 | **100% (30/30)** — fully solved by P25 |

Every one of these failure modes involves the agent making a reasoning or strategy error *after* the relevant evidence has been retrieved — or, increasingly at 95%+, the evidence being found but poorly represented (no provenance links, no holistic user summary, no temporal connections between facts).

## The Ingestion Duplication Discovery

Early in the project, we observed significant variance between ingestion runs. Two ostensibly identical ingestion passes (same model, same temperature, same seed) produced different fact counts:

| Ingestion | Facts | Messages | Notes |
|-----------|-------|----------|-------|
| I-v10 (Step 10 vanilla) | 300,800 | 262,429 | No extraction cache, parallel scheduling |
| I-v11 (with extraction cache) | 282,879 | 246,728 | Deterministic via P8 cache, seed=42 |

The ~18,000 fact difference was caused by approximately 1,500 sessions being extracted twice in the uncached run --- a consequence of `buffer_unordered` parallel scheduling. These duplicate facts were not harmful in the traditional sense (they did not contradict anything), but they provided *false signal reinforcement*: when the agent saw the same fact twice in its context, it was more confident in that answer.

This discovery had two critical implications:

1. **Score comparisons across ingestion runs are unreliable.** Up to 12 percentage points of apparent score change could be explained by ingestion variance alone. This made A/B testing of code changes extremely difficult until P8 (extraction cache) was implemented.

2. **The I-v10 "baseline" of 440/500 was inflated.** When we re-ran on clean I-v11 data (no duplicates), the score dropped to 420/500 (84.0%). The P11 anti-abstention gate + gate loop fix recovered the score to 442/500 (88.4%) on clean data, establishing a genuine baseline.

## The Variance Problem

Benchmark variance is a persistent challenge at every level:

| Level | Variance | Source |
|-------|----------|--------|
| Fast Loop (60 questions) | +/-5 questions (8pp) | LLM non-determinism in answering + judging |
| Gate (231 questions) | +/-20 questions (8.6pp) | Same, plus larger sample catches more stochastic flips |
| Truth (500 questions) | +/-5-10 questions (1-2pp) | Dampened by volume |
| Ingestion (full re-ingest) | +/-18,000 facts (12pp on score) | LLM extraction non-determinism |

Two Gate runs on the same code (202/231 and 203/231) had 21 regressions and 20 fixes that canceled out. Individual question flips are noise; only totals are meaningful. This variance means that any intervention with an expected impact below +/-3 questions on the Gate set is indistinguishable from noise.

The 3-tier testing strategy (Fast Loop / Gate / Truth) was designed to manage this: small changes are tested cheaply on 60 questions, promising results are validated on 231 questions, and only accumulated wins justify the $106 Truth run. Even so, **a neutral Fast Loop result (53/60 = baseline) is a stop signal**, not a "proceed to Gate" signal. If a change shows +0 on 60 questions, it is either not firing or firing without effect, and $49 on a Gate run will not change that.

## The SOTA Lesson: Reasoning >> Data Architecture

The most striking data point is the SOTA leaderboard itself:

| Rank | System | Score | Architecture |
|------|--------|-------|-------------|
| **1** | **Engram** | **95.8%** | **Qdrant + Gemini/GPT-5.2 ensemble agentic** |
| 2 | Mastra OM | 94.87% | No retrieval --- observation logs in context window |
| 3 | Honcho | 92.6% | Agentic + reasoning trees, no graph |
| 4 | Hindsight | 91.4% | Entity graph + 4-way retrieval |
| 5 | Emergence | 86.0% | Accumulator, session-level NDCG |

When this analysis was first written, Engram was at 88.4% (#4). The jump to 95.8% (#1) came from model diversity (Gemini + ensemble routing, +25q; GPT-5.2 fallback upgrade, +7q) and quick wins (P25, judge fix, routing fix, +5q) — not from improving retrieval or data architecture.

Mastra OM uses **no vector database, no graph database, no retrieval mechanism of any kind.** It compresses conversations into structured observation logs using background LLM agents, then places those logs directly in the context window. We surpassed it through model diversity and ensemble optimization alone.

Meanwhile, graph-heavy systems dramatically underperform:

| System | Score | Architecture |
|--------|-------|-------------|
| Zep | 71.2% | Bi-temporal knowledge graph |
| SGMem | 73.0% | Sentence KNN graph |
| Mem0 | 68.4% | Vector + graph dual memory |

The correlation is clear: **more sophisticated data architecture does not translate to higher scores.** What separated 88% from 95.8% was model diversity and targeted logic fixes, not data architecture. What separates 95.8% from higher scores may be **representation quality** — how facts are structured and connected (provenance, temporal relationships, holistic user memory). See [Single-Model Architecture](./single-model-architecture) for the current analysis.

## Implications

1. **Do not invest further in retrieval at 99.6% recall.** Every retrieval augmentation we tried (query expansion, graph prefetch, RRF tuning) was neutral or harmful. The data is already there.

2. **The bottleneck shifts as you climb.** At 88%, the gap was reasoning quality — better models and model diversity closed it (+30q from Phase 6-7). At 94%, the gap is representation quality — how facts are structured, connected, and surfaced. The [Phase 9 architecture review](../journey/phase-9-architecture-review) identified five specific representation gaps.

3. **Model diversity is the fastest lever above 88%.** All single-model interventions in Phase 5 (~$800) produced zero net improvement. One ensemble router (~$50) gained +15 questions. Try a different model before redesigning the architecture.

4. **Ingestion determinism is a prerequisite for meaningful experiments.** The extraction cache (P8) should be used for all benchmark runs. Without it, ingestion variance dominates all other effects.

5. **Stochastic variance requires statistical discipline.** Never read individual question flips. Only trust totals. At 94%+, MultiSession is the noisiest category — 8 stochastic regressions in the Phase 9 Truth run masked 13 targeted fixes.
