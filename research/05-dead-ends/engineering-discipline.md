---
title: "Engineering Discipline Rules"
sidebar_position: 4
---

# Engineering Discipline Rules

These 22 rules were extracted from $1,400+ of failed experiments over three weeks. Each one was learned the hard way -- by violating it and paying the cost. They are organized by phase of work: before writing code, while designing interventions, while evaluating results, and when to stop.

The rules are specific to the context of optimizing an LLM-based benchmark system at high accuracy (88%+), but many generalize to any system where you are making incremental changes to a complex pipeline and measuring the results against a fixed test set.

## Before ANY Code Change

### Rule 1: Trace the full cascade on paper first

Every change touches a pipeline. Before writing code, map the full cascade: input -> what fires -> what overrides what -> what gets filtered -> what the agent sees.

**The experiment that taught us**: P1 added new TemporalParser patterns that forced `TemporalIntent::PointInTime`, which overrode existing intent classification, which added `t_valid` date range filters to Qdrant queries, which excluded relevant facts. This was predictable from reading the code -- the `PointInTime` override at line 101 of the temporal analyzer was documented. But nobody traced the cascade from "new pattern" to "excluded facts" before running a $62 Gate test.

**Cost of violation**: ~$75 (Gate run + isolation test for P1, producing -11q).

### Rule 2: Test detection/matching against the FULL question set before running benchmarks

Before any benchmark run, do a dry-run that shows which questions are affected, what patterns fire, what intents change. This costs $0 and takes minutes.

**The experiment that taught us**: P1's `is_ordering_question()` hit 24 of 61 temporal questions (39%). It was designed to target approximately 5 ordering failures. A 5-minute grep over the question text would have revealed the over-broad matching before spending $62 on a Gate run.

**Cost of violation**: ~$62 for a Gate run that was doomed before it started.

### Rule 3: A neutral Fast Loop is a STOP signal, not a proceed signal

If your change shows +0 on the Fast Loop (60 questions, $13), it means the change is either not firing or firing without effect. Do not proceed to a $49 Gate run. Investigate why the Fast Loop was neutral first.

**The experiment that taught us**: P1 scored 53/60 on the Fast Loop, identical to baseline. We proceeded to the Gate anyway ($62), which revealed -5 (and later -11 on isolation). The Fast Loop neutrality should have triggered investigation, not escalation.

**Cost of violation**: ~$62 for a Gate run that confirmed what the Fast Loop already showed (neutral at best).

## Intervention Design

### Rule 4: NEVER add "NEVER" to agent prompts

Prohibitive language ("NEVER guess," "you MUST use exact dates") kills correct contextual reasoning. The agent at 88% is already good. Advisory language ("prefer exact dates when available") is safe. Prohibitive language is not.

**The experiment that taught us**: P1's ordering prompt said "NEVER guess ordering from vague language. Use exact dates." The agent was correctly inferring temporal ordering from context like "after visiting the museum, I went to the park." The "NEVER" converted these correct inferences into abstentions. 19 of 24 affected questions had been passing.

**Cost of violation**: ~10 temporal regressions, directly attributable to the "NEVER" instruction.

### Rule 5: NEVER add gates without testing interaction with existing gates

Gates compound. A3 + ordering gate double-fired on the same `done()` call. Before adding any gate, list all existing gates and trace what happens when they fire sequentially.

**The experiment that taught us**: P1's ordering evidence gate ran after the A3 evidence gate. Both could reject the same answer, producing two rejection messages in one turn. The agent interpreted the double rejection as strong evidence it was wrong and abstained.

**Cost of violation**: Multiple false abstentions from double-gating, contributing to P1's -11q.

### Rule 6: Verbatim string matching is almost always wrong for natural language

`tool_results.contains("the narrator losing their phone charger")` will never match "lost my charger." If you need entity matching, use keyword extraction or embedding similarity, not `contains()`.

**The experiment that taught us**: P1's ordering evidence gate used `extract_comparison_slots()` to produce literal phrases from the question, then checked `contains()` against tool results. Tool results use different wording, first-person perspective, and abbreviations. The match rate was near zero on legitimate matches, causing the gate to inject false "Missing evidence" warnings.

**Cost of violation**: Multiple incorrect rejection signals, contributing to false abstentions.

### Rule 7: Do not constrain what was already working

If 49 of 57 temporal questions pass without date filters, adding date filters to "help" is net destructive. Only constrain retrieval for questions that are currently failing, never for questions that are passing.

**The experiment that taught us**: P1's TemporalParser patterns added date range filters to Qdrant queries. These filters excluded relevant facts on passing questions. 49/57 temporal questions were passing without the filters. The filters "helped" at most 2-3 failing questions while breaking up to 10 passing ones.

**Cost of violation**: -10 temporal regressions.

## What Actually Moves the Score at 88%+

### Rule 8: Retrieval coverage and ingestion completeness matter more than prompt/gate engineering

Most remaining failures at 88% are `false_abstention` (data not surfaced to agent) or `wrong_items` (agent searched but found wrong data), not `wrong_reasoning` (agent had right data but reasoned incorrectly). Fix the data pipeline, not the reasoning.

**The data**: P9 proved retrieval recall is 498/500. But the agent's search strategy does not always find the right facts from the 283K available. The gap is search strategy, not index coverage and not reasoning prompts.

**Calibration**: Tool correctness fixes (P0, P5) delivered +14pp. All prompt/gate engineering combined delivered approximately +4pp, with many negative experiments along the way.

### Rule 9: Small, isolated, testable changes only

One mechanism per PR. If it touches both the parser, the answerer, and the ingester, it is too big to diagnose when it fails.

**The experiment that taught us**: P1 combined 4 new TemporalParser patterns, an ordering detection function, an ordering prompt, and an ordering evidence gate. When it regressed by -11, we needed a separate isolation test ($13) to determine that the code changes (not ingestion variance) were the cause. Even then, we could not separate which of the 4 mechanisms was most harmful without further testing.

**Cost of violation**: $13 for the isolation test, plus hours of analysis that would have been unnecessary with a single-mechanism change.

## Cost Discipline

### Rule 10: Every failed experiment costs $50-100 and produces nothing

Before proposing an intervention, ask: "What is my prior that this works?" If the answer is below 70%, do not run it. Redesign until confidence is higher.

**The data**: P1 cost ~$75. P16 cost ~$14 (cheapest failure). P18 cost ~$75. P2 series cost ~$65. Each produced zero lasting improvement. Total: ~$229 for these four lines of inquiry alone.

### Rule 11: Estimated impact is almost always wrong -- discount by 3x

Plans consistently predicted "+5 to +8 questions." Reality consistently delivered -11 to +0. After three weeks, we adopted a 3x discount on all estimates and added regression risk to every projection.

**The data**: Tier 1 fixes predicted ~35-45 fixes, achieved +8 net (18-23% hit rate). P1 predicted +3-5, achieved -11. P16 predicted +3-6, achieved +0. P18 predicted +3-6, achieved -5.

Even after 3x discounting, estimates remained optimistic. The fundamental problem is that interventions interact with the entire system in ways that are hard to predict, and the regression risk from affecting 442 passing questions is systematically underestimated.

## Dead Ends (NEVER Retry)

These approaches have been empirically proven ineffective for LongMemEval at 88%+ accuracy. Each entry names the specific experiment that proved it.

### Rule 12: Deterministic post-processing of LLM answers is a dead end

Three attempts failed: the resolver (-2q, FL-A8a), P12-P15b answerer quality fixes (neutral), and P16 evidence-table finalizer (neutral + 1 harmful). Tool result text is natural language, not structured data. You cannot reliably extract "items" or "values" from it. The agent is already correct when deterministic correction would be easy (date_diff matches 100%). The remaining failures are semantic.

**Total cost**: ~$90 across three experiments.

### Rule 13: Evidence row counting is not item counting

Tool results contain conversation messages, not items. Two messages about the same event produce 2 rows. Keyword-based filtering (2+ hits) does not distinguish "relevant mention" from "distinct item." Off-by-one correction is especially dangerous because +/-1 is both the most common real error and the most common dedup noise.

**The experiment**: P16 COUNT kernel reported 28 evidence rows for a claimed count of 5 items. The only override it fired changed 3 to 4 and was wrong (two rows for one bible study).

### Rule 14: "Narrow OR broad" is a false choice for override kernels

Generic token matching (LATEST_VALUE v1) causes catastrophic false positives ("rainwater harvesting" replacement). Restricting to extractable patterns (dollar amounts, time expressions) means the kernel never fires on the actual failures (locations, entity names). There is no middle ground without NLU-level understanding, which is what the LLM agent already provides.

**The experiment**: P16 LATEST_VALUE kernel. V1 matched any token, causing catastrophic false positives in smoke testing. V2 restricted to dollars/times and never fired on any real failure.

### Rule 15: Exposing graph tools to the agent is a dead end

P18 (global, -5 Gate) and P18.1 (Enumeration-only, -2 FL) both regressed. Agent made zero graph tool calls in both runs. Regressions came from schema bloat alone. No SOTA system exposes graph tools to agents. Hindsight (the only successful graph system) uses graph behind the scenes.

**Total cost**: ~$75 across two experiments.

### Rule 16: Tool schema count is a first-class engineering constraint

Adding 4 tool schemas (8 -> 12 tools) changed model planning behavior even when new tools were never called. This is not fixable with prompt engineering -- it is a fundamental LLM function-calling property. Never add tools "just in case."

**The experiment**: P18 A/B test. `GRAPH_RETRIEVAL=0` (8 tools): 9/16 correct. `GRAPH_RETRIEVAL=1` (12 tools): 8/16 correct. Zero graph tool calls in either run.

### Rule 17: The count reducer's enforced corrections are harmful

Agent said "5 trips" (correct), reducer deduped to 4 unique items and overwrote the answer to "4 trips" (wrong). Enforced corrections were disabled and moved to observe-only mode.

**The experiment**: P18.1 FL revealed Q59. This was fixed as part of P12-P15b.

### Rule 18: Structured evidence arrays from the agent are unreliable

P17 asked the agent to include an evidence array in `done()`. The agent treated it as "a few examples," not an exhaustive list (claimed 99 items, provided 4 in the array). MATCH bypass caused 4 direct regressions (short-circuited recount gates on off-by-one). MISMATCH rejections burned 36 iterations for zero benefit.

**The experiment**: P17 (February 25). Same root cause as P16: evidence counting does not equal item counting.

### Rule 19: Bypassing existing quality gates is strictly harmful

P17 MATCH set `recount_verified=true` when evidence matched the count, bypassing recount and completeness gates. All 4 regressions were off-by-one: agent found N-1 items, evidence had N-1 items, MATCH said "looks good" -- but the recount gate exists precisely to push for more searching. Never bypass working gates.

**The experiment**: P17. The recount gate would have caught the off-by-one; the MATCH bypass prevented it from firing.

### Rule 20: Behind-the-scenes graph prefetch is neutral at high recall

P20 implemented Hindsight-style silent graph augmentation with 3-phase spreading activation (seed extraction -> 1-hop neighbors -> Qdrant fetch). Fires on 17/20 targeted questions, injects 253-1773 chars of graph-linked context. FL result: 54/60 = exact baseline. Root cause: Qdrant vector search already retrieves the same facts the graph surfaces. Graph adds diversity only when vector search misses items -- but recall is 498/500.

**The experiment**: P20 (Day 17). Committed as infrastructure but does not move score.

### Rule 21: Temporal scatter search for enumeration is likely neutral

P21 (reviewed and rejected pre-implementation): only 8/13 aggregation failures route to Enumeration strategy, 6/13 have single-day haystacks making temporal buckets useless, and `search_facts` lacks a date-range parameter.

**The experiment**: P21 (Day 17). Rejected before any code was written, saving at least $13.

### Rule 22: Query expansion and RRF tuning dilute precision at high recall

P2-2 (3-variant expansion) was -4. P2-3 (1-variant) was -2. P2-4a/b (RRF k tuning) were -2 to -3. At 498/500 recall, multiple query variants retrieve the same relevant facts plus additional irrelevant facts. RRF fusion averages away the original query's precision.

**Total cost**: ~$65 across 5 Fast Loop runs, all reverted.

## Summary

These rules are not theoretical. Each was extracted from a specific experiment with a specific cost. The total cost of learning them was approximately $1,400 in API compute plus three weeks of engineering time. They exist so that the next experiment starts from a higher baseline of knowledge, not from the same naive optimism that produced the first $1,400 in failed experiments.

The deepest lesson is epistemic: at high accuracy on a complex benchmark, your intuition about what will help is unreliable. The system is too complex to reason about from first principles. The only reliable signal is empirical measurement -- and even that is noisy (gate stochastic variance of +/-20 questions on 231-question runs). Design cheap experiments, measure carefully, and have the discipline to stop when the data says stop.
