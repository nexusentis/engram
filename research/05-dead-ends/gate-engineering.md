---
title: "Gate & Prompt Engineering Pitfalls"
sidebar_position: 3
---

# Gate & Prompt Engineering Pitfalls

Gates are programmatic checks that run during the agent's reasoning loop. They inspect intermediate outputs and inject corrective instructions -- "you haven't searched enough," "your count doesn't match," "verify your ordering." Prompt engineering is the related practice of tuning the agent's system prompt and strategy-specific instructions to shape its behavior.

Both approaches delivered large early wins. P0 (the agentic answerer itself) added +14 percentage points. P5 (tool correctness fixes) added +8pp. But after those foundational gains, further gate and prompt engineering produced diminishing and then negative returns. This page documents the interventions that went wrong and the patterns that explain why.

## P1: Temporal Solver v2 (Day 13) -- NET HARMFUL (-11)

P1 was the most expensive single failure in the project. It added three mechanisms targeting temporal questions:

### Mechanism A: Ordering prompt (approximately 60% of temporal regression)

An `is_ordering_question()` function detected ordering-type temporal questions using patterns like `(which|who) + first` and `" or " + first/before/after`. When triggered, it injected a strict ordering prompt: "NEVER guess ordering from vague language. Use exact dates."

The function fired on 24 of 61 temporal questions -- nearly 40%. Of those 24, 19 were already passing. The strict "NEVER guess" instruction converted correct contextual reasoning into abstentions. The agent had been correctly using phrases like "the session mentions trying Italian food after the road trip" to infer ordering. The new prompt told it that was forbidden.

The pattern `(which|who) + first` was far too broad. It matched "which was the first BBQ in June?" (not a comparison between items -- just asking for a date) and "who first suggested the restaurant?" (a recall question, not ordering). A feature targeting approximately 5 temporal ordering failures affected 24 questions, 19 of which were passing.

### Mechanism B: Ordering evidence gate (approximately 25%)

An evidence gate used `extract_comparison_slots()` to produce slot phrases from the question, then checked whether tool results contained those exact phrases via `contains()`. The slot for "when did the narrator lose their phone charger" was the literal string "the narrator losing their phone charger." Tool results said "lost my charger." The verbatim match failed. The gate injected "ORDERING CHECK: Missing evidence for items," wasting iterations and pushing toward abstention.

Worse, this gate ran after the A3 evidence gate on the same `done()` call. Both gates could fire sequentially on a single answer attempt: A3 rejects because of insufficient evidence, then the ordering gate rejects because of missing slot matches. The agent received two rejection signals for one answer, virtually guaranteeing abstention.

### Mechanism C: TemporalParser cascade (approximately 15%)

Four new TemporalParser patterns (`past_weekend`, `text_number_ago`, `month_only`, `weekday_months_ago`) expanded temporal detection. All new patterns forced `TemporalIntent::PointInTime`, which at line 101 overrode any existing intent (including Ordering). PointInTime added a `t_valid` date range filter to Qdrant queries, which excluded relevant facts that fell outside the guessed date window.

The `month_only` pattern ("in January") guessed the year heuristically -- often wrong for LongMemEval's synthetic data spanning multiple years. The `text_number_ago` pattern ("two weeks ago") approximated months as 30 days. The `weekday_months_ago` pattern returned entire-month windows that were too coarse for precise retrieval.

For Update questions, PointInTime overrode CurrentState intent, losing the `is_latest` filter that correctly retrieves the most recent value. This directly caused 2 Update regressions.

### The isolation test

Because P1 was tested on different ingestion data than the baseline (I-p7a vs I-v10), we initially could not separate code impact from ingestion variance. A follow-up run (R-G-P1b) tested P1 code on the original I-v10 data:

| Category | Baseline (no P1, I-v10) | P1 code (I-v10) | Delta |
|---|---|---|---|
| Temporal | 49/57 (86.0%) | 39/57 (68.4%) | **-10** |
| Updates | 31/31 (100%) | 29/31 (93.5%) | **-2** |
| Abstention | 14/15 (93.3%) | 13/15 (86.7%) | **-1** |
| MultiSession | 49/62 (79.0%) | 50/62 (80.6%) | +1 |
| Extraction | 59/66 (89.4%) | 60/66 (90.9%) | +1 |
| **Total** | **202/231 (87.4%)** | **191/231 (82.7%)** | **-11** |

P1 was net harmful by 11 questions on identical data. It was selectively reverted (P1R), keeping only the 5xx retry with exponential backoff and the UTF-8 safe truncation fix.

**Cost**: Approximately $62 for the Gate run, plus $13 for the isolation test, plus $0 for code changes = ~$75 total for -11q.

## The "NEVER" Rule

P1 taught the most important prompt engineering lesson of the project: **prohibitive language in agent prompts kills correct contextual reasoning.**

The ordering prompt said "NEVER guess ordering from vague language. Use exact dates." The agent at 88% accuracy was already correctly inferring ordering from context like "after visiting the museum, I went to the park." The "NEVER" instruction did not add precision -- it removed the agent's ability to reason contextually. Correct answers became abstentions.

This pattern repeated across multiple experiments. Advisory language ("prefer exact dates when available") is safe. Prohibitive language ("NEVER guess," "you MUST use exact dates") is harmful. The distinction matters because the agent at 88% is already good at the task. It uses contextual cues correctly far more often than it uses them incorrectly. Prohibiting contextual reasoning to prevent the rare incorrect use also prevents the much more common correct use.

**Rule**: Never add "NEVER" to agent prompts. Use advisory language, not prohibitive language.

## Gate Compounding

Gates fire sequentially on each `done()` call. Each gate independently decides whether to accept or reject the agent's answer. When multiple gates fire on the same answer, they can compound in unexpected ways.

In P1, the A3 evidence gate and the ordering gate both fired on the same `done()` call. A3 checked whether the agent had retrieved enough evidence. The ordering gate checked whether comparison slots appeared in tool results. Both could reject simultaneously, producing two rejection messages in the same turn. The agent interpreted this as strong evidence that its answer was wrong and switched to abstention.

Even without P1, gate compounding is a persistent risk. The system has gates for:

- Evidence sufficiency (A3)
- Retrieval call count (minimum searches before answering)
- Count consistency (claimed count vs. listed items)
- Date_diff verification (temporal arithmetic check)
- Anti-abstention (second-chance keyword search)

Each gate was tested in isolation. Their interaction was not. Adding a new gate without testing all possible combinations with existing gates is a recipe for compounding failures.

**Rule**: Before adding any gate, list all existing gates and trace what happens when they fire sequentially on the same `done()` call.

## Query Expansion (P2-1 through P2-4b, Day 12) -- All Reverted

Query expansion generates multiple reformulations of the user's question and retrieves results for each, then fuses them via Reciprocal Rank Fusion (RRF). The theory: different phrasings catch different relevant documents, improving recall.

| Run | What | Score | Delta |
|---|---|---|---|
| P2-1 | Keyword-OR fulltext + RRF k=40 | ~53/60 | -1 |
| P2-2 | 3-variant query expansion (gpt-4o-mini) | 50/60 | **-4** |
| P2-3 | 1-variant expansion + protected tail fusion | 52/60 | -2 |
| P2-4a | RRF k=40 (isolated) | ~51/60 | -3 |
| P2-4b | RRF k=50 (isolated) | ~52/60 | -2 |

The 3-variant expansion (P2-2) was catastrophic at -4. Each variant retrieved a somewhat different set of results, and RRF fusion diluted the precision of the original query's results with less relevant results from the variants. The net effect was that top-ranked results became noisier.

RRF k parameter tuning (P2-4a, P2-4b) was also harmful. The k parameter controls how much rank matters in score calculation. Neither k=40 nor k=50 improved over the default.

The only surviving win from the P2 series was P2-5b: a 3-line bug fix to the A3 evidence gate that was falsely triggering on Enumeration and Preference questions containing "or" in the question text. This had nothing to do with query expansion.

**Lesson**: Query expansion improves recall at the cost of precision. When recall is already high (498/500), the precision cost dominates. At high recall, you want the single best query, not many mediocre queries.

## LLM Reranking (Runs 6a-6b, Day 2) -- Catastrophic

LLM reranking used gpt-4o-mini to score each retrieved result for relevance to the question, then re-ordered results by LLM score.

| Run | What | Score |
|---|---|---|
| 5 | Baseline (date-grouped, no reranking) | 39/50 (78%) |
| 6a | LLM reranking + MMR diversity | 31/50 (62%) |
| 6b | LLM reranking only | 32/50 (64%) |

A 16 percentage point regression. The cause was truncation: each result was truncated to 200 characters for the reranking prompt, losing critical context that full results contained. The LLM made worse relevance judgments on truncated snippets than the original vector similarity scores.

Adding MMR (Maximal Marginal Relevance) diversity filtering on top of reranking made things worse still (31/50), because it removed near-duplicate results that were actually relevant -- different messages about the same topic from different sessions.

**Lesson**: LLM reranking with truncated snippets is worse than no reranking. And removing near-duplicates via diversity filtering removes relevant evidence when multiple sessions discuss the same topic.

## Other Failed Gate Interventions

### B2/B3: Supersession and consolidation (Day 11)

Supersession marks old facts as stale when newer contradicting facts exist (e.g., "moved from NYC" superseded by "moved to SF"). Consolidation merges related facts. Both were neutral -- they helped only when the ingested data was already clean, and our I-v11 data was clean.

### I-v12: Dual-seed ingestion (Day 14)

Ran extraction with two different random seeds and merged the results, producing ~301K facts (vs. ~283K from single-seed). The theory: two extraction passes catch different facts, improving coverage.

Result: -6q (436/500 vs. 442/500 baseline). Doubled facts diluted the context window. For aggregation questions, the agent now found duplicate mentions of the same item, inflating counts. The additional facts from the second seed were mostly paraphrases of facts from the first seed, adding noise without adding information.

## P31: Enumeration Uncertainty Fallback (Day 21) -- NEUTRAL

P31 extended the ensemble router's `should_fallback()` logic to trigger on high-iteration Enumeration questions. When Gemini used >= 8 iterations on an Enumeration question and produced a non-abstention answer, the system routed to GPT-4o as fallback.

### What it tried

The hypothesis was that high iteration count on Enumeration signals low confidence --- the agent is searching extensively but not finding enough items. Since GPT-4o has different search patterns, it might find the missing items. Config: `fallback_on_enum_uncertainty = true`, `enum_uncertainty_min_iterations = 8` in `ensemble.toml`.

### Why it failed

P31 fired on 3 Fast Loop questions (`e56a43b9`, `f9e8c073`, `8fb83627`). GPT-4o gave the same wrong answers as Gemini on all three. The root cause: these are **shared failures** where both models fail for the same structural reason --- cumulative context overload (294K chars across 35 tool calls) and semantic errors in multi-session item identification. The models are not finding different wrong items; they are both overwhelmed by the same volume of context and making the same counting mistakes.

### Evidence

FL result: 56/60 (93.3%) = exact baseline. Zero questions flipped. Cost: ~$8.

### Lesson

Ensemble routing is only useful for **complementary** failure modes --- where one model fails and the other succeeds. The P22 ensemble succeeded (+15 questions) because Gemini and GPT-4o had complementary failures: 40 questions where they disagreed. But the remaining MultiSession failures are shared: both models get the same wrong count for the same structural reason. No amount of routing between models can fix a failure that both models share. The path forward requires changing what the models see (context compression, provenance), not which model answers.

## The Meta-Lesson

At 88%+ accuracy, the agent is mostly right. It uses contextual reasoning, makes appropriate tool calls, and constructs correct answers for 442 of 500 questions. The 58 failures are hard cases -- entity conflation, temporal anchoring, cross-session aggregation with scattered items.

Interventions that constrain agent behavior (strict prompts, gates, post-processing overrides) affect the 442 passing questions at least as much as the 58 failing ones. The math is unforgiving: helping 3 failures while regressing 4 passes is a net loss. And the interventions that seem most targeted (ordering prompt affecting "only" ordering questions) turn out to fire far more broadly than expected (24/61 temporal questions, 19 of which were passing).

The calibration data tells the story:

| Intervention type | Effectiveness rate |
|---|---|
| Tool correctness fixes (P0, P5) | ~14pp lift |
| Judge criteria changes | ~70% hit rate |
| Gate/prompt fixes | ~18-23% of estimated impact |
| Deterministic post-processing | 0% hit rate (3 attempts, 3 failures) |
| Query expansion / RRF tuning | Negative |
| LLM reranking | Catastrophically negative |

The only interventions that delivered lasting value at high accuracy were foundational correctness fixes (P0: building the agentic loop, P5: fixing tool bugs) and judge calibration. Everything else -- every clever gate, every sophisticated prompt, every deterministic override -- was neutral or harmful.
