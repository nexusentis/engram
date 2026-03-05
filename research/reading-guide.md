---
title: "The Engram Story"
sidebar_position: 0
slug: reading-guide
description: "How we built a competitive long-term memory system for LLMs, from 0% to 95.8% on LongMemEval-S — and what we learned about the limits of retrieval-augmented generation."
---

# The Engram Story

**Engram** is a Rust-native long-term memory system for LLMs. Over approximately four weeks, we systematically engineered it from a blank slate to **479/500 (95.8%)** on the LongMemEval-S benchmark — reaching #1 globally, surpassing Mastra OM (94.87%) and ahead of Honcho (92.6%), Hindsight (91.4%), and Emergence AI (86%).

This documentation tells that story: the architecture, the breakthroughs, the dead ends, and the hard-won engineering lessons from ~$1,500 of experiments.

*By Federico Rinaldi*

## The Score Arc

```
Week 1:   36/50  →  45/50    Non-agentic baseline → agentic + tool fixes
Week 2:  419/500 → 440/500   First full benchmark → tier 1-2 iterations
Week 3:  440/500 → 467/500   Clean data, gate engineering, model upgrade, ensemble
Week 3+:                      Productionization — monolith → 6 crates, REST server, -12K LOC
Week 4:  467/500 → 472/500   Quick wins, architecture review, Abstention 100%
Week 4:  472/500 → 479/500   GPT-5.2 ensemble (#1 globally), Extraction 100%
```

## Reading Guide

This documentation is organized as a narrative you can read front-to-back, or jump into any section:

### [1. Context](./context/)
What is LongMemEval? Who are we competing against? What does the competitive landscape look like?

### [2. Architecture](./architecture/)
Our pipeline design: ingestion, storage, retrieval, and answering. The key decisions we made and why.

### [3. The Journey](./journey/)
The chronological story of going from zero to 95.8% --- eleven phases, from the first baseline through the ensemble router to the GPT-5.2 ensemble that reached #1 globally.

### [4. Forensics](./forensics/)
Deep analysis of our remaining failures. What categories hurt us, why, and what the failure modes tell us about the problem.

### [5. Dead Ends](./dead-ends/)
**The most valuable section for practitioners.** Everything we tried that didn't work, why it failed, and the engineering discipline rules we extracted from $1,500+ of failed experiments.

### [6. Lessons & Future](./lessons/)
What actually moves the score at each stage, the gap between retrieval and reasoning, and the roadmap beyond 95.8%.

### [Appendices](./appendices/)
Full benchmark run history (50+ runs), execution logs, academic references, and independent review prompt archives.

## Key Numbers

| Metric | Value |
|--------|-------|
| Best score | 479/500 (95.8%) |
| Total experiments | 50+ runs |
| Total cost | ~$1,500 |
| Retrieval recall | 498/500 (99.6%) |
| Ingested facts | 282,879 |
| Sessions processed | 23,867 |
| Architecture | Qdrant + gpt-4o-mini extraction + Gemini/GPT-5.2 ensemble agentic answering |

## SOTA Context

| Rank | System | Score | Architecture |
|------|--------|-------|-------------|
| **1** | **Engram** | **95.8%** | **Qdrant + Gemini/GPT-5.2 ensemble agentic** |
| 2 | Mastra OM | 94.87% | No retrieval — observation logs in context |
| 3 | Honcho | 92.6% | Agentic + fine-tuned models |
| 4 | Hindsight | 91.4% | Entity graph + 4-way retrieval |
| 5 | Emergence | 86.0% | Accumulator (Chain-of-Note) |

Phase 11 replaced GPT-4o with GPT-5.2 as the ensemble fallback model — same code, same triggers, strictly better fallback. The result: **479/500 (95.8%)**, surpassing Mastra OM to reach #1 globally. Extraction hit 100% (150/150) for the first time. The remaining 21 failures are concentrated in MultiSession (10/21 = 48%) and Temporal (8/21 = 38%).
