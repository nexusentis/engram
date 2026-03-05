---
title: "Dead Ends & Failed Experiments"
sidebar_position: 5
description: "Everything we tried that didn't work — and why. The most expensive lessons."
---

# Dead Ends & Failed Experiments

Between 442/500 (88.4%, Phase 4) and 479/500 (95.8%, Phase 11), we spent approximately **$800+ on experiments that produced zero net improvement.** Some produced negative improvement. This section documents every one of them, because knowing what does not work is at least as valuable as knowing what does.

The pattern is consistent: at high accuracy, most interventions are neutral or harmful. The agent is already correct on the vast majority of questions. Any change that "helps" the failing 6-12% must not disturb the passing 88-94%. This is a much harder constraint than it sounds.

What finally broke through the 88% ceiling was not any of these interventions — it was **model diversity** (Phase 6-7), which gained +25 questions through a model swap and ensemble router. The lesson: when single-model engineering hits diminishing returns, try a qualitatively different approach.

## Summary of All Failed Interventions

| Intervention | Date | Cost | Result | Category |
|---|---|---|---|---|
| Deterministic resolver (FL-A8a) | Day 12 | ~$13 | -2q (harmful) | Post-processing |
| P12-P15b answerer quality fixes | Day 14 | ~$62 | Gate-neutral (203/231 vs 202/231) | Post-processing |
| P16 evidence-table finalizer | Day 15 | ~$14 | Net neutral, 1 harmful override | Post-processing |
| P1 temporal solver v2 | Day 13 | ~$62 | -11q on identical data | Gate/prompt engineering |
| Query expansion (P2-1 through P2-4b) | Day 12 | ~$65 | -2 to -4q, all reverted | Retrieval tuning |
| LLM reranking (Runs 6a-6b) | Day 2 | ~$13 | 62-64% (down from 78%) | Retrieval tuning |
| P18 graph tools for agent | Day 16 | ~$62 | -5 Gate, 0 graph tool calls | Graph integration |
| P18.1 scoped graph tools | Day 16 | ~$13 | -2 FL, 0 graph tool calls | Graph integration |
| P20 behind-the-scenes graph prefetch | Day 17 | ~$13 | Neutral (54/60 = baseline) | Graph integration |
| P21 temporal scatter search | Day 17 | $0 | Rejected pre-implementation | Retrieval tuning |
| B2/B3 supersession + consolidation | Day 11 | ~$20 | Neutral | Ingestion |
| I-v12 dual-seed ingestion | Day 14 | ~$120 | -6q (436/500) | Ingestion |
| P17 structured evidence array | Day 16 | ~$26 | MATCH bypass: -4 regressions | Post-processing |
| P23 Gemini-tuned anti-abstention | Day 22 | ~$0 | Abandoned pre-implementation (review) | Gate engineering |
| P30 balanced truncation for Enumeration | Day 21 | ~$6 | Inert (never fired), reverted | Truncation engineering |
| P31 enum uncertainty fallback | Day 21 | ~$8 | Neutral (fired 3x, 0 flips on FL) | Ensemble/routing |

**Total estimated cost of failed experiments: ~$824+**

*P31 demonstrates that routing between models is only useful when they have complementary failure modes. On shared failures (both models wrong for the same structural reason), no amount of routing helps. This confirms the ensemble ceiling identified in Phase 7.*

*P23 is notable for being abandoned before spending any money — a $0 dead end. A pre-implementation review revealed that the remaining false abstentions were loop-break failures (both models exhausted), not gate threshold failures. Only the Gate 16 `_abs` guard (a cost optimization) was shipped. This demonstrates the value of the "review before running" discipline.*

## The Meta-Lesson

At 88%+ accuracy, the dynamics of improvement change fundamentally:

1. **The base rate of harm exceeds the base rate of help.** Any change affects both passing and failing questions. With 442 passing and 58 failing, a change that helps 3 failures but regresses 2 passes is barely a win — and most changes regress more than they fix.

2. **Estimated impact is systematically wrong.** Across all experiments, plans predicted "+5 to +8 questions." Reality delivered -11 to +0. We learned to discount all estimates by 3x and add regression risk. Even after discounting, estimates were still too optimistic.

3. **Deterministic post-processing is a dead end.** Three independent attempts to deterministically correct agent answers all failed. The remaining failures are semantic reasoning errors, not arithmetic mistakes.

4. **Graph exposure to agents is unprecedented for good reason.** No top-performing system on LongMemEval exposes graph tools to agents. The correlation between graph complexity and benchmark score is negative.

5. **Retrieval augmentation hits diminishing returns at high recall.** With 498/500 recall, any "find more facts" intervention mostly rediscovers already-found facts.

## Detailed Analysis

Each of these pages examines a class of failed interventions in depth, with root cause analysis and the specific lessons extracted:

- [**Deterministic Post-Processing**](./deterministic-postprocessing.md) -- Three failed attempts to correct agent answers after the fact
- [**Graph Tools & Retrieval Augmentation**](./graph-tools.md) -- Four attempts to use knowledge graphs, all neutral or harmful
- [**Gate & Prompt Engineering Pitfalls**](./gate-engineering.md) -- Ordering prompts, query expansion, LLM reranking, and the "NEVER" rule
- [**Engineering Discipline Rules**](./engineering-discipline.md) -- 22 rules extracted from $1,400+ of failed experiments
