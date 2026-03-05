---
title: "Academic References"
sidebar_position: 2
---

# Academic References

Summaries of the key papers and systems that informed Engram's design. Organized by relevance to the LongMemEval benchmark.

## The LongMemEval Paper (ICLR 2025)

**Paper:** [arxiv.org/abs/2410.10813](https://arxiv.org/abs/2410.10813)
**Authors:** Di Wu et al.

### Key Findings

1. **Round-level storage is the best default.** Decomposing sessions into individual turns significantly improves performance over storing full sessions.

2. **Fact extraction as VALUE hurts.** Replacing stored content with extracted facts loses information needed to answer questions.

3. **Fact extraction as KEY helps.** Using facts to augment the retrieval index (key expansion) improves recall +9.4% and accuracy +5.4%. The optimal approach: facts as keys, raw turns as values.

4. **Time-aware query expansion helps.** Extracting temporal ranges from queries and filtering irrelevant content: +6.8% to +11.3% recall on temporal questions.

5. **Chain-of-Note prompting helps.** Generating reading notes for each retrieved document before answering: up to +10 absolute points.

6. **JSON-structured formatting helps.** Formatting retrieved items as structured JSON improves reasoning.

### Relevance to Engram

We use facts as both keys and values, and discard raw turns. The paper indicates this is suboptimal. Keeping raw turns as values and using facts only to augment the retrieval index would be more effective. Our date-grouped formatting (+4pp) is consistent with finding #6.

---

## EMem: Elementary Discourse Units (Nov 2025)

**Paper:** [arxiv.org/html/2511.17208v2](https://arxiv.org/html/2511.17208v2)
**Score:** 77.9% on LongMemEval-S (EMem-G variant)

### Key Idea

Uses Elementary Discourse Units (EDUs) --- short, self-contained event statements that bundle participants, temporal cues, and context. Example: "Bob spent five days in Tokyo in March 2024 to attend the Global AI Innovation Symposium 2024."

EDUs achieve a middle ground between atomic fact triples (which fragment discourse) and raw sessions (which preserve too much noise). They are non-compressive and event-structured, reducing context from 101K to 1.0K-3.6K tokens.

The EMem-G variant uses Personalized PageRank over an EDU graph for retrieval, with recall-oriented LLM filtering that biases toward inclusion over precision.

---

## Zep/Graphiti: Temporal Knowledge Graph (Jan 2025)

**Paper:** [arxiv.org/abs/2501.13956](https://arxiv.org/abs/2501.13956)

### Three-Tier Hierarchical Graph

1. **Episode Subgraph:** Raw data stored non-lossily
2. **Semantic Entity Subgraph:** Extracted entities and relationships
3. **Community Subgraph:** Groups of strongly-connected entities with summaries

### Bi-Temporal Tracking

Each edge carries four timestamps: event time creation/expiration (when it was true in the real world) and ingestion time creation/expiration (when the system learned about it). This enables retroactive corrections, fact versioning, and "what did we know and when" queries.

### Results

+18.5% accuracy gain over full-context baseline, 90% latency reduction, only 1.6K average context tokens. However, the system scores only 71.2% on LongMemEval-S, suggesting that the temporal graph architecture does not transfer well to this benchmark's question distribution.

---

## EverMemOS: Memory Operating System (Jan 2026)

**Paper:** [arxiv.org/abs/2601.02163](https://arxiv.org/abs/2601.02163)
**Score:** ~83% on LongMemEval-S

Three-stage lifecycle: Episodic Trace Formation (dialogues to MemCells with atomic facts and foresight signals), Semantic Consolidation (MemCells organized into thematic MemScenes), and Reconstructive Recollection (MemScene-guided agentic retrieval).

---

## SimpleMem: Efficient Lifelong Memory (Jan 2026)

**Paper:** [arxiv.org/abs/2601.02553](https://arxiv.org/abs/2601.02553)

Three-stage pipeline: Semantic Structured Compression (multi-view indexed memory units), Online Semantic Synthesis (instant integration of related context, eliminating redundancy), and Intent-Aware Retrieval Planning (inferring search intent to determine retrieval scope). Reports +26.4% F1 improvement and 30x token reduction.

---

## MemGAS: Multi-Granularity Association (May 2025)

**Paper:** [arxiv.org/abs/2505.19549](https://arxiv.org/abs/2505.19549)

Uses Gaussian Mixture Models to cluster and associate memories across granularity levels, with an entropy-based router that adaptively selects optimal granularity per query. The key finding is that no single granularity is optimal for all query types.

---

## A-MEM: Agentic Memory (NeurIPS 2025)

**Paper:** [arxiv.org/abs/2502.12110](https://arxiv.org/abs/2502.12110)

Zettelkasten-inspired note structure where each memory is enriched with LLM-generated keywords, tags, and contextual descriptions. Dynamically constructed links to related memories, with agent-driven memory organization rather than fixed operations.

---

## FRAME: Reflective Memory Management (ACL 2025)

**Paper:** [arxiv.org/abs/2503.08026](https://arxiv.org/abs/2503.08026)

Two types of reflection: Prospective Reflection (dynamically summarizing interactions across granularities --- utterances, turns, sessions) and Retrospective Reflection (iteratively refining retrieval using RL based on LLM-cited evidence). Reports +10% accuracy over no-memory-management baseline on LongMemEval.

---

## SGMem: Sentence Graph Memory (Sep 2025)

**Paper:** [arxiv.org/abs/2509.21212](https://arxiv.org/abs/2509.21212)

Sentence-level graphs within chunked units, capturing associations across turn, round, and session contexts. Lightweight (no LLM-based extraction needed), combining retrieved raw dialogue with generated memory. Scores 73.0% on LongMemEval-S.

---

## Synapse: Spreading Activation Memory (Jan 2026)

**Paper:** [arxiv.org/html/2601.02744v1](https://arxiv.org/html/2601.02744v1)

Combines episodic and semantic memory using spreading activation from cognitive science. Episodic memory stores time-stamped situational experiences; semantic memory uses a tree structure with conceptual relationships. Includes consolidation pathways from episodic to semantic.

---

## Surveys

- [Memory in the Age of AI Agents](https://arxiv.org/abs/2512.13564) (Dec 2025) --- Taxonomy of factual, experiential, and working memory for agents.
- [Agent Memory Paper List](https://github.com/Shichun-Liu/Agent-Memory-Paper-List) --- Comprehensive curated list of agent memory research.
- [Graph-based Agent Memory](https://arxiv.org/html/2602.05665) (Feb 2026) --- Survey of graph-based approaches to agent memory.

---

## Techniques Not Yet Widely Implemented

1. **Observational Memory** (Mastra) --- Pure text, no database, currently #1 on LongMemEval-S
2. **Bi-temporal tracking** (Zep) --- Event time vs ingestion time for corrections
3. **Multi-granularity adaptive routing** (MemGAS) --- Entropy-based granularity selection
4. **Background dreaming** (Honcho) --- Offline consolidation and deduction
5. **Prospective/Retrospective reflection with RL** (FRAME)
6. **Spreading activation** (Synapse) --- Cognitive science-inspired retrieval
7. **Elementary Discourse Units** (EMem) --- Event-centric non-compressive memory
