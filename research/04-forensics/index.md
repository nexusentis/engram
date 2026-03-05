---
title: Forensics
sidebar_position: 4
description: Deep analysis of failures — from 58 at 88.4% to 21 at 95.8%
---

# Forensics

:::info Current State: 479/500 (95.8%), 21 failures — #1 globally
This forensics section was written at **442/500 (88.4%)** during Phase 4, analyzing 58 failures. Since then, model diversity (Phase 6-7), quick wins (Phase 9), and the GPT-5.2 ensemble (Phase 11) reduced failures to **21**: MultiSession 10, Temporal 8, Updates 3, Extraction 0, Abstention 0. Extraction hit 100% for the first time. The analysis below remains valuable — many of the same failure patterns persist at smaller scale. For the current failure distribution, see [Phase 11: Inverted Ensemble](../journey/phase-11-inverted-ensemble).
:::

At 442/500 (88.4%) on Run R-T5, Engram had 58 questions it could not answer correctly. This section dissects those failures in detail, because understanding _why_ we fail is more informative than celebrating where we succeed.

## The retrieval red herring

The first instinct when a memory system gives wrong answers is to blame retrieval. We tested that hypothesis rigorously. **Retrieval recall is 498/500 (99.6%).** The relevant conversation sessions are in the database for virtually every question. The data is there. The agent either cannot find it through its tool calls, finds it but reasons incorrectly, or finds it and refuses to commit to an answer.

This single finding reframes the entire optimization problem. We are not dealing with a data coverage gap. We are dealing with an answering quality gap -- a combination of agent search strategy, temporal reasoning, and confidence calibration.

## Gap to SOTA (at the time of this analysis)

| System | Score | Gap from Engram (442) |
|--------|-------|----------------------|
| Mastra OM | 474/500 (94.9%) | +32 |
| Honcho | 463/500 (92.6%) | +21 |
| Hindsight | 457/500 (91.4%) | +15 |
| **Engram (R-T5)** | **442/500 (88.4%)** | -- |
| Emergence | 430/500 (86.0%) | -12 |

Matching Honcho required fixing 21 of the 58 failures. As of Phase 11, **we surpassed all competitors**: Engram reached 479/500 (95.8%), #1 globally ahead of Mastra OM (94.87%). The forensic patterns below — aggregation undercounting, temporal reasoning errors, stale values — remain the dominant failure modes at the 21-failure scale.

## Failure mode distribution

The 58 failures fall into three categories of error:

| Failure Mode | Count | Percentage | Description |
|-------------|-------|------------|-------------|
| Wrong answer | 37 | 64% | Agent returns a definite answer that is incorrect |
| False abstention | 16 | 28% | Agent says "I don't have enough information" when it does |
| False positive | 5 | 9% | Agent answers confidently when it should abstain |

The dominance of wrong answers (64%) means the agent is not being overly cautious -- it is committing to answers but getting them wrong. The 16 false abstentions represent cases where the anti-abstention gate (P11) did not fire or was insufficient. The 5 false positives are entity conflation errors where the agent confuses similar-but-different concepts.

## Category breakdown

| Category | Total | Failures | Accuracy | Dominant Subtypes |
|----------|-------|----------|----------|-------------------|
| MultiSession | 121 | 18 | 85.1% | Aggregation (13), Cross-session lookup (5) |
| Temporal | 127 | 17 | 86.6% | Time-anchored (5), Date diff (6), Ordering (3) |
| Extraction | 150 | 10 | 93.3% | Preference recall (6), Fact recall (3) |
| Updates | 72 | 8 | 88.9% | Stale values (5), Off-by-one (2) |
| Abstention | 30 | 5 | 83.3% | Entity conflation (3), Hallucination (2) |

MultiSession and Temporal together account for 35 of 58 failures (60%). Both categories require the agent to synthesize information across multiple conversation sessions or time points -- tasks that demand broader search strategies and more careful reasoning than single-fact extraction.

## What the forensics tell us

The deep analysis across these pages leads to several conclusions:

1. **The off-by-one pattern is the single most common failure mode.** Across aggregation, temporal counting, and update counting, the agent consistently finds N-1 of N items. It searches, finds most items, and stops one short. This is a search completeness problem, not a reasoning problem.

2. **False abstentions on temporal questions are a confidence calibration issue.** The agent retrieves relevant dates but refuses to compute temporal differences. On the duplicated I-v10 data, these same questions passed -- the redundant facts pushed the agent above its internal confidence threshold.

3. **Stale value returns are a truncation bug.** Tool results are date-grouped oldest-first, and context truncation keeps the front (oldest). For update questions, the latest evidence gets dropped. This is a code bug, not a model limitation.

4. **Entity conflation in abstention questions is architecturally hard.** Distinguishing "tennis" from "table tennis" or "Senior SWE" from "SW Eng Manager" requires semantic precision that neither retrieval nor the current agent prompt reliably provides.

5. **The remaining gap to SOTA is primarily about reasoning quality, not data architecture.** Mastra OM achieves 94.9% with no retrieval at all. The problem space has shifted from "can we find the data" to "can we reason correctly over what we find." *Update: By Phase 11 (479/500, #1 globally), model diversity and ensemble optimization closed this gap entirely. The remaining 21 failures are representation quality problems — see [Single-Model Architecture](../lessons/single-model-architecture).*

## Sub-pages

- [**Failure Breakdown by Category**](./category-breakdown) -- Detailed tables of all 58 failures organized by benchmark category and failure subtype
- [**Temporal Failures Deep Dive**](./temporal-analysis) -- Why temporal questions regressed 20 points on clean data and what that reveals about reasoning vs. retrieval
- [**Retrieval and Code Audit**](./retrieval-audit) -- The systematic code audit that found 6 correctness bugs worth +14pp
