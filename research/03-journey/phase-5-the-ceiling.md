---
title: "Phase 5: Hitting the Ceiling"
sidebar_position: 5
description: "Every intervention at 88.4% is neutral or harmful — P12 through P21 (Week 3)"
---

# Phase 5: Hitting the Ceiling (Week 3)

Phase 5 is the story of running into a wall. After reaching 442/500 (88.4%) on clean data with P11, ten subsequent proposals -- spanning answerer post-processing, evidence table finalization, agent evidence arrays, graph tool exposure, graph prefetch augmentation, and temporal scatter search -- all proved neutral or harmful. The cumulative cost of these experiments exceeded $400 and produced zero net improvement.

This phase is documented not as a failure but as a systematic elimination of approaches, narrowing the solution space toward what might actually work at 90%+.

## The 58 Remaining Failures

Before describing the interventions, it helps to understand what they were trying to fix. The full forensics report (R-T5, 442/500, I-v11 clean data) classified the 58 failures:

| Subtype | Count | Fixable? |
|---------|-------|----------|
| Aggregation (MultiSession) | 13 | Off-by-one undercounts, promote count reducer |
| Temporal (various) | 17 | 5 time-anchored unfixable, rest are date_diff/calc errors |
| Preference recall (Extraction) | 6 | Need personalization model |
| Date diff | 6 | Calculation errors or wrong date identification |
| Update lookup (stale values) | 5 | Truncation bug + gate fixes |
| Cross-session lookup | 5 | Hard -- vocabulary mismatch, wrong session |
| Should abstain (false positive) | 5 | Hard -- entity conflation (tennis vs table tennis) |
| Fact recall | 3 | Hard -- data scattered across sessions |

The failure modes suggested two categories of fixes: answerer quality (truncation, routing, counting) and search strategy (finding the right facts in the first place). Phase 5 tried both.

## P12-P15b: Answerer Quality Fixes (Day 14)

Four answerer-side improvements were implemented and shipped as a single commit:

- **P12: Truncation ordering fix.** The context truncation used a `BTreeMap` that grouped results oldest-first. For Update questions, this meant the latest evidence got truncated away while old stale values were preserved. Fix: reverse ordering for Update strategy so newest facts appear first.

- **P13: A2 strategy routing fix.** Improved detection of Update-type questions and routing to the Update answering strategy with appropriate thresholds.

- **P14: Preference prompt enhancement.** Added guidance for preference/advice questions to extract user-specific context rather than giving generic advice.

- **P15b: Count reducer (observe-only).** A deterministic count validation system that parses the agent's evidence, deduplicates items, and compares the claimed count against the evidence count. Initially set to observe-only mode (logs proposed corrections but does not override) because the confidence was below the enforcement threshold.

### Gate Run: Neutral

| Run | Score |
|-----|-------|
| G-P12-P15b (run 1) | 202/231 |
| G-P12-P15b (run 2) | 203/231 |
| G-P2 baseline | 202/231 |

Two Gate runs produced 202 and 203, against a baseline of 202. The 21 regressions and 20 fixes between runs canceled out -- pure stochastic variance. The changes were technically correct (truncation ordering was genuinely a bug, the reducer's analysis was sound) but did not move the score.

**Root cause**: The aggregation failures were "agent finds wrong items" (search strategy), not "agent miscounts found items" (post-processing). The reducer found "consistent" (claimed count == listed count) in 5/5 parseable cases. The agent was wrong about *which* items it found, not *how many* it counted.

P12-P15b was committed as a cleanup but recognized as evidence of an answerer post-processing ceiling.

## P16: Evidence-Table Finalizer (Day 15)

P16 was the most ambitious post-processing intervention: approximately 750 lines of new code implementing three deterministic "kernels" that intercept the agent's `done(answer)` call and attempt to correct it using structured evidence extracted from tool call history.

### The Three Kernels

1. **COUNT**: Parse evidence rows from tool results, filter by keyword relevance (2+ hits), deduplicate, compare against agent's claimed count. Override on off-by-one undercount (confidence 0.94).

2. **LATEST_VALUE**: Extract concrete values (dollar amounts, time patterns) from dated evidence groups. Detect when agent's answer contains a value from an older date and a newer value exists. Override with newest (confidence 0.93).

3. **DATE_DIFF**: Compare agent's stated number against the `date_diff` tool's computed result. Override if they differ (confidence 0.92).

### Fast Loop: Baseline Noise

| Run | Score | Overrides Fired |
|-----|-------|----------------|
| FL-P16 | 54/60 (90.0%) | 1 (COUNT, harmful) |
| Baseline | 53/60 (88.3%) | -- |

The +1 was stochastic noise. Only one override fired across 60 questions: COUNT changed "3 days" to "4 days" on a faith-activities question. It was **wrong** -- two evidence rows about the same bible study on Dec 17th (different wording) were counted as distinct items.

### Why P16 Failed: Three Independent Reasons

1. **Evidence table rows are not items.** Tool results contain raw conversation messages, not distinct real-world items. Two messages about the same event produce two rows. Counting keyword-matching rows counts *mentions*, not *entities*. Solving this requires NLU-level entity resolution, which is the same problem the LLM agent is already solving.

2. **The kernels have no viable middle ground.** LATEST_VALUE with generic token matching (v1) caused catastrophic false positives ("gallon" matched a tanks-counting question). Restricted to dollar amounts and time patterns (v2), it never fires because actual update failures involve locations and entity names. COUNT is noise-dominated. Either too broad or too narrow.

3. **The agent is already correct when correction would be easy.** DATE_DIFF found that in 100% of cases where the agent called the date_diff tool, it correctly used the result. The failures are upstream (agent called the tool with wrong dates, or did not call it at all). Post-hoc correction cannot fix upstream search errors.

P16 was reverted. This was the **third failed post-processing intervention** (after the deterministic resolver at -2q and P12-P15b at neutral).

## P17: Evidence Array from Agent (Day 15)

P17 asked the agent to include a structured evidence array in its `done()` call: for each piece of supporting evidence, provide the source session ID, date, and excerpt. The idea was that if the agent provided its evidence explicitly, a verifier could check it.

### The Result: Agent Treats Evidence as "A Few Examples"

When asked to list evidence, the agent would claim "based on 99 sessions" but provide only 4 examples. The evidence array was treated as illustrative, not exhaustive. This made verification impossible: a 4-item array for a question expecting 5 items was neither proof of a miss (maybe the agent found more but only listed a few) nor proof of completeness.

Two specific mechanisms caused regressions:

- **MATCH bypass (-4 questions)**: When the evidence count matched the agent's answer, the system set `recount_verified=true`, bypassing the recount and completeness gates. But the recount gate exists precisely to push for more searching on off-by-one cases. Bypassing it allowed under-searched answers through.

- **MISMATCH rejections**: When evidence count did not match, the system burned 36 additional iterations trying to reconcile, producing zero benefit.

P17 was reverted.

### P17-lite: Telemetry Fields (Committed)

A minimal version (P17-lite) was committed: the agent can optionally include `latest_date` and `computed_value` fields in `done()`, logged for analysis but never used for verification or override. This provided useful debugging telemetry without affecting scores.

## P18: Graph Tools Exposed to Agent (Day 16)

With post-processing exhausted, attention turned to the SurrealDB knowledge graph built during P7b (317K entities, 88K relationships, 152K mentions). P18 exposed four graph tools to the agent: `graph_enumerate`, `graph_lookup`, `graph_relationships`, `graph_disambiguate`.

### Two Variants, Both Failed

| Variant | Activation | FL Score | Gate Score | Delta |
|---------|-----------|----------|------------|-------|
| P18 (global) | All questions | 54/60 (+1) | 199/231 | **-5 Gate** |
| P18.1 (scoped) | Enumeration-only | 52/60 | -- | **-2 FL** |

### The Critical Finding: Zero Graph Tool Calls

**The agent made zero graph tool calls in both variants.** All 16 Enumeration-strategy questions had graph tools available. None were used. The regressions were caused entirely by tool schema expansion (8 to 12 tools) changing the model's planning behavior.

An A/B validation confirmed: on 16 regression questions, `GRAPH_RETRIEVAL=0` scored 9/16; `GRAPH_RETRIEVAL=1` scored 8/16 with zero `graph_*` calls. The schema alone caused the regression.

### Why No SOTA System Exposes Graph Tools

| System | Score | Graph? | How Used |
|--------|-------|--------|----------|
| Mastra OM | 94.87% | NO | No retrieval at all |
| Honcho | 92.6% | NO | Vector search only |
| Hindsight | 91.4% | YES | Behind the scenes -- 1 of 4 retrieval channels, RRF-fused |
| Zep/Graphiti | 71.2% | YES | Behind the scenes |

The pattern is clear: no competitive system exposes graph queries as agent tools. Graph data is used as a silent retrieval channel, fused with other signals before the agent sees it.

P18 was reverted. The lesson: **tool schema count is a first-class engineering constraint.** Adding tools changes model behavior even when the tools are never called.

## P20: Behind-the-Scenes Graph Prefetch (Day 17)

Directly inspired by Hindsight's architecture (the only successful graph system), P20 silently augmented prefetch with graph-linked facts. Zero tool schema changes. Zero prompt changes. The agent received richer initial context without knowing it came from a graph.

### Three-Phase Algorithm

1. **Seed extraction**: For each fact from prefetch results, reverse-lookup entities via SurrealDB's `mention` table. Cap at 5 seed entities.
2. **1-hop spreading activation**: For each seed entity, get 1-hop neighbors. Score by link type: seed mention = 1.0, neighbor mention = 0.5, multi-link bonus 1.5x. Deduplicate against existing prefetch.
3. **Fetch and format**: Look up top 6 scored facts from Qdrant, format as date-grouped text appended to prefetch.

Gating: only fires when `category == MultiSession` OR `strategy == Enumeration` (~183/500 questions).

### Fast Loop: Exact Baseline

| Run | Score | P20 Fires | Graph Context Injected |
|-----|-------|-----------|----------------------|
| FL-P20b | 54/60 | 17/20 targeted questions | 253-1773 chars per question |

54/60 = exact baseline. P20 fired correctly on 17/20 targeted questions, injecting meaningful graph-linked context. But it made no difference.

### Root Cause: Graph Facts Overlap with Vector Search

Qdrant recall is 498/500. Vector search already finds the relevant facts. Graph spreading activation discovers the *same* facts via entity-to-mention-to-fact paths. The deduplication step removed most graph-found facts because they were already in prefetch. The surviving 6 additional facts were typically low-relevance peripheral mentions.

Hindsight's graph works because their base retrieval misses items. Our base retrieval does not miss items. Adding a redundant retrieval channel to a near-perfect retrieval system adds nothing.

P20 was committed as infrastructure (the graph prefetch code is clean and correct) but does not move the score.

## P21: Temporal Scatter Search (Day 17 -- Rejected Pre-Implementation)

P21 proposed splitting the user's time range into temporal buckets and running separate `search_facts` calls per bucket for Enumeration questions. Before implementation, a pre-implementation review rejected it:

1. Only 8/13 aggregation failures route to Enumeration strategy
2. 6/13 have all relevant items in a single session -- temporal bucketing would be useless
3. `search_facts` has no date-range parameter
4. P20's lesson: at 498/500 recall, scatter search will mostly rediscover already-found facts
5. Realistic impact: +0 to +1, with -1 to -3 downside risk

P21 was not implemented. This was the first proposal rejected before any code was written, saving approximately $13-49 in benchmark costs.

## The Wall

After P12 through P21, the elimination is comprehensive:

| Approach | Proposals | Result |
|----------|-----------|--------|
| Answerer post-processing | P12-P15b, P16 | Neutral or harmful |
| Deterministic evidence correction | P16 (3 kernels) | 1 override fired (harmful) |
| Agent-provided evidence arrays | P17 | Agent gives "a few examples," not exhaustive list |
| Graph tools exposed to agent | P18, P18.1 | Zero graph calls, schema bloat regresses |
| Behind-the-scenes graph retrieval | P20 | Graph facts overlap with vector search |
| Temporal scatter search | P21 | Rejected pre-implementation |
| Deterministic resolver | (Phase 3) | Net harmful (-2q) |
| Query expansion | (Phase 4, Steps 12-13) | Catastrophic (-4) or negative (-2) |
| RRF k tuning | (Phase 4, Steps 11, 14) | Negative (-2 to -3) |

Every retrieval-side intervention is blocked by 498/500 recall. Every post-processing intervention is blocked by the fact that failures are semantic reasoning errors, not arithmetic or formatting errors. Every graph intervention is blocked by vector search already finding the same facts.

## What the 58 Failures Actually Need

The forensics report classifies the remaining failures as:

- **Agent finds N-1 of N items then stops searching** (aggregation: 13 failures)
- **Agent picks the wrong temporal anchor** (temporal: 17 failures)
- **Agent conflates similar entities** (abstention false positives: 5 failures)
- **Agent doesn't iterate enough on Update questions** (stale values: 5 failures)
- **Data scattered across sessions with no linking signal** (cross-session: 5 failures)

These are reasoning quality problems. The data is in Qdrant. The agent retrieves it. The agent reasons incorrectly about it.

## Where 90%+ Might Come From

The SOTA leaderboard provides clues:

| System | Score | Architecture |
|--------|-------|-------------|
| Mastra OM (gpt-5-mini) | 94.87% | Observation log compression, no retrieval |
| Honcho (gemini-3-pro) | 92.6% | Reasoning trees + agentic vector search |
| Hindsight (gemini-3) | 91.4% | 4-channel retrieval + cross-encoder reranking |
| **Engram (gpt-4o)** | **88.4%** | Vector search + agentic tools + gates |
| Emergence | 86.0% | Unknown |

The gap between us and the top is not data or retrieval -- it is reasoning quality. Three paths remain theoretically viable:

1. **Model upgrade**: gpt-5 or equivalent may reason better over the same evidence
2. **Mastra-style observation logs**: Background LLM agents compress conversations into structured event logs that fit entirely in the system prompt -- eliminating retrieval entirely
3. **Fundamental architecture change**: Move from "retrieve then reason" to "compress then present"

Each of these represents a qualitative shift in approach, not an incremental optimization. The lesson of Phase 5 is that incremental optimization has reached its ceiling at 88.4%.

## Phase 5 Summary

| Proposal | Type | Result | Cost |
|----------|------|--------|------|
| P12-P15b | Answerer quality | Neutral (Gate: 202-203/231) | ~$100 |
| P16 | Evidence-table finalizer | Neutral, 1 harmful override | ~$14 |
| P17 | Agent evidence array | Harmful, reverted | ~$50 |
| P17-lite | Telemetry fields | Neutral, committed | $0 |
| P18 | Graph tools (global) | -5 Gate, reverted | ~$62 |
| P18.1 | Graph tools (scoped) | -2 FL, reverted | ~$13 |
| P20 | Graph prefetch (silent) | Neutral, committed | ~$13 |
| P21 | Temporal scatter | Rejected pre-implementation | $0 |

### Key Lessons

1. **At 88.4%, the bottleneck is agent reasoning quality, not data or retrieval.** Recall is 498/500. Every "retrieve more" or "retrieve differently" intervention is redundant.

2. **Deterministic post-processing of LLM answers is a dead end.** Three independent attempts (resolver, P12-P15b, P16) all failed. Tool result text is natural language, not structured data. You cannot reliably extract "items" or "values" from it with string processing.

3. **Tool schema count changes model behavior.** Even unused tools alter the model's planning policy. This is not a prompt engineering problem -- it is a fundamental property of LLM function calling.

4. **Graph data is valuable but must be used silently.** No SOTA system exposes graph tools to agents. The only successful graph integration (Hindsight) uses graph as one of four retrieval channels, fused before the agent sees results.

5. **High recall makes additional retrieval channels redundant.** Graph prefetch, temporal scatter, query expansion -- all discover facts that vector search already found. The marginal value of a second retrieval channel approaches zero as the first channel approaches perfect recall.

6. **Reasoning quality correlates with model capability, not system complexity.** Mastra OM at 94.87% uses no retrieval, no graph, no vectors -- just observation log compression in the system prompt. Graph-heavy systems (Zep 71.2%, SGMem 73.0%) dramatically underperform. Simpler architectures with better reasoning models win.
