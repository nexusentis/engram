---
title: "Key Design Decisions"
sidebar_position: 2
---

# Key Design Decisions

This section documents the major architectural decisions we made during the project, the evidence that informed each one, and --- where applicable --- the experiments that validated or invalidated our choices. These are presented roughly in the order they were made.

## Why Qdrant

We chose Qdrant as our primary storage layer because it provides vector search and fulltext search in a single store, eliminating the need to maintain separate systems for semantic and keyword retrieval.

The practical advantages that drove the decision:

- **Hybrid search in one query.** Qdrant supports both dense vector search (cosine similarity over text-embedding-3-small embeddings) and fulltext search (BM25-like scoring over indexed content fields). We fuse these channels using Reciprocal Rank Fusion (RRF). Running both channels against the same store simplifies the architecture compared to maintaining, say, Qdrant for vectors and Elasticsearch for keywords.
- **Structured payload filtering.** Qdrant allows arbitrary JSON payloads on each point, with keyword and datetime indexes. This means we can filter by `user_id`, `epistemic_type`, or `t_valid` date ranges without a separate metadata store.
- **Rust-native client.** Since Engram is a Rust project, the official Qdrant Rust client integrates cleanly with our async pipeline.
- **Snapshot and restore.** Qdrant's snapshot mechanism proved essential for reproducible benchmarking. We could ingest once, snapshot, and restore to the same state across dozens of experiment runs. This was not a selection criterion, but it became operationally critical.

The main limitation we encountered is that Qdrant does not natively support graph queries. When we later explored entity-graph augmentation (P18, P20), we had to stand up a separate SurrealDB instance. However, graph-based approaches ultimately proved neutral or harmful to our score, so this limitation did not constrain our final results.

## Why gpt-4o-mini for Extraction

The extraction model processes all ~24,000 conversation sessions to produce atomic facts. We evaluated two models head-to-head in a controlled experiment.

### The Experiment: Run 8a vs Run 8b (Day 4)

Both runs used the same extraction prompt, the same 50-question validation split, and fresh ingestion with identical parameters except for the model:

| Run | Model | Score | Extraction | Temporal | Cost | Speed |
|-----|-------|-------|------------|----------|------|-------|
| 8a | gpt-4o-mini | 38/50 (76%) | 12/15 | 11/13 | ~$3 | ~380 sessions/min at concurrency 100 |
| 8b | gpt-5-nano | 36/50 (72%) | 12/15 | 10/13 | ~$8 | ~13 sessions/min at concurrency 10 |

gpt-4o-mini outperformed gpt-5-nano by 2 questions (+4pp) while being approximately 50x faster and 2.5x cheaper. The speed difference is the most consequential: at 380 sessions per minute, a full ingestion of 24,000 sessions completes in roughly 60 minutes. At 13 sessions per minute, the same ingestion would take over 30 hours.

The likely explanation for gpt-5-nano's worse performance: at the time of our experiments, gpt-5-nano did not support temperature=0 (it required temperature=1.0 or used its default), introducing non-determinism into extraction. Our code detected this automatically and omitted the temperature parameter for nano models. The resulting extraction variance meant that repeated ingestions could produce different fact sets, complicating reproducibility. gpt-4o-mini at temperature=0.1 (later temperature=0.0) produced far more consistent extractions.

This was one of the clearest decisions in the project. We never revisited it.

## Why gpt-4o for Answering

The answering model handles the agentic reasoning loop: interpreting questions, calling search tools, reasoning over retrieved facts, and producing final answers. We chose gpt-4o because it offered the best reasoning quality available within our cost constraints at the time.

The key factors:

- **Reasoning quality.** The answering agent must handle temporal arithmetic, cross-session synthesis, enumeration with deduplication, and nuanced abstention decisions. These tasks require strong instruction-following and multi-step reasoning. gpt-4o was the strongest generally-available model for these tasks during our experiment window.
- **Function calling reliability.** The agentic loop depends on the model correctly selecting and parameterizing tools across up to 20 iterations. gpt-4o's function-calling behavior was the most reliable we tested.
- **Cost.** At our OpenAI Tier 4 rate limits (2M TPM for gpt-4o), we could process approximately 7 questions per minute at a cost of roughly $0.20 per question. A full 500-question benchmark run costs approximately $100. This was expensive but manageable for a research project with a ~$1,400 total budget.

We did not conduct a formal model comparison for answering the way we did for extraction. The decision was based on qualitative assessment and the practical constraint that no cheaper model could handle the agentic loop reliably.

## Why Agentic Answering Won Over Single-Pass

This was the single most impactful architectural decision in the project. The evidence came from a direct A/B comparison.

### The Pivotal Experiment: Run #20 vs Run #21 (Day 6)

Both runs used identical data (ingestion #18), identical code (the same 6 correctness fixes from an independent code audit), and differed only in whether the answering stage was a single LLM call or an agentic tool-calling loop:

| Run | Mode | Multi | Abst | Upd | Temp | Extr | Total |
|-----|------|-------|------|-----|------|------|-------|
| #20 | Non-agentic | 7/12 | 3/3 | 7/7 | 10/13 | 13/15 | 40/50 (80%) |
| #21 | Agentic | **11/12** | 3/3 | 7/7 | 8/13 | 13/15 | **42/50 (84%)** |

The agentic mode gained +4 on MultiSession (7 to 11 out of 12) at the cost of -2 on Temporal (10 to 8 out of 13). The net gain was +2 questions (+4pp) on this split. But the more important signal was *where* the gains came from: MultiSession questions require finding facts spread across multiple conversation sessions, which inherently demands multiple search queries with different phrasings. A single-pass retriever gets one shot; the agent can reformulate and search again.

By Run #26, after further tool correctness fixes and strategy detection refinements, the agentic mode reached 45/50 (90%) on the validation split. The non-agentic mode on the same data never exceeded 41/50 (82%). The gap widened as we improved the tools the agent had access to.

### Why the first agentic attempts failed

It is worth noting that our first agentic run (Run #3, Day 2) actually scored *worse* than non-agentic: 35/50 vs 37/50. This was not because agentic answering is inherently worse, but because the tools were broken. The agent was calling tools that returned cross-user contaminated results, used fragile temporal filters, and searched only one collection instead of four. The lesson --- later codified as a core project principle --- is that **tool correctness matters more than prompt engineering**. The +14pp gain from agentic mode only materialized after fixing 6 tool-level bugs.

## Why We Kept Atomic Facts

The MemoryBank paper that introduced the LongMemEval benchmark recommends storing raw conversation turns as values. Several top systems (Honcho, Hindsight) follow this advice by maintaining dual storage: raw messages alongside extracted facts. We considered and rejected this approach for Engram.

Our reasoning:

- **Extraction as compression.** The benchmark dataset contains ~24,000 sessions. Storing raw turns would require maintaining a second parallel index (raw messages) with its own retrieval channel. We opted to invest in improving extraction quality instead of maintaining dual storage.
- **Retrieval precision.** Atomic facts are short, semantically dense, and embed well. Raw conversation turns are long, noisy, and contain filler language that dilutes embedding quality. Our hybrid search (vector + fulltext) works better on clean atomic facts than on raw multi-turn conversations.
- **The 5-dimension experiment failed.** Run #4 tested enhanced extraction with 5-dimension facts (what/when/where/who/why), following the Hindsight model. It scored 34/50 (68%) vs the 36/50 (72%) baseline --- a regression. The added complexity in the extraction prompt produced worse facts, not better ones. This discouraged us from further enriching the fact model.

This decision has a clear cost. Our retrieval recall is 498/500 (99.6%) --- the facts are *in* Qdrant for virtually every question --- but 58 questions still fail because the agent cannot find the right facts or reason correctly over them. Some of these failures trace directly to details that were in the original conversations but not captured in any extracted fact. Raw message storage would likely recover a subset of these, and this remains the most promising unexplored avenue for further improvement.

## The Extraction Model Comparison in Detail

Beyond the Run 8a/8b head-to-head, several observations informed our extraction model choice:

### Temperature and Determinism

We discovered early that extraction non-determinism was a major source of benchmark noise. Two ingestion runs with identical code and model but temperature=0.1 could produce fact sets that differed by hundreds of facts, leading to score swings of up to 12 percentage points on the full 500-question benchmark. This variance was initially attributed to "ingestion randomness" but was later traced to two causes:

1. **Duplicated sessions.** Our ingestion pipeline (I-v10) contained ~1,500 sessions that were extracted twice due to a deduplication bug, inflating the fact count from ~283K to ~301K. The duplicates acted as signal reinforcement in the answering stage.
2. **Temperature-induced extraction variance.** At temperature=0.1, gpt-4o-mini produces slightly different extractions on each run. At temperature=0.0 with seed=42, extractions become nearly deterministic.

We moved to temperature=0.0 and seed=42 for all production ingestion runs starting with I-v1 (Day 9). We later built an extraction cache (P8) that stores the LLM's extraction output keyed by session content hash, achieving 100% cache hit rates on subsequent runs and eliminating extraction variance entirely.

### The gpt-5-nano Temperature Problem

gpt-5-nano posed a specific challenge: it did not support temperature=0 through the API at the time of our experiments. Our code detected nano models via a string match (`model.contains("nano")`) and omitted the temperature parameter, letting the model use its default (temperature=1.0). This meant every nano extraction run was inherently non-deterministic.

Even setting aside the score difference, the inability to achieve deterministic extraction with gpt-5-nano was disqualifying for a benchmark-oriented project where reproducibility is essential.

## Seed and Determinism Choices

Deterministic ingestion became a first-class engineering priority after we discovered the 12pp variance problem. Our final configuration:

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Extraction model | gpt-4o-mini | Fastest, cheapest, best-scoring option |
| Temperature | 0.0 | Eliminates extraction variance |
| Seed | 42 | Arbitrary but fixed; enables API-level determinism |
| Extraction cache | Enabled (`EXTRACTION_CACHE_DIR`) | 100% cache hits on re-runs; zero API calls for cached sessions |
| Concurrency | 100-150 | Maximizes throughput within Tier 4 rate limits (10M TPM for gpt-4o-mini) |

The extraction cache (P8) was the final piece. By caching the raw LLM output for each session (keyed by a hash of the session content), we eliminated the last source of non-determinism: even if the OpenAI API returned slightly different results for the same input (which can happen even at temperature=0 across API versions), the cache ensures identical fact sets across runs. This made it possible to isolate the effect of query-time changes from ingestion-time changes, which was essential for the 3-tier testing strategy (Fast Loop / Gate / Truth) that governed all experiments from Week 2 onward.
