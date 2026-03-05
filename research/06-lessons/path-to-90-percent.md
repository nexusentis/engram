---
title: "Path to 90%+ (Historical)"
sidebar_position: 3
---

# Path to 90%+ (Historical)

:::info Historical Document
This roadmap was written at **442/500 (88.4%)** during Phase 5, when Engram was 4th globally. It correctly identified model upgrade and Mastra-style compression as viable paths, but underestimated the impact of model diversity.

**What actually happened**: A model swap to Gemini 3.1 Pro broke through to 452/500 (Phase 6), an ensemble router reached 467/500 (Phase 7), quick wins pushed to 472/500 (Phase 9), and replacing GPT-4o with GPT-5.2 as fallback reached **479/500 (95.8%)** (Phase 11) — **#1 globally**, surpassing Mastra OM.

For the current roadmap, see **[Path to 96%+: Multi-Model Roadmap](./path-to-95-percent)**.
:::

## Where We Were

Engram stood at 442/500 (88.4%) on LongMemEval-S, placing 4th globally. The gap to the top three:

| System | Score | Gap from Engram | Architecture |
|--------|-------|----------------|-------------|
| Mastra OM | 94.87% | -33 questions | Observation logs, no retrieval |
| Honcho | 92.6% | -21 questions | Reasoning trees, agentic loop |
| Hindsight | 91.4% | -15 questions | Entity graph + 4-way retrieval |

An independent assessment concluded that **90-93% is realistic** with the current Qdrant + gpt-4o architecture, provided reasoning and control flow improve. *This turned out to be correct — but the path was a model swap and ensemble, not the interventions listed below.*

## Exhausted Approaches

The following intervention categories have been tried multiple times and are empirically dead ends at 88%+. They should not be retried without a fundamentally new mechanism.

| Category | Attempts | Best Result | Why It Failed |
|----------|----------|-------------|---------------|
| Prompt/gate engineering | P1, P11, P12-P15b | P11: +22q (one-time gate fix); rest neutral or harmful | Gates compound unpredictably; "NEVER" language kills reasoning |
| Query expansion | 3-variant, 1-variant, keyword-OR | All -2 to -4 questions | Rank signal flattening, context dilution |
| RRF k tuning | k=40, k=50 | -2 to -3 questions | Over-weighting top results hurts category balance |
| LLM reranking | gpt-4o-mini per-passage scoring | -14pp | Truncated snippets produce worse judgments |
| Dual-seed ingestion | I-v12 | -6 questions (436/500) | Doubled facts dilute context, especially for aggregation |
| Deterministic post-processing | Resolver, P12-P15b, P16 | All neutral or harmful | Evidence rows are not items; agent already correct when correction easy |
| Graph tools for agent | P18 (global), P18.1 (scoped) | -5 on Gate, -2 on FL | Schema bloat changes model behavior; 0 graph tool calls |
| Behind-the-scenes graph prefetch | P20 | Neutral (54/60 = baseline) | Graph facts overlap entirely with vector search results |
| Temporal scatter enumeration | P21 (rejected pre-implementation) | N/A | Wrong failure mode; `search_facts` lacks date-range parameter |

## Remaining Viable Paths (with Outcomes)

### 1. Mastra-Style Observation-Log Compression — *Evolved into Hybrid OM proposal*

The approach used by the #1 system. Two background LLM agents (Observer and Reflector) process conversations into structured event logs with a three-date temporal model (event date, observed date, confidence decay). At query time, the compressed log goes directly into the context window --- no retrieval step at all.

**Why it might work for us:** LongMemEval-S rewards "see all relevant sessions" over sparse semantic retrieval. Observation logs preserve temporal relationships that retrieval fragments. Our temporal category (86.6%) is the second-weakest and the one most likely to benefit from a context-first approach.

**Why it is hard:** Our 24,000 sessions need approximately 100x compression to fit in a context window. At that compression ratio, detail is lost. A hybrid approach --- per-user observation summaries for recent/active topics, with Qdrant retrieval for long-tail queries --- would preserve the strengths of both architectures.

**Estimated effort:** 2-3 weeks implementation + ~$8 ingestion re-run.

### 2. Model Upgrade — *This worked. +10q from Gemini 3.1 Pro (Phase 6)*

The answering model (gpt-4o) was strong but not the frontier for reasoning. A model upgrade to gpt-5 or an equivalent with improved multi-step reasoning could improve performance on the 17 temporal failures and 13 aggregation failures without any architectural changes.

**Evidence:** Our empirical finding that reasoning quality is the bottleneck directly implies that a better reasoner would improve the score. The agent already retrieves the right evidence 99.6% of the time; it just needs to reason about it more accurately.

**Risk:** Model upgrades can regress on categories that the current model handles well. Testing must cover all 500 questions, not just the failing subset.

**Estimated effort:** Low (model swap + testing), but depends on model availability and cost.

### 3. Fundamental Architecture Change — *Not attempted; ensemble + model upgrades reached 95.8% first*

Moving away from RAG entirely toward a context-window-first approach, as Mastra OM demonstrates. This would involve:

- Replacing Qdrant retrieval with pre-computed, compressed conversation summaries stored per user
- Using the full context window to present relevant conversation history
- Trading retrieval precision for context completeness

This is the most radical option and the most likely to close the gap to 95%. It is also the most expensive to implement and validate, as it requires rebuilding the ingestion and answering pipelines.

### 4. Better Agent Search Strategy — *Partially addressed by P-NEW-C routing fix*

The only reasoning-side intervention that has not been fully explored. The 13 aggregation failures are "agent finds wrong/incomplete items" --- a search strategy problem, not a post-processing problem. Specific opportunities:

- **Retrieval continuation policy:** When the agent finds partial results, it currently stops after a fixed number of iterations. A more adaptive policy that continues searching when evidence is incomplete could recover 3-5 aggregation failures.
- **Temporal normalization in agent reasoning:** The agent struggles with relative time references ("last Tuesday", "a few months ago") that refer to different absolute dates across sessions. Explicit temporal grounding before comparison could help 3-4 temporal failures.
- **Structured evidence synthesis:** Asking the agent to build and maintain an explicit evidence table during its search loop (different from P16's post-hoc evidence table) could improve counting accuracy.

**Estimated impact:** +3-6 questions (already discounted 3x from raw estimates of +10-18).

## Estimated Ceilings (with Actual Results)

| Intervention | Predicted | Actual | Notes |
|-------------|-----------|--------|-------|
| Baseline (no changes) | 442/500 (88.4%) | — | — |
| Better agent search strategy | ~445-448 | Partially addressed | P-NEW-C routing fix: +1 |
| + Model upgrade | ~448-455 | **452/500 (90.4%)** | Gemini 3.1 Pro (Phase 6) |
| + Model diversity / ensemble | Not predicted | **467/500 (93.4%)** | P22 ensemble router (Phase 7) |
| + Quick wins (P25, judge, routing) | Not predicted | **472/500 (94.4%)** | Phase 9 |
| + GPT-5.2 fallback (replacing GPT-4o) | Not predicted | **479/500 (95.8%)** | Phase 11 |

The prediction that "90-93% is realistic" was correct, but the path was model diversity (not predicted in this roadmap), not the architectural changes proposed above. The ceiling estimate of "reaching 95% almost certainly requires a fundamental architecture change" was **wrong** — we reached 95.8% without one, through better model selection alone.

## The Core Insight (Validated)

The SOTA leaderboard tells a clear story: **reasoning quality dominates data architecture.** Mastra OM (94.87%) uses no retrieval. Honcho (92.6%) uses no graph. The systems that invested most heavily in data infrastructure (Zep at 71.2%, SGMem at 73.0%, Mem0 at 68.4%) perform worst.

This insight proved correct. The gap from 88% to 95.8% was closed by model diversity (swapping gpt-4o for Gemini 3.1 Pro, then exploiting complementary failure modes via ensemble routing, then upgrading the fallback to GPT-5.2) and targeted logic fixes (P25 abstention override, judge calibration, routing fixes) — not by improving retrieval, storage, or graph infrastructure. Engram reached #1 globally at 95.8% without any fundamental architecture change. See [Phase 11](../journey/phase-11-inverted-ensemble) for details.
