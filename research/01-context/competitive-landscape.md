---
title: "Competitive Landscape"
sidebar_position: 2
---

# Competitive Landscape

By the time we began this project, LongMemEval-S had attracted submissions from over 20 systems spanning startups, research labs, and open-source projects. The leaderboard evolved rapidly --- new entries appeared weekly, and the top score climbed from 86% to nearly 95% in under three months. This page documents the full competitive landscape as we understood it during Engram's development, including the architectural patterns that separated the leaders from the rest.

## Overall Leaderboard

At project start:

| Rank | System | Model | Overall | Architecture |
|------|--------|-------|---------|-------------|
| 1 | Mastra OM | gpt-5-mini | 94.87% | Pure text, no vector DB |
| 2 | Mastra OM | gemini-3-pro | 93.27% | Pure text, no vector DB |
| 3 | Honcho | Gemini 3 Pro | 92.6% | Agentic + fine-tuned models |
| 4 | Hindsight | Gemini-3 | 91.4% | Entity graph + 4-way retrieval |
| 5 | Honcho | Claude Haiku 4.5 | 90.4% | Agentic + fine-tuned models |
| 6 | Hindsight | OSS-120B | 89.0% | Entity graph + 4-way retrieval |
| 7 | Emergence | Internal | 86.0% | Accumulator (Chain-of-Note) |
| 8 | Supermemory | Gemini-3 | 85.2% | Proprietary |
| 9 | Supermemory | GPT-5 | 84.6% | Proprietary |
| 10 | Mastra OM | GPT-4o | 84.23% | Pure text, no vector DB |
| 11 | Hindsight | OSS-20B | 83.6% | Entity graph + 4-way retrieval |
| 12 | EverMemOS | - | 83.0% | MemCell lifecycle |
| 13 | Emergence Simple | GPT-4o | 82.4% | Simple RAG + facts |
| 14 | Supermemory | GPT-4o | 81.6% | Proprietary |
| 15 | Mastra RAG | GPT-4o (topK=20) | 80.05% | Simple RAG + formatting |
| 16 | EMem-G | - | 77.9% | EDU-based + graph |
| 17 | Engram (us) | gpt-4o-mini / GPT-4o | 78.0% (50q val) | Hybrid search + date-grouped, Qdrant |
| 18 | Zep | GPT-4o | 71.2% | Fact extraction + graph |
| 19 | Full-context | GPT-4o | 60.2% | Baseline |

*Note: Our 78% was an early 50-question validation score. Our eventual best on the full 500-question benchmark reached **95.8% (479/500)** with the Phase 11 Gemini+GPT-5.2 ensemble, placing us **#1 globally** — surpassing Mastra OM (94.87%). See the [Journey section](../journey/) for the full progression.*

## Per-Category Breakdown

The per-category numbers reveal where different architectures excel and struggle. The categories are not equally difficult --- Extraction is straightforward for most systems, while Temporal and Multi-Session separate the top tier from the rest.

| System | SS-User | SS-Asst | SS-Pref | Knowledge Update | Temporal | Multi-Session | Overall |
|--------|---------|---------|---------|-----------------|----------|---------------|---------|
| Full-ctx GPT-4o | 81.4% | 94.6% | 20.0% | 78.2% | 45.1% | 44.3% | 60.2% |
| Zep (GPT-4o) | 92.9% | 80.4% | 56.7% | 83.3% | 62.4% | 57.9% | 71.2% |
| Mastra RAG (GPT-4o) | 97.1% | 100% | 56.7% | 84.6% | 75.2% | 76.7% | 80.0% |
| Supermemory (GPT-4o) | 97.1% | 96.4% | 70.0% | 88.5% | 76.7% | 71.4% | 81.6% |
| Supermemory (Gemini-3) | 98.6% | 98.2% | 70.0% | 89.7% | 82.0% | 76.7% | 85.2% |
| Hindsight (OSS-120B) | 100% | 98.2% | 86.7% | 92.3% | 85.7% | 81.2% | 89.0% |
| Honcho (Haiku 4.5) | 94.3% | 96.4% | 90.0% | 94.9% | 88.7% | 85.0% | 90.4% |
| Hindsight (Gemini-3) | 97.1% | 96.4% | 80.0% | 94.9% | 91.0% | 87.2% | 91.4% |

*Mastra OM per-category breakdown not yet published at time of writing.*

### What the numbers tell us

**Extraction (SS-User, SS-Asst, SS-Pref)** is largely solved above the 80% tier. Most systems score 94%+ on User and Assistant facts. SS-Preference is the outlier --- full-context GPT-4o gets only 20%, while Honcho reaches 90%. Preference questions require understanding nuanced user attitudes, not just recalling explicit statements.

**Knowledge Update** separates systems that track temporal ordering from those that do not. The jump from Zep (83.3%) to Honcho (94.9%) reflects the difference between storing facts without version tracking and explicitly handling superseded information.

**Temporal** is the single hardest category, and the one where architectural choices matter most. Full-context GPT-4o scores only 45.1% --- even with all information available, the model struggles with temporal reasoning. Systems that provide explicit temporal scaffolding (date-grouped formatting, relative time annotations, temporal retrieval channels) perform dramatically better.

**Multi-Session** is the second hardest, requiring aggregation across many conversation sessions. This is where agentic multi-step retrieval shows its value --- single-pass retrieval cannot guarantee finding all mentions of a topic spread across dozens of sessions.

## The Three Architectural Camps

The leaderboard entries cluster into three distinct architectural paradigms, each with different trade-offs.

### Camp 1: Context-Window Compression (Mastra OM)

**Core idea**: Keep everything in the LLM context window. Use background agents to compress and prioritize memories. No retrieval step at all.

- No retrieval failures possible --- everything is in context
- Scales to approximately 500 sessions with modern context windows (1M+ tokens)
- Prompt-cacheable for cost efficiency (append-only context, stable prefix)
- Requires aggressive compression for larger histories
- **Best for**: Bounded conversation histories where highest accuracy is the priority

Mastra OM compresses approximately 115K tokens of raw conversation into 3--30K tokens of prioritized observations, which fit comfortably in a modern context window. The LLM then reasons directly over the full compressed history.

### Camp 2: Agentic Retrieval (Honcho, Hindsight)

**Core idea**: Store raw messages and extracted facts in a database. Use a tool-calling agent to iteratively search, reformulate queries, and cross-reference until it has enough evidence to answer.

- Multiple retrieval strategies (semantic, keyword, temporal, graph)
- Agent can reformulate queries when initial results are insufficient
- Handles unbounded conversation histories
- More expensive per query (multiple LLM calls per question)
- **Best for**: Production systems with unbounded histories

Honcho gives its answering agent 7 tools and up to 20 iterations. Hindsight provides 5 tools and 10 iterations. Both systems store raw conversation messages alongside extracted facts, allowing the agent to drill down from high-level facts to original context.

### Camp 3: Simple RAG (Mastra RAG, Emergence Simple, Engram)

**Core idea**: Store in a vector database, perform single-pass retrieval, generate a single-pass answer.

- Low latency, low cost, simple implementation
- Ceiling appears to be approximately 82% with good formatting (Mastra RAG with topK=20)
- Higher ceilings possible with agentic answering layered on top
- **Best for**: Low-latency, cost-sensitive applications

This was Engram's starting point. Over the course of development, we layered an agentic answering loop on top of our RAG foundation, effectively moving toward Camp 2 while keeping Camp 3's storage layer.

## Critical Differentiators

Analyzing the leaderboard, four factors consistently separated high-scoring from low-scoring systems.

### 1. Raw message preservation (biggest impact)

Every system above 80% stores raw conversation turns in some form. The top systems use extracted facts for indexing and routing, but they always preserve access to the original conversation context.

| System | Raw messages available? |
|--------|----------------------|
| Mastra OM | Yes (preserved in compressed observations) |
| Honcho | Yes (searchable via `search_messages` and `grep_messages`) |
| Hindsight | Yes (retrievable via `expand` tool) |
| Mastra RAG | Yes (stored with timestamps) |
| **Engram** | **No --- only extracted facts, raw messages discarded** |

This was our single largest architectural gap. The LongMemEval paper's finding that "facts as VALUE hurts" was playing out across the entire leaderboard.

### 2. Retrieval strategy (second biggest impact)

Systems above 90% use agentic multi-step retrieval. The agent can reformulate queries, try different search strategies, and cross-reference results:

- Honcho: 7 tools, up to 20 iterations
- Hindsight: 5 tools, up to 10 iterations
- Engram (initial): single-pass retrieve-then-answer

### 3. Temporal handling (third biggest impact)

Every system in the top tier invests heavily in temporal reasoning:

- Mastra OM: three-date model with explicit temporal anchoring and relative time annotations
- Honcho: absolute timestamps enforced during extraction, dedicated temporal search tools
- Hindsight: dedicated temporal retrieval channel with dateparser-based query analysis
- Mastra RAG: gained +40pp on temporal questions from timestamp formatting alone
- Engram (initial): was using `Utc::now()` instead of actual session dates

### 4. Answering sophistication

Top systems use category-aware answering strategies rather than a single generic prompt:

- Honcho: 237-line structured prompt with explicit strategies for enumeration, summarization, and knowledge updates
- Hindsight: hierarchical search strategy (mental models, then observations, then raw facts)
- Mastra OM: full context in window, LLM does all reasoning naturally

## Our Starting Position

When we began benchmarking Engram, we sat at approximately 78% on a 50-question validation set (later calibrated to roughly 72% on the full 500-question benchmark). We were firmly in Camp 3 --- simple RAG with Qdrant vector search, atomic fact extraction, and single-pass answering.

The gap analysis was clear:
- We were **6 points below** Mastra RAG (80%), which used the same model (GPT-4o) and a similar topK --- the difference was formatting and raw message preservation
- We were **13 points below** Hindsight (91.4%), which had entity graphs, 4-way retrieval, and an agentic answerer
- We were **23 points below** Mastra OM (94.87%), which had abandoned retrieval entirely

The question was not whether we *could* close these gaps, but *which* gaps were closable within our Qdrant + atomic-facts architecture, and which would require fundamental redesign. The rest of this research documentation tells that story.
