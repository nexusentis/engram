---
title: "Glossary & Conventions"
sidebar_position: 1
description: "Reference guide for terminology, ID systems, testing tiers, and timeline conventions used throughout this documentation."
---

# Glossary & Conventions

This page defines the terminology, ID systems, and conventions used throughout the Engram research documentation. If you encounter an unfamiliar abbreviation or prefix, check here first.

## Proposals (P-numbers)

Every code change hypothesis is assigned a **P-number**. Proposals progress through validation stages and are either shipped, reverted, or abandoned.

| ID Pattern | Meaning | Examples |
|------------|---------|----------|
| `P{n}` | Base proposal | P0 (agentic answerer), P1 (temporal solver), P11 (anti-abstention gate) |
| `P{n}{letter}` | Variant of a proposal | P7a (entity_id fix), P15b (count reducer observe-only) |
| `P{n}-lite` | Minimal version of a failed proposal | P17-lite (telemetry fields only, after full P17 failed) |
| `P{n1}-P{n2}{letter}` | Bundled proposals shipped together | P12-P15b (four answerer quality fixes in one commit) |
| `P-NEW-{letter}` | Late-stage proposals outside original numbering | P-NEW-A (zero-vs-abstain gate), P-NEW-C (routing fix) |

### Proposal Status

| Status | Meaning |
|--------|---------|
| `SHIPPED_CODE` | Committed to main, not yet validated on the full benchmark |
| `FL_VALIDATED` | Passed Fast Loop (60 questions) |
| `TRUTH_VALIDATED` | Passed full 500-question Truth run |
| `ABANDONED` | Rejected before or after implementation |
| Reverted | Shipped, tested, found harmful, removed |

### Key Proposals Quick Reference

| Proposal | What It Did | Outcome |
|----------|-------------|---------|
| P0 | Built the agentic answerer | +14pp (shipped) |
| P1 | Temporal solver with strict prompts | -11q (reverted) |
| P5 | 6 tool correctness bug fixes | +14pp (shipped) |
| P7a | Entity ID fix in extraction | Shipped |
| P8 | Extraction cache for deterministic ingestion | Shipped |
| P11 | Anti-abstention gate fix + gate loop fix | +22q on clean data (shipped) |
| P12-P15b | Answerer quality fixes (truncation, routing, reducer, preference) | Neutral (shipped as cleanup) |
| P16 | Evidence-table finalizer (3 kernels) | Neutral, 1 harmful override (reverted) |
| P17 | Agent evidence arrays in done() | Harmful (reverted); P17-lite telemetry shipped |
| P18 / P18.1 | Graph tools exposed to agent | -5 Gate / -2 FL, zero graph calls (reverted) |
| P20 | Behind-the-scenes graph prefetch | Neutral (shipped as infrastructure) |
| P21 | Temporal scatter search | Rejected pre-implementation |
| P22 | Ensemble router (Gemini + GPT-4o) | +15q (shipped, TRUTH_VALIDATED) |
| P23 | Gemini-tuned anti-abstention | Abandoned; only Gate 16 guard shipped |
| P25 | Abstention override for _abs questions | +6 Abstention (shipped, TRUTH_VALIDATED) |
| P-NEW-C | Update strategy routing fix | +1 (shipped, TRUTH_VALIDATED) |

## Runs

Benchmark runs use a typed prefix indicating their scope:

| Prefix | Type | Size | Cost (approx.) | Purpose |
|--------|------|------|-----------------|---------|
| `FL-` | Fast Loop | 60 questions | ~$4-13 | Detect regressions after code changes |
| `G-` | Gate | 231 questions | ~$15-49 | Validate before shipping a change |
| `T{n}` or `R-T{n}` | Truth | 500 questions | ~$30-106 | Definitive score measurement |
| `#{n}` | Numbered runs | 50 questions | ~$5-10 | Early validation runs (Phases 1-2) |

**Examples**: `FL-P16` (Fast Loop testing proposal P16), `G-P1` (Gate run for P1), `R-T5` (Truth run #5, 442/500).

## Ingestion Snapshots

Each ingestion of the dataset into Qdrant is assigned an `I-` identifier:

| ID | Facts | Key Property |
|----|-------|-------------|
| I-v1 | 286,719 | First clean ingestion |
| I-v10 | 300,800 | Vanilla re-ingestion, accidentally contained ~1,500 duplicated sessions |
| I-p7a | 282,467 | P7a re-ingestion with entity IDs |
| I-v11 | 282,879 | **Clean baseline** — deterministic via extraction cache, matches dataset exactly |

## Testing Tiers

All benchmark changes follow a 3-tier testing protocol:

| Tier | Name | Questions | Composition | Purpose |
|------|------|-----------|-------------|---------|
| 1 | **Fast Loop** | 60 | 30 known failures + 30 known passes (stratified) | Cheap regression detection after every code change |
| 2 | **Gate** | 231 | 81 failing + 150 passing questions | Validate before shipping; formal shipping criterion |
| 3 | **Truth** | 500 | Full benchmark | Definitive score after accumulating wins |

**Shipping criterion** (Gate): `delta_pp = 0.2 * fixed_81 - 0.56 * regressed_150 >= 0`

**Key rule**: A neutral Fast Loop (same score as baseline) is a **stop signal**, not a proceed signal. Investigate before escalating to Gate.

## Failure Modes

| Term | Meaning |
|------|---------|
| `wrong_answer` | Agent returns a definite but incorrect answer |
| `false_abstention` | Agent says "I don't have enough information" when it does |
| `false_positive` | Agent answers confidently when it should abstain (question asks about nonexistent information) |
| `off-by-one` | Agent finds N-1 of N items — the most common aggregation failure |

## Metrics & Units

| Abbreviation | Meaning |
|-------------|---------|
| q | Questions (e.g., "+15q" = 15 additional questions answered correctly) |
| pp | Percentage points (e.g., "+4pp" = 4 percentage point improvement) |
| LOC | Lines of code |

## Timeline Convention

This documentation uses **relative Week/Day references** instead of absolute dates to focus on the narrative arc rather than calendar specifics.

| Reference | Project Phase | What Happened |
|-----------|--------------|---------------|
| Week 1 (Days 1-5) | Phases 1-2 | Non-agentic baseline through agentic breakthrough |
| Week 2 (Days 6-9) | Phase 3 | First full 500-question benchmark, 3-tier testing |
| Week 3 (Days 10-19) | Phases 4-7 | Clean data, ceiling, model upgrade, ensemble |
| Week 4 (Days 20-22) | Phases 8-9 | Productionization, quick wins, architecture review |

**Day 1** = the first benchmark run. The project spanned approximately three weeks of active development.

## Other ID Prefixes

| Prefix | Type | Example |
|--------|------|---------|
| `INV-` | Investigation report | INV-002 (P1 failure analysis) |
| `FIX-` | Specific code fix | FIX-001 |
| `B{n}` | Architecture plan experiments | B2 (structured supersession), B3 (temporal normalization) |

## Category Names

The LongMemEval-S benchmark has five question categories:

| Category | What It Tests | Count |
|----------|--------------|-------|
| Extraction | Recalling specific user facts | 150 |
| Temporal | Reasoning about time (date diff, ordering, anchoring) | 127 |
| MultiSession | Aggregating information across multiple conversations | 121 |
| Updates | Returning the most recent value when something has changed | 72 |
| Abstention | Correctly refusing to answer when information doesn't exist | 30 |
