---
title: "Phase 4: Clean Data & Gate Engineering"
sidebar_position: 4
description: "Ingestion deduplication, P11 anti-abstention gates, and reaching 88.4% (Weeks 2-3)"
---

# Phase 4: Clean Data & Gate Engineering (Weeks 2-3)

Phase 4 is the most technically complex part of the journey. It began with ambitious plans for ingestion improvements (structured supersession, temporal normalization), discovered that those plans were harmful, uncovered a major data duplication problem, established a clean baseline, and ultimately recovered the full score through a single well-targeted gate fix. The arc: plan big, fail, diagnose, simplify, succeed.

## Step 1-4: The B2/B3 Experiments (Days 9-10)

After T2 (440/500, 88.0%), analysis suggested that retrieval quality and ingestion coverage were the bottleneck. Two ingestion-time improvements — labeled B2 and B3 from an earlier architecture plan — were proposed:

- **B2 (Structured Supersession)**: After upserting each fact, search Qdrant for similar existing facts (cosine > 0.92). If an older similar fact exists from the same user, mark it `is_latest=false`. Goal: help the agent distinguish current from stale facts.
- **B3 (Temporal Normalization)**: Resolve relative dates ("yesterday", "last Tuesday") to absolute timestamps during ingestion.

### Step 1: Vanilla Re-Ingestion Baseline

First, a clean baseline was established with no B2/B3 changes:

| Metric | Value |
|--------|-------|
| Sessions | 23,867 |
| Facts | 286,719 |
| Messages | 246,669 |
| Errors | 21 (embedding token limit exceeded) |
| Config | gpt-4o-mini, concurrency=100, temp=0, seed=42 |
| Time | 4h 20m |

### Step 4: B2+B3 Re-Ingestion

With B2+B3 enabled:

| Metric | Vanilla (Step 1) | B2+B3 (Step 4) | Delta |
|--------|-----------------|----------------|-------|
| Facts | 286,719 | 286,596 | -123 |
| Errors | 21 | 404 | +383 |
| Messages | 246,669 | 246,669 | 0 |

The 404 additional errors were mostly embedding API `$.input is invalid` errors from empty/whitespace content. But the real damage was more subtle.

### Step 4b: Fast Loop on B2+B3 Data

| Category | Vanilla Baseline | B2+B3 | Delta |
|----------|-----------------|-------|-------|
| Updates | 8/8 (100%) | 7/8 (87.5%) | **-1 REGRESSION** |
| MultiSession | 14/15 | 13/15 | -1 (stochastic) |
| Extraction | 14/17 | 14/17 | 0 |
| Temporal | 13/15 | 13/15 | 0 |
| Abstention | 5/5 | 5/5 | 0 |
| **Total** | **54/60** | **52/60** | **-2** |

B2 supersession **hurt** the Updates category. Root cause analysis revealed the mechanism: B3's `TemporalParser` resolved "3 months ago" backward from the session date, corrupting `t_valid` timestamps. B2's supersession logic, which depended on `t_valid` ordering, then marked the *newer* fact as `is_latest=false` instead of the older one. The agent saw both "5 pounds" and "10 pounds" facts and combined them into the wrong answer "15 pounds."

### Step 5: Fix B2+B3 + Re-Ingest

Seven fixes were applied and a full re-ingestion run was executed (25 hours at concurrency=20 due to B2's per-user sequential requirement):

| Metric | Value |
|--------|-------|
| Facts | 283,168 (-3,551 from vanilla due to B2 supersession) |
| Messages | 246,658 |
| Errors | 119 (transient) |
| Time | 25h 49m |

### Step 8: Truth Run on Step 5 Data

| Category | T2 (est.) | Step 5 | Delta |
|----------|-----------|--------|-------|
| Abstention | ~27/30 | 28/30 | +1 |
| Extraction | ~136/150 | 136/150 | ~0 |
| MultiSession | ~105/121 | 96/121 | **-9** |
| Temporal | ~113/127 | 106/127 | **-7** |
| Updates | ~68/72 | 63/72 | **-5** |
| **Total** | **~440/500** | **429/500 (85.8%)** | **-11** |

A net loss of 11 questions. Joint forensic analysis traced 71 total failures:

| Root Cause | Count |
|-----------|-------|
| Ingestion variance (different extractions) | ~35 |
| B2 over-dedup (historical events hidden) | ~8-10 |
| B2 under-dedup (stale values both `is_latest=true`) | ~6-8 |
| Stochastic LLM non-determinism | ~12-15 |
| Hallucination | 2 |

B2's fundamental flaw: cosine similarity cannot distinguish "same fact, new value" (real update) from "related historical event" (distinct fact). Real updates have cosine 0.62-0.85 (too low for the 0.92 threshold), while historical events about the same topic have cosine > 0.92 (falsely superseded).

**Verdict: Abandon B2/B3 entirely. Restore vanilla ingestion.** The leaders handle this differently: Honcho uses LLM "dreaming" agents for background consolidation, Hindsight tracks a confidence-scored opinion network. Cosine similarity is not a substitute for semantic understanding.

## Step 10: Vanilla Re-Ingestion (Day 12)

With B2/B3 abandoned, a fresh vanilla re-ingestion was run at high concurrency:

| Metric | Value |
|--------|-------|
| Sessions | 23,867/23,867 (0 errors) |
| Facts | **300,800** |
| Messages | **262,429** |
| Config | gpt-4o-mini, concurrency=150, temp=0, seed=42 |
| Time | **1h 42m** (vs 25h 49m for Step 5) |

This data was designated **I-v10** -- the baseline for all subsequent experiments.

Three Fast Loop variance runs established the range:

| Run | Score | Notes |
|-----|-------|-------|
| FL-10a | 54/60 (90.0%) | |
| FL-10b | 50/60 (83.3%) | Outlier low |
| FL-10c | 55/60 (91.7%) | New FL best |

**Median: 54/60. Range: 50-55.** This 5-question range (83-92%) on a 60-question set underscored why Fast Loop results must be interpreted cautiously.

## P1: The Temporal Solver Disaster (Day 13)

P1 attempted to improve temporal question handling with four new TemporalParser patterns, ordering question detection, a specialized prompt, and an evidence gate. It also included P7a (entity_id fix) which required re-ingestion.

### The Result: NET HARMFUL (-11)

| Category | G-P2 (no P1, I-v10) | G-P1b (P1, I-v10) | Delta |
|----------|---------------------|-------------------|-------|
| Temporal | 49/57 (86.0%) | 39/57 (68.4%) | **-10** |
| Updates | 31/31 (100%) | 29/31 (93.5%) | **-2** |
| Abstention | 14/15 (93.3%) | 13/15 (86.7%) | -1 |
| MultiSession | 49/62 (79.0%) | 50/62 (80.6%) | +1 |
| Extraction | 59/66 (89.4%) | 60/66 (90.9%) | +1 |
| **Total** | **202/231 (87.4%)** | **191/231 (82.7%)** | **-11** |

R-G-P1b was run on the **same I-v10 data** as G-P2, isolating the code change. P1 destroyed Temporal scoring: -10 questions, from 86% to 68%.

Joint investigation (INV-002) identified three mechanisms:

1. **Ordering prompt over-strictness (~60%)**: `is_ordering_question()` matched 24/61 temporal questions (39%), including 19 that were already passing. The prompt said "NEVER guess ordering from vague language. Use exact dates." The agent had been correctly using contextual reasoning; the strict prompt converted correct answers to abstentions.

2. **Verbatim slot matching (~25%)**: The evidence gate extracted comparison slots like "the narrator losing their phone charger" and checked if tool results contained that exact string. Tool results said "lost my charger" -- verbatim `contains()` failed. This wasted iterations and pushed toward abstention.

3. **PointInTime cascade (~15%)**: New temporal parser patterns forced `TemporalIntent::PointInTime`, which added date range filters to Qdrant queries. These filters excluded relevant facts that fell outside the estimated date window.

P1 was selectively reverted (P1R): all ordering/parser/prompt code removed, but UTF-8 safe truncation, 5xx retry, and P7a entity_ids kept.

This was the project's single most expensive lesson: **prompt strictness kills.** "NEVER guess" converts correct contextual reasoning into abstentions. Advisory language ("prefer X when available") preserves the agent's ability to reason correctly in ambiguous cases.

## The Duplication Discovery (Days 13-14)

During P1 investigation, a disturbing observation emerged. I-v10 had 300,800 facts. The P7a re-ingestion (I-p7a) had 282,467. But the dataset has exactly 23,867 sessions -- why the 18,000-fact difference?

Investigation (documented in `extraction-diversity-findings.md`) revealed that **I-v10 had approximately 1,500 sessions extracted twice.** The parallel ingestion pipeline at concurrency=150 had a race condition that caused some sessions to be processed more than once. The extra ~18,000 facts were duplicates.

This explained several mysteries:
- Why score varied so much between ingestion runs (up to 12pp)
- Why some questions worked on I-v10 but failed on I-p7a (duplicate facts provided reinforcing signal)
- Why B2/B3 re-ingestion (283K facts) scored lower -- it had fewer accidental duplicates

### P8: Extraction Cache for Deterministic Ingestion

To eliminate ingestion variance, P8 added an LLM response cache to the extraction pipeline. The cache key is a SHA-256 hash of the full API request body (model + messages + temperature + seed). Any prompt or model change auto-invalidates the cache.

First ingestion run populates the cache; subsequent runs get 100% cache hits, producing identical fact sets regardless of concurrency or scheduling order.

### I-v11: Clean Data Baseline

Using the extraction cache, a clean ingestion was produced:

| Metric | I-v10 (duplicated) | I-v11 (clean) |
|--------|-------------------|---------------|
| Facts | 300,800 | **282,879** |
| Messages | 262,429 | **246,728** |
| Errors | 0 | 0 |

I-v11 matches the dataset exactly: 23,867 sessions producing 282,879 unique facts.

### R-T4: The Bare Truth on Clean Data

| Category | T2 (I-v1, duplicated data) | R-T4 (I-v11, clean data) | Delta |
|----------|--------------------------|-------------------------|-------|
| Extraction | 139/150 (92.7%) | ~130/150 | ~-9 |
| Temporal | 111/127 (87.4%) | 89/127 (70.1%) | **-22** |
| MultiSession | 104/121 (86.0%) | ~98/121 | ~-6 |
| Updates | 60/72 (83.3%) | ~60/72 | ~0 |
| Abstention | 26/30 (86.7%) | ~24/30 | ~-2 |
| **Total** | **440/500 (88.0%)** | **420/500 (84.0%)** | **-20** |

**Clean data dropped the score from 440 to 420 -- a 20-question loss.** The Temporal category collapsed from 87.4% to 70.1% (-22 questions). The duplicate facts in I-v10 had been providing a signal reinforcement effect: when the same fact appeared twice, the agent had more "evidence" to commit to an answer rather than abstaining.

### P9: Retrieval Recall is 498/500

Before panicking about the score drop, P9 built an offline retrieval recall harness. The result: **498/500 questions have their answer facts present in Qdrant on both I-v10 and I-v11.** Retrieval was not the bottleneck. The 20-question drop was entirely a reasoning/confidence issue -- the agent had the data but was refusing to commit to answers.

## P11: Anti-Abstention Gate Fix (+22 Questions)

The temporal forensics report analyzed the 38 temporal failures on I-v11. The finding: **30 of 38 (79%) were false abstentions** -- the agent retrieved relevant evidence but said "I don't have enough information." Of these, 22 were PASS-to-FAIL regressions that had worked on I-v10's duplicated data.

P11 identified a specific bug: the anti-abstention keyword check required `abstention_gate_used == true`, but this flag was only set when `retrieval_call_count < 5`. Questions with 5 or more retrieval calls could abstain unchallenged -- the anti-abstention gate had a dead code path.

P11 also fixed a gate loop deadlock. The date_diff gate (which rejects answers where the agent's number disagrees with the tool's computation) was not marked as one-shot, meaning it could fire repeatedly. Combined with the duplicate-call detection logic, this created an infinite loop that burned $0.50+ per question without ever converging.

### The Fix

Three changes:

1. **Anti-abstention keyword check**: Fire regardless of `retrieval_call_count`, not just when < 5
2. **Gate message filtering**: `collect_retrieval_results()` filters out gate rejection messages (prevents the anti-abstention gate from triggering on its own rejection text)
3. **date_diff gate one-shot + abstention-first ordering**: Make date_diff gate fire at most once; run abstention check before numeric gates

### R-T5: 442/500 (88.4%) -- Best Ever on Clean Data

| Category | R-T4 (I-v11, pre-P11) | R-T5 (I-v11, post-P11) | Delta |
|----------|----------------------|------------------------|-------|
| Extraction | ~130/150 | 140/150 (93.3%) | +10 |
| Temporal | 89/127 (70.1%) | 110/127 (86.6%) | **+21** |
| MultiSession | ~98/121 | 103/121 (85.1%) | +5 |
| Updates | ~60/72 | 64/72 (88.9%) | +4 |
| Abstention | ~24/30 | 25/30 (83.3%) | +1 |
| **Total** | **420/500 (84.0%)** | **442/500 (88.4%)** | **+22** |

P11 recovered 22 questions from the clean-data drop, bringing the score above the previous best of 440 on duplicated data. Temporal jumped from 70.1% to 86.6% (+21 questions) -- the false abstention problem was almost entirely resolved.

442/500 (88.4%) on clean, deterministic data. This would prove to be the permanent high-water mark.

## Phase 4 Summary

| Milestone | Score | Data | Key Event |
|-----------|-------|------|-----------|
| B2/B3 experiment | 429/500 | Step 5 (B2+B3) | B2 supersession harmful, abandoned |
| I-v10 vanilla | ~439/500 | I-v10 (300K, duplicated) | Established baseline |
| P1 temporal solver | -11 on Gate | I-v10 | NET HARMFUL, reverted |
| Clean data (I-v11) | **420/500** | I-v11 (283K, clean) | Duplication discovery, -20q |
| P11 gate fix | **442/500 (88.4%)** | I-v11 (clean) | Anti-abstention + gate loop fix, +22q |

### Key Lessons

1. **Cosine similarity cannot distinguish updates from related facts.** B2's supersession approach (threshold 0.92) was fundamentally flawed. Real updates have cosine 0.62-0.85 (below threshold); related historical events have cosine > 0.92 (false supersession). Only LLM-level understanding can make this distinction.

2. **Data duplication can masquerade as good performance.** I-v10's 300K facts (vs I-v11's 283K) were not "more data" -- they were duplicated extractions providing artificial signal reinforcement. The true clean baseline was 20 points lower.

3. **Deterministic ingestion is prerequisite for meaningful experiments.** Without the extraction cache (P8), every ingestion run produced different facts, causing up to 12pp score variance that dominated all other effects. You cannot tune what you cannot measure.

4. **False abstention is a confidence problem, not a retrieval problem.** Recall was 498/500 on both datasets. The agent had the evidence; it lacked confidence to commit. P11 fixed this by ensuring the anti-abstention gate could always fire, giving the agent a second chance when it retrieved relevant evidence but wanted to give up.

5. **Prompt strictness kills reasoning.** P1's "NEVER guess ordering" converted correct contextual reasoning into abstentions. P11's anti-abstention gate took the opposite approach: give the agent more chances to answer, not fewer. The lesson: at 88%+, the agent is already mostly right. Interventions should make it *less* cautious, not more.

6. **Gate bugs can create infinite loops.** The date_diff gate + duplicate detection interaction burned $0.50/question in a loop that never converged. Every gate must be one-shot or have explicit termination conditions.

With 442/500 on clean data, the question became: could we push higher? [Phase 5](./phase-5-the-ceiling) would answer that question definitively: no, not with the approaches we tried.
