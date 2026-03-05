---
title: "Phase 7: Ensemble Router"
sidebar_position: 8
description: "452/500 → 467/500 — Gemini primary + GPT-4o fallback ensemble reaches #2 globally"
---

# Phase 7: Ensemble Router (P22)

**Period**: Week 3, Days 18-19
**Starting Score**: 452/500 (90.4%) with Gemini 3.1 Pro
**Ending Score**: 467/500 (93.4%) with Gemini + GPT-4o ensemble

## Context

Phase 6 revealed that Gemini 3.1 Pro and gpt-4o have **complementary failure modes**: Gemini fixes 40 of gpt-4o's failures but introduces 30 new ones, with 14 of those being false abstentions after heavy tool use. A perfect oracle combining both models would reach ~470/500 (94%). P22 was the highest-confidence intervention on the roadmap (85%, +8 to +13 expected) — a simple router that falls back to gpt-4o when Gemini gives up.

## The TOML-First Refactor

Before implementing the ensemble, we eliminated all model-specific code from Rust. Previously, model behavior was controlled by scattered `if model.contains("nano")` / `model.starts_with("gpt-5")` checks across 6+ files. The refactor moved everything to TOML config files:

- **`config/benchmark.toml`** — default config with all models and settings
- **`config/ensemble.toml`** — P22 ensemble: Gemini primary + GPT-4o fallback
- **`config/gemini-primary.toml`** — Gemini-only
- **`config/gpt4o-primary.toml`** — GPT-4o only

Key changes:
- New `benchmark_config.rs` — `BenchmarkConfig`, `ModelRegistry`, `ModelProfile` types
- `ModelRegistry` with fail-fast lookup (unknown models error at startup, not silently use defaults)
- Per-model auth via env var indirection (`token_cmd_env`, `api_key_env` in TOML reference env var **names**, never secrets)
- `BenchmarkConfig::load()` with env var overrides for backward compatibility
- Eliminated `with_env_components()`, replaced by `from_benchmark_config()`
- Zero model-specific branching in Rust — all routing through `ModelProfile` fields

This meant switching models, tuning thresholds, or configuring ensemble routing required editing a TOML file, not recompiling.

## Ensemble Implementation

The ensemble router is straightforward:

1. Run Gemini 3.1 Pro as primary answerer
2. When Gemini abstains, hits the duplicate-loop break, or exceeds the per-question cost limit ($0.50), route to gpt-4o as fallback
3. Use gpt-4o's answer if it provides one; otherwise keep Gemini's abstention

Critical bug discovered during testing: the fallback `LlmClient` was sending Gemini's model name (`google/gemini-3.1-pro-preview`) to OpenAI's API, which rejected it as an invalid model ID. Fixed by adding a `model_name` field to `LlmClient` — each client now knows its own model name, and the agentic loop uses `llm_client.model_name()` instead of `self.config.answer_model`.

### Configuration (`config/ensemble.toml`)

```toml
[ensemble]
enabled = true
primary_model = "google/gemini-3.1-pro-preview"
fallback_model = "gpt-4o"
fallback_on_abstention = true
fallback_on_loop_break = true
max_fallback_attempts = 1
```

## Testing Progression

### Targeted Test (24 questions) — Day 18

First ran the ensemble on 24 questions where Gemini was known to abstain from the Phase 6 checkpoint.

**Result: 22/24 (91.7%)**

- Ensemble fallback fired 3 times
- Q88432d0a: Gemini cost-limited → GPT-4o → correct (5 baking activities)
- Qgpt4_fa19884d: Gemini iteration-exhausted → GPT-4o → correct (bluegrass band)
- Q37f165cf: Gemini iteration-exhausted → GPT-4o → incorrect (both models failed)

This validated the router logic and the model_name fix.

### Fast Loop (60 questions) — Day 18

**Result: 57/60 (95.0%)** — baseline was ~53-54/60

| Category | Score |
|----------|-------|
| MultiSession | 14/15 (93.3%) |
| Abstention | 5/5 (100%) |
| Updates | 8/8 (100%) |
| Extraction | 16/17 (94.1%) |
| Temporal | 14/15 (93.3%) |

+3-4 over baseline. Strong positive signal — proceeded to Truth.

### Truth Run (500 questions) — Day 19

**Result: 467/500 (93.4%)**

| Category | Ensemble | Gemini-only | gpt-4o only | Ensemble Delta |
|----------|----------|-------------|-------------|----------------|
| Extraction | 147/150 (98.0%) | 143/150 (95.3%) | 140/150 (93.3%) | **+4** |
| MultiSession | 111/121 (91.7%) | 101/121 (83.5%) | 103/121 (85.1%) | **+10** |
| Updates | 68/72 (94.4%) | 66/72 (91.7%) | 64/72 (88.9%) | **+2** |
| Temporal | 117/127 (92.1%) | 118/127 (92.9%) | 110/127 (86.6%) | -1 |
| Abstention | 24/30 (80.0%) | 24/30 (80.0%) | 25/30 (83.3%) | 0 |

**+15 over Gemini alone, +25 over gpt-4o alone.**

### Ensemble Fallback Stats

| Category | Fallbacks Fired | Correct | Wrong | Hit Rate |
|----------|----------------|---------|-------|----------|
| MultiSession | 13 | 10 | 3 | 76.9% |
| Temporal | 6 | 4 | 2 | 66.7% |
| Extraction | 4 | 1 | 3 | 25.0% |
| Updates | 1 | 1 | 0 | 100% |
| **Total** | **24** | **16** | **8** | **66.7%** |

The biggest gains came from MultiSession (+10), where Gemini's false abstentions on aggregation questions were rescued by gpt-4o. Extraction fallbacks had a low hit rate — 3 of 4 extraction fallbacks failed, suggesting those questions are genuinely harder (preference recall questions where neither model succeeds).

### Run Details

- **Duration**: 15,410 seconds (~4h 17m) at concurrency 5
- **429 rate limit errors**: Zero unrecoverable (lower concurrency eliminated the issue)
- **Token auto-refresh**: Working correctly (proactive at 45min, reactive on 401)
- **Estimated cost**: ~$35-40 (Gemini primary ~$30 + GPT-4o fallback on 24 questions ~$5-10)

## The 33 Remaining Failures

| Category | Failures | Key Patterns |
|----------|----------|-------------|
| MultiSession | 10 | Aggregation off-by-one (5), false abstention (2), wrong value (3) |
| Temporal | 10 | Date diff errors (4), ordering (2), time-anchored (2), other (2) |
| Abstention | 6 | False positive — entity conflation (3), answers "0" (2), hallucination (1) |
| Updates | 4 | Stale value (2), off-by-one (1), wrong store (1) |
| Extraction | 3 | Preference recall (2), wrong cocktail (1) |

Notable shifts from the 48 Gemini-only failures:
- MultiSession dropped from 20 to 10 failures (GPT-4o rescued 10)
- Abstention stayed at 6 (same count, but ensemble didn't worsen it)
- 8 fallback failures are new — questions where Gemini abstained AND gpt-4o also failed

## Technical Changes (Committed)

| File | Change |
|------|--------|
| `benchmark_config.rs` (NEW) | `BenchmarkConfig`, `ModelRegistry`, `ModelProfile`, TOML loading, env overrides |
| `answerer.rs` | `LlmClient.model_name` field, ensemble fallback logic, `from_model_profile()` |
| `integration_benchmark.rs` | `BenchmarkConfig::load()` replaces ~200 lines of env parsing |
| `config/ensemble.toml` (NEW) | Ensemble config: Gemini primary + GPT-4o fallback |
| `config/benchmark.toml` (NEW) | Default config with all models |
| `config/gemini-primary.toml` (NEW) | Gemini-only config |
| `config/gpt4o-primary.toml` (NEW) | GPT-4o only config |

## Key Insight

**Simple model routing beats sophisticated single-model engineering.** Phases 5 and 6 proved that no single-model intervention could break past 452/500 — we tried 10+ approaches (graph tools, deterministic post-processing, gate engineering, strategy routing) and all were neutral or harmful. The ensemble router, with ~200 lines of new code, gained +15 questions. The lesson: when you've exhausted single-model improvements, exploit model diversity before changing architecture.

## Leaderboard Update

| Rank | System | Score |
|------|--------|-------|
| 1 | Mastra OM | 94.87% (474) |
| **2** | **Engram (P22 Ensemble)** | **93.4% (467)** |
| 3 | Honcho | 92.6% (463) |
| 4 | Hindsight | 91.4% (457) |
| 5 | Emergence | 86.0% (430) |

Jumped from #4 to #2, passing both Hindsight (+10) and Honcho (+4). Only 7 questions behind #1 Mastra OM.

## Data Files

- Config used: `config/ensemble.toml`

*Note: Phase 11 later replaced GPT-4o with GPT-5.2 as fallback, reaching 479/500 (95.8%) — #1 globally. See [Phase 11: Inverted Ensemble](./phase-11-inverted-ensemble).*
