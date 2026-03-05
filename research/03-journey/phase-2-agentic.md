---
title: "Phase 2: The Agentic Breakthrough"
sidebar_position: 2
description: "Tool correctness fixes unlock agentic answering: 70% to 88% on 50q (Week 1)"
---

# Phase 2: The Agentic Breakthrough (Week 1)

Phase 2 is the story of a single diagnostic report that changed the trajectory of the project. A systematic code audit identified six tool correctness bugs. Fixing them transformed agentic answering from consistently worse than non-agentic (-4pp average) to consistently better (+14pp on the same data). Tool correctness was worth more than all prompt engineering combined.

## Runs #9-16: The 9-Phase Plan and Ablation Testing (Days 4-5)

Before the diagnostic breakthrough, a series of experiments tried to improve the score through architectural changes: a 9-phase answering plan, ablation testing, judge fixes, and loop detection. The results were noisy and inconclusive.

| Run | Mode | Key Change | Total |
|-----|------|-----------|-------|
| #9a | non-agentic | Baseline (all P5 off), clean ingestion (temp=0.0, seed=42) | **33/50 (66%)** |
| #9b | non-agentic | NDCG reranking only | **31/50 (62%)** |
| #9c | non-agentic | All 4 P5 improvements | **32/50 (64%)** |
| #10 | **agentic** | Full 9-phase: strategy, 12K limit, 20 iter, rel dates | **35/50 (70%)** |
| #11 | **agentic** | Same but P5 OFF | **32/50 (64%)** |
| #12 | non-agentic | Non-agentic on same temp=0.0 data | **32/50 (64%)** |
| #13 | **agentic** | + Loop detection (dupe skip + cost breaker) | **34/50 (68%)** |
| #14 | non-agentic | Non-agentic on same temp=0.1 data | **35/50 (70%)** |
| #15 | non-agentic | + Judge exact-match + abstention prompt fix | **37/50 (74%)** |
| #16 | **agentic** | Agentic with same fixes | **34/50 (68%)** |

Note that Runs #9-16 did not have the numeric answer parsing fix, so their absolute scores are depressed. The pattern, however, was clear: agentic and non-agentic traded wins run-to-run, with non-agentic slightly ahead.

## Run #17: Numeric Answer Fix (Day 5)

A critical bug was discovered: the benchmark harness was dropping numeric gold answers (`3`, `120`, `15`) to empty strings during parsing. This meant questions with numeric answers were being scored incorrectly -- correct answers appeared as false positives.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| **#17** | non-agentic | **+ Numeric answer fix** | 9/12 | 3/3 | **7/7** | 10/13 | 12/15 | **41/50 (82%)** |

With the parsing fix, non-agentic jumped to 82% -- the highest non-agentic score ever achieved. Updates hit 100% (7/7) because many Update questions have numeric answers that were previously being mangled. This run, on #13 ingestion data, would remain the non-agentic high-water mark.

## Runs #18-19: User-Scoped Messages (Day 6)

A structural fix added user_id scoping to message retrieval, preventing cross-user contamination in the message search path.

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #18 | **agentic** | + user_id scoping on messages, fresh ingestion | 8/12 | 3/3 | 6/7 | 5/13 | 13/15 | **35/50 (70%)** |
| #19 | non-agentic | Non-agentic on same data | 7/12 | 3/3 | 7/7 | 8/13 | 13/15 | **38/50 (76%)** |

These runs used fresh ingestion data, making them not directly comparable to #17. Agentic was still at 70%, non-agentic at 76%. The 82% to 76% drop in non-agentic was later attributed primarily to ingestion variance (different data) rather than code changes.

Run #18 is important as the data baseline for the breakthrough that follows. All subsequent runs through #31 reused this data, enabling fair comparisons.

## Runs #20-21: The 6-Fix Correctness Patch -- THE Turning Point (Day 6)

On Day 6, a comprehensive code audit identified six tool and retrieval correctness bugs. All six were query-time fixes requiring no re-ingestion:

1. **user_id filter on search_facts**: Facts were not filtered by user, allowing cross-user contamination
2. **search_entity collection fix**: Tool queried non-existent `"memories"` collection instead of the four real fact collections (world, experience, opinion, observation)
3. **Message hybrid RRF fusion**: Replaced append-then-truncate with proper Reciprocal Rank Fusion for message retrieval
4. **Temperature config**: Wired `temperature` configuration through LlmClient (was hardcoded and ignored)
5. **datetime_range filter**: Replaced numeric range on `t_valid` with proper datetime filtering in temporal retrieval
6. **Category-specific strategy hints**: Added strategy hints to non-agentic prompt based on question category

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #20 | non-agentic | + 6 correctness fixes | 7/12 | 3/3 | 7/7 | 10/13 | 13/15 | **40/50 (80%)** |
| **#21** | **agentic** | + 6 correctness fixes | **11/12** | 3/3 | **7/7** | 8/13 | 13/15 | **42/50 (84%)** |

**Agentic went from 70% to 84% on the same data -- a 14 percentage point jump.** For the first time, agentic definitively beat non-agentic (84% vs 80%). The category breakdown tells the story:

| Category | Pre-fix Agentic (#18) | Post-fix Agentic (#21) | Delta |
|----------|----------------------|----------------------|-------|
| MultiSession | 8/12 | **11/12** | **+3** |
| Temporal | 5/13 | 8/13 | **+3** |
| Updates | 6/7 | **7/7** | **+1** |
| Extraction | 13/15 | 13/15 | 0 |
| Abstention | 3/3 | 3/3 | 0 |
| **Total** | **35/50 (70%)** | **42/50 (84%)** | **+7 (+14pp)** |

MultiSession jumped from 8/12 to 11/12 (92%). The user_id scoping and collection fixes meant the agent was finally searching in the right places. The cost per question dropped dramatically -- Q42, which previously cost $1.00 over 20 loop iterations, now resolved in 4 iterations for $0.14.

**This was the single most impactful change in the entire project.** The +14pp from fixing tool correctness dwarfed every prompt engineering change before or after it. The lesson was unambiguous: fix your tools before tuning your prompts.

## Run #22-23: Temporal Fixes B-F -- Agentic Hits 88% (Day 6)

With the correctness bugs fixed, temporal reasoning became the clear optimization target. Agentic Temporal was at 8/13 (62%) versus non-agentic's 10/13 (77%) -- the agent had the right tools now but wasn't using them optimally for time-related questions.

Five targeted temporal fixes were implemented:

- **Fix B**: Integrated `query_analyzer` into the agentic path (previously only used in non-agentic)
- **Fix C**: Temporal done-gate -- prevent premature answers before temporal tools have been used
- **Fix D**: Holiday date parsing (Christmas, Thanksgiving, etc.) in the temporal parser
- **Fix E**: Richer temporal output with turn order preservation
- **Fix F**: Deterministic `date_diff` tool for computing days/weeks/months between dates

| Run | Mode | Key Change | Multi | Abst | Upd | Temp | Extr | **Total** |
|-----|------|-----------|-------|------|-----|------|------|-----------|
| #22 | **agentic** | + Temporal strategy detection fix | **11/12** | 3/3 | 7/7 | **9/13** | 13/15 | **43/50 (86%)** |
| **#23** | **agentic** | + Temporal fixes B-F | 10/12 | 3/3 | 7/7 | **12/13** | 12/15 | **44/50 (88%)** |

Temporal jumped from 8/13 to **12/13 (92%)**. The done-gate (Fix C) was the most impactful single temporal fix -- it prevented the agent from committing to an answer before calling the date_diff tool. The deterministic date_diff tool (Fix F) eliminated arithmetic errors that occurred when the LLM tried to compute time differences mentally.

Only Q7 still failed in Temporal, and investigation suggested this was a benchmark label issue rather than a system error.

## Runs #24-31: Diagnostic Iteration and Peak (Day 6)

The remainder of Day 6 was spent on targeted diagnostic iteration: analyzing each remaining failure, implementing a fix, re-running, and repeating. This iterative process pushed the 50q score to its peak.

| Run | Key Changes | Total | Notable |
|-----|------------|-------|---------|
| #24 | Verification of #23 | **44/50 (88%)** | Confirmed reproducibility |
| #25 | 6 diagnostic fixes | **42/50 (84%)** | Regressed -- over-broad temporal pattern matching |
| **#26** | Strategy detection refinement | **45/50 (90%)** | Multi 12/12, Extr 14/15 |
| #27 | date_diff years fix, search-before-compute, Update detection | **43/50 (86%)** | 3 targeted fixes worked, 5 stochastic regressions |
| #28 | Stagnation breaker, computation routing | **44/50 (88%)** | Q18 fixed via stagnation breaker |
| #29 | Yes/No exclusion, gate=2 (wrong) | **44/50 (88%)** | Gate=2 was net harmful |
| **#30** | Combined best: Yes/No + gate=3 | **45/50 (90%)** | 3 categories at 100% |
| **#31** | Verification-focused enumeration | **46/50 (92%)** | **50q all-time peak** |

Run #26 achieved perfect MultiSession (12/12, 100%) for the first time, through restricted temporal override and an enumeration done-gate. Run #30 had three categories at 100% simultaneously (Extraction, Updates, Abstention).

Run #31 introduced verification-focused enumeration guidance from an independent code review: a 3-phase approach (Broad Recall, Targeted Expansion, Verification) with evidence-based counting ("build an evidence table: for each candidate item, list item_name, session_id, exact quote"). This pushed the 50q score to **46/50 (92%)** -- the all-time peak on the validation set.

At 92% on 50 questions, the natural next step was the full 500-question benchmark. As [Phase 3](./phase-3-full-benchmark) would reveal, the 50q validation had been telling us an overly optimistic story.

## Phase 2 Summary

| Milestone | Score | Key Insight |
|-----------|-------|-------------|
| Pre-fix agentic | 70% | Tool bugs masked agentic potential |
| Post 6-fix patch | **84%** | +14pp from correctness alone |
| + Temporal fixes B-F | **88%** | Temporal done-gate + date_diff tool |
| + Diagnostic iteration | **92%** | Verification-focused enumeration |

### The Impact Hierarchy

Phase 2 established a clear hierarchy of what moves benchmark scores:

| Intervention Type | Best Example | Impact |
|-------------------|-------------|--------|
| Tool correctness fixes | 6-fix patch (Run #21) | **+14pp** |
| Data presentation | Date-grouped format (Phase 1, Run #5) | +4pp |
| Targeted gates | Temporal done-gate (Run #23) | +3-4pp |
| Prompt engineering | Enumeration guidance (Run #31) | +2pp |
| Strategy detection | Temporal/Enumeration routing (Run #26) | +1-2pp |

### Key Lessons

1. **Tool correctness is worth more than all prompt engineering combined.** The 6-fix patch (+14pp) exceeded the cumulative impact of every prompt and gate change. If your agent has access to broken tools, no amount of prompt tuning will compensate.

2. **Agentic can beat non-agentic -- once the tools work.** The Phase 1 conclusion that "agentic is worse" was wrong. It was the tools that were worse. After fixes, agentic consistently outperformed non-agentic by 2-4pp.

3. **Gates prevent bad answers more reliably than prompts encourage good ones.** The temporal done-gate (preventing premature answers) was more impactful than any prompt guidance about how to reason temporally.

4. **Deterministic tools outperform LLM mental math.** The date_diff tool eliminated temporal arithmetic errors entirely. When a computation has a correct answer, give the agent a calculator rather than asking it to reason.

5. **50q validation can be dangerously optimistic.** The 92% peak on 50 questions turned out to overestimate the full 500q score by approximately 8 percentage points -- a gap that [Phase 3](./phase-3-full-benchmark) would make painfully clear.
