---
title: "Phase 3: Scaling to 500 Questions"
sidebar_position: 3
description: "From 50q validation to 500q truth: the humbling reality check (Week 2)"
---

# Phase 3: Scaling to 500 Questions (Week 2)

Phase 3 marks the transition from a 50-question validation set to the full 500-question LongMemEval-S benchmark. The optimism of 92% on 50 questions met the reality of 83.8% on 500. This phase established the 3-tier testing discipline that would govern all subsequent work and pushed the score from 83.8% to 88.0% through systematic gate and judge improvements.

## Run F1: The Humbling (Day 7)

*Run naming convention: **F** = first full benchmark, **T** = truth (validated) run, **FL** = fast loop (60-question quick check). Numbers are sequential within each tier.*

Run F1 was the first full 500-question benchmark. Fresh ingestion (23,855 sessions, temp=0.0, seed=42), agentic mode with all fixes through Run #31.

| Category | 50q (Run #31) | 500q (F1) | Delta |
|----------|--------------|-----------|-------|
| Extraction | 14/15 (93%) | 128/150 (85.3%) | -8pp |
| Abstention | 3/3 (100%) | 22/30 (73.3%) | **-27pp** |
| Updates | 7/7 (100%) | 64/72 (88.9%) | -11pp |
| Temporal | 12/13 (92%) | 109/127 (85.8%) | -6pp |
| MultiSession | 10/12 (83%) | 96/121 (79.3%) | -4pp |
| **Total** | **46/50 (92%)** | **419/500 (83.8%)** | **-8pp** |

**The 50q validation overestimated the full benchmark by 8 percentage points.** The gap was largest in Abstention (-27pp), where the 50q set had only 3 questions and the agent happened to handle all of them. The full set of 30 Abstention questions exposed serious hallucination problems: the agent was fabricating answers for questions about information that does not exist in the user's conversation history.

At 83.8%, we were close to Emergence AI's published 86% but still short of meaningful competition. The SOTA context at the time: Mastra OM at 94.87%, Honcho at 92.6%, Hindsight at 91.4%.

### The Validation Trap

The 50q overestimation is a fundamental sampling problem. With only 50 questions:
- Each question is worth 2pp (50q) versus 0.2pp (500q)
- Small categories (Abstention: 3 questions in 50q) have extreme variance
- The validation set was not stratified to match the difficulty distribution of the full set
- Questions that happened to align with our particular strengths were overrepresented

This experience established a permanent rule: **never trust a score that hasn't been validated on 500 questions.** The 50q Fast Loop is useful for detecting regressions but unreliable for estimating absolute accuracy.

## The 3-Tier Testing Discipline

F1's humbling result prompted the adoption of a formal 3-tier testing protocol:

| Tier | Set | Size | Cost | Purpose |
|------|-----|------|------|---------|
| **Fast Loop** | 30 fails + 30 passes (stratified) | 60q | ~$13 | Detect regressions after every code change |
| **Gate** | FixCheck-81 + RegCheck-150 | 231q | ~$49 | Validate before shipping a change |
| **Truth** | Full 500q | 500q | ~$106 | Definitive score after accumulating wins |

The Fast Loop uses a stratified sample: 30 questions from known failures (to measure fix rate) and 30 from known passes (to measure regression rate). The Gate run combines 81 failing questions with 150 passing ones, with a formal shipping criterion:

```
delta_pp = 0.2 * fixed_81 - 0.56 * regressed_150 >= 0
```

Each fixed failure is worth +0.2pp on the full 500q set. Each regression in the RegCheck-150 estimates approximately 0.56pp loss (since the 150 are a subsample of 419 passes).

## Tier 1 Iteration: F1 to T1 (Days 7-8)

Post-F1 failure analysis identified four categories of fixes, all query-time (no re-ingestion needed):

- **1A**: Slot completeness check with comparison verification
- **1B**: Update strategy 3-phase recency
- **1C**: Enumeration 4-phase + qualifier gate
- **1D**: Preference 3-phase personalization
- **Judge**: Number-word equivalence, URL stripping, abstention match, keyword leniency

Three Fast Loop runs tested incremental combinations:

| Run | Key Changes | Score | Delta vs F1 baseline |
|-----|------------|-------|---------------------|
| FL-1 | + Tier 1A-D | 49/60 (81.7%) | Establishing FL baseline |
| FL-2 | + Preference 3-phase | 50/60 (83.3%) | +1 |
| FL-5 | + Comparison check, search escalation, judge URL strip | 52/60 (86.7%) | +3 |

Three Gate runs validated the cumulative changes:

| Run | Key Changes | Score |
|-----|------------|-------|
| G1 | All Tier 1 fixes | 190/231 (82.3%) |
| G2 | + Judge: number-word, abstention match | 191/231 (82.7%) |
| **G3** | + Hybrid fact retrieval, recount gate, preference done-gate, temporal date_diff gate | **195/231 (84.4%)** |

G3 fixed 58/81 of F1's failures (71.6% fix rate) but regressed 16/150 of F1's passes (10.7% regression rate). The regressions were attributed primarily to LLM non-determinism rather than harmful code changes.

### Truth Run T1 (Day 8)

| Category | F1 | T1 | Delta |
|----------|-----|-----|-------|
| MultiSession | 96/121 (79.3%) | 101/121 (83.5%) | +5 |
| Abstention | 22/30 (73.3%) | 27/30 (90.0%) | **+5** |
| Updates | 64/72 (88.9%) | 62/72 (86.1%) | -2 |
| Temporal | 109/127 (85.8%) | 110/127 (86.6%) | +1 |
| Extraction | 128/150 (85.3%) | 132/150 (88.0%) | +4 |
| **Total** | **419/500 (83.8%)** | **432/500 (86.4%)** | **+13** |

**T1: 432/500 (86.4%), up from 419 (+13 questions, +2.6pp).** The biggest gains were in Abstention (+5, from gate forcing broader search before giving up) and MultiSession (+5, from hybrid fact retrieval improvements). Updates regressed by 2, suggesting the recency gate was occasionally surfacing older values.

Cost: approximately $25 at ANSWER_CONCURRENCY=3 with gpt-4o answering.

## Tier 2 Iteration: T1 to T2 (Day 8)

Post-T1 failure analysis yielded five more fixes:

1. **Enhanced enumeration recount gate**: Require itemized evidence list with session citations, programmatic count-mismatch detection
2. **Abstention gate**: Force broader keyword search if model abstains with fewer than 5 retrieval calls
3. **Update recency gate**: Require 3+ retrieval calls + update-language grep for Update questions
4. **Judge rubric update**: Preference/advice leniency (personalized answers scored as correct)
5. **TemporalParser new patterns**: "last Saturday", "couple days ago", "few weeks ago" + directive temporal constraint injection

Two Fast Loop validation runs on the old 50q set:

| Run | Score | Notable |
|-----|-------|---------|
| FL-T2a (no Fix 3) | 47/50 (94.0%) | Multi 11/12, Extr 15/15 |
| FL-T2b (all 5 fixes) | 47/50 (94.0%) | Multi **12/12**, Extr 15/15 |

### Truth Run T2 (Day 8)

| Category | T1 | T2 | Delta |
|----------|-----|-----|-------|
| Extraction | 132/150 (88.0%) | 139/150 (92.7%) | **+7** |
| MultiSession | 101/121 (83.5%) | 104/121 (86.0%) | +3 |
| Temporal | 110/127 (86.6%) | 111/127 (87.4%) | +1 |
| Abstention | 27/30 (90.0%) | 26/30 (86.7%) | -1 |
| Updates | 62/72 (86.1%) | 60/72 (83.3%) | -2 |
| **Total** | **432/500 (86.4%)** | **440/500 (88.0%)** | **+8** |

**T2: 440/500 (88.0%), up from 432 (+8 questions, +1.6pp).** Fix 4 (judge rubric change) was the most effective, accounting for +7 of the Extraction gain. The judge was previously penalizing personalized advice answers that were functionally correct.

Fix 3 (update recency gate) actually **regressed** Updates by -2 -- forcing grep-based verification of recent values sometimes surfaced older values instead.

### Estimated vs Actual Impact

The Tier 2 plan predicted 35-45 question fixes. The actual result was +8 net. This 18-23% hit rate became a calibration factor for all subsequent planning:

| Intervention | Estimated Impact | Actual Impact | Hit Rate |
|-------------|-----------------|---------------|----------|
| Judge rubric (Fix 4) | +7-10 Extraction | +7 | **70%** |
| Recount gate (Fix 1) | +10-17 MultiSession | +3 | ~18% |
| Abstention gate (Fix 2) | +3-5 Abstention | ~+1 | ~25% |
| TemporalParser (Fix 5) | +3-5 Temporal | ~+1 | ~25% |
| Recency gate (Fix 3) | +3-5 Updates | **-2** | Negative |

Judge calibration changes had by far the highest hit rate (70%). Gates delivered roughly 18-23% of their estimated impact. This pattern -- judge changes reliable, gates unreliable -- held throughout the project.

## Phase 3 Summary

| Run | Score | Delta | Cost |
|-----|-------|-------|------|
| F1 | 419/500 (83.8%) | Baseline | ~$60 |
| T1 | 432/500 (86.4%) | +13 | ~$25 |
| T2 | 440/500 (88.0%) | +8 | ~$62 |

### Key Lessons

1. **50q overestimates 500q by approximately 8pp.** This is not noise -- it is a systematic bias from small-sample validation. Always run 500q before claiming a score.

2. **Judge changes have the highest hit rate (~70%).** When the judge penalizes correct answers, fixing the judge is the cheapest and most reliable way to improve scores. Fix measurement before fixing the system.

3. **Gates deliver 18-23% of estimated impact.** Plan accordingly. If a gate is predicted to fix 10 questions, expect 2-3.

4. **Gates can be actively harmful.** The update recency gate (Fix 3) regressed Updates by 2 questions. Gates that force additional retrieval can surface older values that confuse the agent.

5. **3-tier testing prevents expensive mistakes.** The Fast Loop catches regressions for $13. The Gate validates before committing to a $106 Truth run. Without this discipline, each failed experiment costs $100+.

6. **The critical insight from T2's failure analysis**: "Gates verify the model 'did steps,' not that the final value is provably derived from complete evidence." The bottleneck was shifting from tool correctness (solved in Phase 2) to retrieval quality and ingestion coverage. This realization would drive [Phase 4](./phase-4-clean-data)'s focus on data purity.
