---
title: The Journey
sidebar_position: 3
description: "From 0% to 95.8% — the chronological story"
---

# The Journey: From Zero to 95.8%

This is the chronological story of building Engram's LongMemEval-S benchmark score from nothing to 479/500 (95.8%) over approximately four weeks. It is a story of rapid progress, humbling setbacks, and hard-won lessons about what actually moves the needle in AI memory systems.

Each **phase** represents a distinct strategic direction — a hypothesis about what would move the score, the experiments that tested it, and what we learned. Some phases lasted days, others a single afternoon. Results were validated through a 3-tier testing protocol: **Fast Loop** (60 questions, ~$4, for rapid iteration), **Gate** (231 questions, ~$15, for promotion decisions), and **Truth** (full 500 questions, ~$30-100, for final validation).

## Score Progression

The following table traces the major milestones. Each entry represents a meaningful inflection point, not every individual run (there were 40+ runs total across this period).

```
Score (%)
  |
96|                                                                                                    * Phase 11 (95.8%)
95|                                                                                        * Phase 9 (94.4%)
94|                                                                      * P22 Ensemble (93.4%)
  |
92|                                        o #31 (92%, 50q peak)
  |
90|                       o #26 (90%)  o #30                   * Gemini (90.4%)
  |
88|                    o #23 (88%)              * T2 (88.0%, 500q)
  |                                                          * R-T5 (88.4%)
86|                                       * T1 (86.4%)
  |
84|                 o #21 (84%)        * F1 (83.8%)
  |
82|           o #17 (82%)
  |
80|
  |
78|        o #5 (78%)
  |
76|
  |
74|  o #2 (74%)
72|  o #1 (72%)
  |
70| o #3 (70%, agentic)
  |
68| o #4 (68%)
  |
  +---+-----+-----+-----+-----+-----+-----+-----+-----+-----+----> Date
   W1D1  W1D2  W1D4  W1D5  W1D6  W2D7  W2D8  W2D9  W3D14  W3D18  W3D19  W4

  o = 50q validation run    * = 500q truth run
```

## The Eleven Phases

| Phase | Period | Score Arc | Key Event |
|-------|--------|-----------|-----------|
| [Phase 1: Building the Baseline](./phase-1-baseline) | Week 1, Days 1-4 | 72% to 78% | Date-grouped formatting (+4pp), LLM reranking disaster |
| [Phase 2: The Agentic Breakthrough](./phase-2-agentic) | Week 1, Days 5-6 | 70% to 92% (50q) | 6-fix correctness patch (+14pp), temporal fixes |
| [Phase 3: Scaling to 500 Questions](./phase-3-full-benchmark) | Week 2, Days 7-9 | 83.8% to 88.0% (500q) | The humbling 50q-to-500q gap, tier testing discipline |
| [Phase 4: Clean Data & Gate Engineering](./phase-4-clean-data) | Week 2-3, Days 9-14 | 420 to 442 on clean data | Deduplication discovery, P11 (anti-abstention gate fix, +22 questions) |
| [Phase 5: Hitting the Ceiling](./phase-5-the-ceiling) | Week 3, Days 14-17 | 442/500 (88.4%) flat | Every intervention neutral or harmful |
| [Phase 6: Model Upgrade & Multi-Model Analysis](./phase-6-model-upgrade) | Week 3, Days 17-18 | 442 to 452/500 | Gemini 3.1 Pro +10q, complementary failure modes discovered |
| [Phase 7: Ensemble Router](./phase-7-ensemble) | Week 3, Days 18-19 | 452 to 467/500 | P22 (ensemble router) Gemini+GPT-4o +15 questions, jumped to #2 globally |
| [Phase 8: Productionization](./phase-8-productionization) | Week 3-4, Days 19-20 | 467/500 (unchanged) | Monolith → 6 crates, REST server, -12K LOC dead code |
| [Phase 9: Quick Wins & Architecture Review](./phase-9-architecture-review) | Week 4, Day 22 | 467 to 472/500 | 4 quick wins (+5), Abstention 100%, architecture deep dive charts path to 96%+ |
| [Phase 10: GPT-5.2 Ensemble Experiment](./phase-10-gpt52-ensemble) | Week 4, Day 21 | 472 to 466/500 | GPT-5.2 primary + Gemini fallback — proved ensemble direction matters |
| [Phase 11: Inverted Ensemble — #1 Globally](./phase-11-inverted-ensemble) | Week 4, Day 21 | 466 to 479/500 | Gemini + GPT-5.2 fallback — **#1 globally (95.8%)**, Extraction 100% |

## Cumulative Investment

| Metric | Final Value |
|--------|-------------|
| Total API spend | ~$1,500+ |
| Total development time | ~3 weeks |
| Total benchmark runs | 40+ (50q) + 10 (500q) |
| Ingestion runs | 8+ (each ~$8, 1-25 hours) |
| Lines of Rust changed | ~16,000+ |
| Proposals attempted | 26+ (P0 through P26, plus P-NEW-A/C) |
| Proposals that shipped | 13 (P0, P2, P5, P7a, P8, P11, P12-P15b, P20, P22, P23, P25, P-NEW-C, judge fix) |
| Proposals reverted or abandoned | 10+ |

## The Arc in One Sentence

Tool correctness and data formatting got us from 70% to 88%; a model swap to Gemini 3.1 Pro broke through the ceiling to 90.4%; exploiting complementary failure modes via a simple ensemble router reached 93.4%; quick wins pushed to 94.4% with Abstention at 100%; and replacing GPT-4o with GPT-5.2 as fallback reached 95.8% (#1 globally), with Extraction hitting 100% for the first time.
