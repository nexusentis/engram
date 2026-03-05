---
title: "Phase 10: GPT-5.2 Ensemble Experiment"
sidebar_position: 11
description: "GPT-5.2 primary + Gemini fallback ensemble: 466/500 (93.2%) in 39 min"
---

# Phase 10: GPT-5.2 Ensemble Experiment

**Period**: Week 4, Day 21
**Starting Score**: 472/500 (94.4%) --- Phase 9, Gemini primary
**Ending Score**: 466/500 (93.2%) --- GPT-5.2 primary + Gemini fallback

## Context

GPT-5.2 scored 453/500 standalone --- nearly identical to Gemini (452) but **~15x faster** (10 min vs 3h48m). Oracle analysis showed 491/500 (98.2%) ceiling with complementary errors: 39 questions where GPT-5.2 fixes Gemini, 38 where Gemini fixes GPT-5.2, and 9 shared failures.

The hypothesis: GPT-5.2 as primary (for speed) with Gemini fallback (for rescue) would match or exceed Phase 9's 472.

### Per-Category Model Strengths

| Category | Gemini | GPT-5.2 | Winner | Delta |
|----------|--------|---------|--------|-------|
| Temporal | 118/127 | 109/127 | **Gemini** | +9 |
| Updates | 66/72 | 64/72 | **Gemini** | +2 |
| Abstention | 24/30 | 30/30 | **GPT-5.2** | +6 |
| MultiSession | 101/121 | 107/121 | **GPT-5.2** | +6 |
| Extraction | 143/150 | 143/150 | Tie | 0 |

## Code Changes

### 1. Smart-Quote Bug Fix --- `is_prompt_abstention()`

GPT-5.2 outputs Unicode right single quotation mark (U+2019) instead of straight apostrophe ~56% of the time. This broke `is_prompt_abstention()` detection, silently preventing ensemble fallback on abstentions containing "don\u{2019}t".

Fix: normalize curly apostrophes before matching in `is_prompt_abstention()`.

### 2. Retry Log Enhancement

Added model name to all retry log lines: `[RETRY] Rate limited (429) on gpt-5.2, retrying in 2s (attempt 1/12)`. Previously logs didn't identify which model was rate-limited.

### 3. Vertex AI Region Rotation

Added region cycling on 429s for Vertex AI endpoints: `global` -> `us-central1` -> `europe-west1` -> `asia-southeast1`. Confirmed that Vertex quota is project-level (not regional), so rotation doesn't help --- but the infrastructure is in place if regional quotas are enabled later.

## Fast Loop Validation

Two FL runs validated the changes.

### FL #1 (before bug fixes): 54/60 --- -2 from baseline

Two regressions: Q20 (smart-quote bug blocked fallback), Q34 (stochastic). This identified the smart-quote bug.

### FL #2 (after bug fixes): 57/60 --- +1 over Phase 9 baseline

Smart-quote fix working. Duration was 1609s (~27 min) due to Gemini 429 retry storms.

## Truth Run: 466/500 (93.2%)

**Runtime**: 2346s (~39 min), concurrency 7
**Config**: GPT-5.2 primary, Gemini (Vertex AI) fallback

| Category | Phase 10 (466) | Phase 9 (472) | GPT-5.2 Solo (453) | Delta vs P9 |
|----------|----------------|---------------|---------------------|-------------|
| Extraction | 147/150 (98.0%) | 148/150 (98.7%) | 143/150 (95.3%) | -1 |
| MultiSession | 107/121 (88.4%) | 106/121 (87.6%) | 107/121 (88.4%) | +1 |
| Updates | 66/72 (91.7%) | 69/72 (95.8%) | 64/72 (88.9%) | **-3** |
| Temporal | 116/127 (91.3%) | 119/127 (93.7%) | 109/127 (85.8%) | **-3** |
| Abstention | 30/30 (100%) | 30/30 (100%) | 30/30 (100%) | 0 |

### Ensemble Trigger Summary

76 questions triggered fallback to Gemini:

| Trigger | Count |
|---------|-------|
| Abstention | 41 |
| Enum-uncertainty (>= 8 iterations) | 22 |
| Loop-break (iteration exhaustion) | 13 |

### Fallback Effectiveness

| | Correct | Total | Rate |
|---|---|---|---|
| **Primary (GPT-5.2)** | 399 | 424 | 94.1% |
| **Fallback (Gemini)** | 67 | 76 | 88.2% |

Fallback by category:
- MultiSession: 26/31
- Temporal: 22/24
- Extraction: 11/12
- Updates: 8/9

9 fallback failures (Gemini also got wrong): 5 MultiSession, 2 Temporal, 1 Extraction, 1 Updates.

### 429 Rate Limiting

- **GPT-5.2**: Zero 429s across entire run
- **Gemini (Vertex AI)**: 138 retry events, max depth attempt 8/12, zero exhaustion failures
- Retry distribution: 75 at attempt 1, 33 at attempt 2, 16 at attempt 3, 8 at attempt 4, 6 at attempt 5+
- Region rotation logged but quota is project-level --- all regions hit same limit

### Generative Language API Attempt (Failed)

Before the successful Vertex AI run, we attempted using Google's Generative Language API (`generativelanguage.googleapis.com`) with an API key. This hit the Tier 1 RPD (Requests Per Day) limit of 250 after ~443/500 questions answered. Each agentic fallback makes multiple API calls, so ~20-30 fallback questions consumed the entire daily quota. The run was killed and restarted on Vertex AI, which has per-minute (not per-day) rate limits.

## Key Findings

### 1. GPT-5.2 Primary is 6x Faster but -6 Questions

| Run | Score | Time | Cost |
|-----|-------|------|------|
| Phase 9 (Gemini primary) | 472/500 | 3h48m | ~$30 |
| Phase 10 (GPT-5.2 primary) | 466/500 | 39 min | ~$55 |
| GPT-5.2 standalone | 453/500 | 10 min | ~$49 |

The 39 min runtime (vs 3h48m) enables faster iteration, but the -6 regression from Phase 9 means GPT-5.2 primary is not a free speed upgrade.

### 2. Category Strengths Predict Ensemble Direction

The -6 came from Temporal (-3) and Updates (-3) --- exactly the categories where Gemini is stronger. The fallback mechanism rescues abstentions and loop-breaks but cannot compensate for GPT-5.2's weaker primary answers on temporal reasoning and update tracking.

### 3. Fallback is High-Value but Direction-Dependent

76 fallbacks at 88.2% accuracy means the ensemble adds ~13 correct answers over what GPT-5.2 alone would get (453 standalone vs 466 ensemble = +13). But the question is which model should be primary. When GPT-5.2 is primary, it gets Temporal/Updates wrong as primary answers (no fallback triggered), and fallback only fires on abstentions/loop-breaks.

### 4. Generative Language API is Unusable for Benchmarks

The 250 RPD daily cap makes Google AI Studio API keys impractical for benchmark workloads with agentic (multi-call) patterns. Vertex AI with service account auth is the only viable path for Gemini at benchmark scale.

## Next: Inverted Ensemble (Gemini Primary + GPT-5.2 Fallback)

The oracle data and category analysis both point to the same conclusion: **Gemini should remain primary** for its Temporal/Updates strength. But the fallback model should be **GPT-5.2** instead of GPT-4o --- GPT-5.2 is strictly better (453 vs 442 standalone, 30/30 vs 24/30 abstention).

Expected improvement over Phase 9 (472): GPT-5.2 as fallback rescues more of Gemini's abstention failures and loop-breaks than GPT-4o did. The P22 ensemble (Gemini + GPT-4o) scored 467; Phase 9 added code fixes to reach 472. Gemini + GPT-5.2 with the same code fixes should be >= 472.

Downside: back to ~3h48m runtime.
