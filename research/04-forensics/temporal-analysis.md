---
title: Temporal Failures Deep Dive
sidebar_position: 2
---

# Temporal Failures Deep Dive

:::info Phase 4 Snapshot — 17 temporal failures at 442/500
This deep dive analyzes the 17 temporal failures from Phase 4 (442/500). By Phase 11 (479/500), temporal holds at 119/127 (93.7%) with 8 failures remaining — a combination of model diversity gains (Gemini excels at temporal reasoning) and a judge fix for temporal duration comparisons. The I-v10/I-v11 regression analysis and confidence calibration insights below remain foundational.
:::

Temporal questions are the category that best illustrates the gap between retrieval and reasoning. The agent can find the relevant conversation sessions -- recall is 498/500 -- but it struggles to extract the right dates from those sessions, compute differences correctly, and commit to an answer with confidence. This page analyzes the temporal failure patterns in depth, with particular attention to the I-v10 to I-v11 regression that exposed how much the system's temporal performance depended on accidental data duplication.

## Two snapshots of temporal performance

The temporal category tells two very different stories depending on which data snapshot we examine:

| Dataset | Temporal Score | False Abstentions | Notes |
|---------|---------------|-------------------|-------|
| I-v10 (duplicated data) | 109/127 (85.8%) | ~8 | 18 failures |
| I-v11 (clean data, pre-P11) | 89/127 (70.1%) | 30 | 38 failures |
| I-v11 (clean data, post-P11) | 110/127 (86.6%) | ~5 | 17 failures |

The move from I-v10 to I-v11 added 20 temporal failures. P11 (anti-abstention gate fix) recovered 21 of them. The net result is roughly equivalent accuracy, but the composition of failures shifted significantly.

## The I-v10 to I-v11 regression: what happened

When we cleaned the ingestion pipeline (I-v11), we eliminated ~1,500 sessions that had been extracted twice in I-v10. The total fact count dropped from ~301K to 282,879. Retrieval recall remained at 498/500 -- the same sessions were findable. But temporal accuracy collapsed by 20 questions.

**Why duplicate facts helped temporal reasoning:**

The duplicated facts were not providing _new_ information. They were providing _redundant_ information -- the same events described in slightly different phrasings from two extraction passes. When the agent searched for temporal evidence, it would find 2-3 mentions of the same event instead of 1. This had two effects:

1. **Confidence reinforcement.** Multiple mentions of "started yoga on March 15" pushed the agent above its internal confidence threshold for committing to that date. With a single mention, the agent hedged and abstained.

2. **Phrase diversity in retrieval.** Two extraction passes produced slightly different wordings for the same fact. This increased the chance that at least one phrasing would rank highly for a given query, making retrieval more robust to query-fact vocabulary mismatch.

The result was 22 questions that passed on I-v10 but became false abstentions on I-v11. The agent had the same underlying data but lacked the confidence to use it.

## Pre-P11 failure classification (38 failures)

Before P11, the 38 temporal failures broke down along three dimensions. The PASS→FAIL column shows regressions from I-v10 (caused by losing duplicate data); FAIL→FAIL are chronic failures present on both datasets.

### By subtype

| Subtype | Count | PASS→FAIL | FAIL→FAIL | Description |
|---------|-------|-----------|-----------|-------------|
| date_diff | 23 | 20 | 3 | "How many days/weeks/months ago/between..." |
| time_anchored_lookup | 6 | 0 | 6 | "What did I do last Saturday?" / relative time |
| count_with_cutoff | 5 | 3 | 2 | "How many X before/since Y?" |
| ordering_list | 3 | 1 | 2 | "What order did I do X, Y, Z?" |
| duration_aggregation | 1 | 1 | 0 | "How many weeks total on X + Y?" |
| **Total** | **38** | **26** | **12** | |

### By failure mode

| Mode | Count | PASS→FAIL | FAIL→FAIL | Description |
|------|-------|-----------|-----------|-------------|
| false_abstention | 30 | 22 | 8 | Agent says "I don't have enough information" |
| calc_error | 3 | 2 | 1 | Agent finds data but computes wrong answer |
| wrong_count | 2 | 2 | 0 | Agent counts items but gets wrong total |
| incomplete_set | 2 | 0 | 2 | Agent lists items but misses some |
| wrong_value | 1 | 0 | 1 | Agent gives wrong entity (not temporal calc) |
| **Total** | **38** | **26** | **12** | |

### By scope

| Scope | Count | PASS→FAIL | FAIL→FAIL |
|-------|-------|-----------|-----------|
| single_session | 30 | 19 | 11 |
| cross_session | 8 | 7 | 1 |
| **Total** | **38** | **26** | **12** |

**Key patterns**: 22/26 regressions are false_abstention on date_diff questions. All 6 time_anchored_lookup are chronic (never solved, even on duplicated data). Cross-session temporal questions regressed harder (7/8 PASS→FAIL) because they had the most to lose from reduced fact diversity.

## False abstention dominance

30 of 38 temporal failures (79%) were false abstentions. The agent retrieved relevant sessions, found dates within them, but said "I don't have enough information" instead of computing the answer.

This is a confidence calibration problem, not a retrieval problem. The agent's internal threshold for "enough evidence to answer a temporal question" was set too high for the signal density of single-copy facts.

### The anti-abstention gate (P11)

P11 addressed this directly. The anti-abstention keyword check had a bug: it required `abstention_gate_used == true`, but the gate only fired when `retrieval_call_count < 5`. Questions where the agent performed 5 or more searches could abstain unchallenged. P11 also fixed a gate loop deadlock where the non-one-shot date_diff gate combined with duplicate detection to create infinite loops, burning approximately $0.50 per question.

P11 recovered 21 of the 22 PASS-to-FAIL regressions, bringing temporal from 89/127 back to 110/127. The mechanism was straightforward: when the agent attempted to abstain with temporal keywords in the retrieved evidence, the gate forced it to reconsider and attempt an answer.

## Failure classification at 442/500 (post-P11)

After P11, 17 temporal questions remain as failures. Their composition is fundamentally different from the pre-P11 failures:

### By subtype

| Subtype | Count | Description |
|---------|-------|-------------|
| Date diff | 6 | Agent finds events but computes wrong time difference |
| Time-anchored lookup | 5 | Question uses relative time ("last Saturday") that agent cannot resolve |
| Ordering | 3 | Agent must list events chronologically but misses items or misordering |
| Other temporal | 3 | Mixed: wrong entity, wrong ordering, off-by-one count |

### By failure mode

| Mode | Count | Description |
|------|-------|-------------|
| Wrong answer | 11 | Agent commits to an incorrect answer |
| False abstention | 6 | Agent refuses to answer despite having evidence |

The shift from 79% false abstention (pre-P11) to 35% false abstention (post-P11) confirms that the gate fix worked. The remaining failures are harder: they are cases where the agent genuinely reasons incorrectly, not cases where it lacks confidence.

## Time-anchored lookup: the architectural gap

Five temporal failures use relative time expressions that the system cannot resolve:

- "What art event did I attend **two weeks ago**?"
- "What music event did I go to **last Saturday**?"
- "What bike was fixed this **past weekend**?"
- "What did I cook for my friend **a couple days ago**?"
- "What did I invest in **four weeks ago**?"

These questions require mapping a relative time reference to an absolute date range, which in turn requires knowing _when_ the question was asked relative to the conversation sessions. The benchmark provides this temporal context implicitly through the conversation ordering, but the system has no mechanism to resolve "last Saturday" into a specific date and then search for events on that date.

All 5 failed on both I-v10 and I-v11. They represent a structural limitation rather than a tunable parameter. Fixing them would require either:
- Resolving relative time expressions to absolute dates during ingestion (adding temporal anchors to each session)
- Building a query-time temporal resolver that computes "two weeks before the latest session" and translates it to a date range filter

For 5 questions (1% of the benchmark), this is not cost-effective. These are accepted losses.

## Date diff errors: reasoning at the boundary

The 6 date_diff failures are the most instructive because they show the agent finding the right data and then failing at the final step -- arithmetic or date identification.

| QID | Expected | Got | Error Type |
|-----|----------|-----|------------|
| 0bc8ad92 | 5 months | 3 months | Wrong date identification |
| gpt4_a1b77f9c | 8 weeks | 8 weeks (still failed) | Possibly judge error |
| 370a8ff4 | 15 weeks | 11 weeks | Wrong date identification |
| gpt4_7bc6cf22 | 12-13 days | 5 days | Wrong date identification |
| eac54adc | 19-20 days | Abstained | False abstention |
| gpt4_cd90e484 | 2 weeks | 21 days | Unit mismatch (correct value) |

The dominant error is wrong date identification (3/6): the agent picks the wrong event or the wrong date from the retrieved sessions and then computes the difference between incorrect anchors. The arithmetic itself is usually correct once the dates are chosen -- the error is in selecting which dates to use.

One case (gpt4_cd90e484) is noteworthy: the agent answered "21 days" when the expected answer was "2 weeks." These are equivalent values, suggesting this may be a judge strictness issue rather than an agent error.

4 of these 6 are regressions from I-v10. On duplicated data, the redundant temporal mentions helped the agent identify the correct dates. This reinforces the finding that temporal reasoning quality is sensitive to signal density in the retrieved context.

## Ordering failures: aggregation meets temporality

The 3 ordering failures combine the aggregation problem (finding all items) with the temporal problem (sorting correctly):

| QID | Expected | Got |
|-----|----------|-----|
| gpt4_7f6b06db | Hike, road trip, Yosemite | Road trip, Dubai, Yosemite |
| gpt4_d6585ce8 | 5 concerts in order | 4 concerts (missed 1) |
| gpt4_f420262c | JetBlue, Delta, United, AA | JetBlue, AA, JetBlue (3 of 4) |

In all three cases, the agent fails to find all items (same as aggregation off-by-one) and then produces a partial or incorrect ordering. These are among the hardest questions in the benchmark because they require exhaustive search, correct date extraction for each item, and chronological sorting.

## What this tells us about reasoning versus retrieval

The temporal forensics reveal a clean separation between two types of failure:

**Retrieval-addressable failures (solved by P11):** 21 questions where the agent had the data but lacked confidence. These were solved by a gate that forced the agent to try answering instead of abstaining. No changes to retrieval were needed.

**Reasoning-limited failures (the remaining 17):** Questions where the agent commits to an answer but gets it wrong. The data is in the context. The dates are findable. The agent picks the wrong dates, computes incorrectly, or misses items. These cannot be solved by retrieving more data or adjusting confidence thresholds.

This distinction is the central finding of the temporal forensics. The system has moved past the "can we find the data" phase into the "can we reason correctly over the data" phase. The gap to SOTA (Mastra OM at 94.9% with zero retrieval) confirms that reasoning quality, not data architecture, is the frontier.

The practical implication is that further improvements in temporal accuracy require either:
- A stronger base model with better temporal reasoning (model upgrade)
- Structured temporal scaffolding that decomposes date arithmetic into explicit steps the model can verify
- A fundamentally different approach like Mastra's observation-log compression that gives the model pre-digested temporal summaries instead of raw conversation transcripts

All three paths represent significant architectural investments for diminishing marginal returns. At 86.6% temporal accuracy with 5 structurally unfixable questions, the realistic ceiling for the current architecture is approximately 91-92% on temporal questions alone.
