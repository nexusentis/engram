# Phase 6: Model Upgrade & Multi-Model Analysis

**Period**: Week 3, Days 17-18
**Starting Score**: 442/500 (88.4%) with gpt-4o
**Ending Score**: 452/500 (90.4%) with gemini-3.1-pro-preview

## Context

After exhausting all retrieval-side and post-processing interventions (P12-P21), we pivoted to testing stronger answering models. The hypothesis: reasoning quality, not retrieval, is now the bottleneck.

## Models Tested

| Model | Test Set | Score | Cost/500q | Latency/call |
|-------|----------|-------|-----------|------------|
| gpt-4o (baseline) | 500q Truth | 442/500 (88.4%) | ~$106 | ~1.8s |
| gpt-5.2 | 58 failures | 40/58 (69.0%) | ~$126 est. | ~1.8s |
| gemini-3-pro-preview | 20q (quota limit) | 14/20 (70.0%) | ~$30 est. | ~25s |
| **gemini-3.1-pro-preview** | **500q Truth** | **452/500 (90.4%)** | **~$30** | **~25s** |

## Gemini 3.1 Pro Truth Run Details

- **Run time**: 17,116 seconds (~4h 45m)
- **Concurrency**: 7
- **429 retries**: 4,068 (99.8% recovery rate)
- **Fatal errors**: 9
- **Token refreshes**: ~8 (auto-refresh via token command)
- **Infrastructure**: Vertex AI with OAuth auto-refresh (implemented during this phase)

### Category Breakdown

| Category | Gemini 3.1 Pro | gpt-4o | Delta |
|----------|---------------|--------|-------|
| Extraction | 143/150 (95.3%) | 140/150 (93.3%) | **+3** |
| Temporal | 118/127 (92.9%) | 110/127 (86.6%) | **+8** |
| Updates | 66/72 (91.7%) | 64/72 (88.9%) | **+2** |
| Abstention | 24/30 (80.0%) | 25/30 (83.3%) | -1 |
| MultiSession | 101/121 (83.5%) | 103/121 (85.1%) | -2 |

### Movement Analysis

| Movement | Count |
|----------|-------|
| Fixed by Gemini (gpt-4o failed → passed) | 40 |
| Regressions (gpt-4o passed → failed) | 30 |
| Persistent failures (both fail) | 18 |
| Net improvement | +10 |

### Regression Patterns (30 questions)

- **14 false_abstention**: Gemini says "I don't have enough info" on questions gpt-4o answered correctly. Most occur after heavy tool use (16/30 had ≥15 tool calls, 4 hit iteration cap).
- **11 wrong_value**: Gemini finds wrong items/counts (100 rare items instead of 99, Walmart instead of Thrive Market, stale Instagram followers).
- **5 false_positive (abstention)**: Gemini answers when it should abstain, confusing similar entities (guitar/violin, tennis/table tennis, baseball/football).

### Independent Review Key Findings

1. **Not "giving up early"** — most Gemini abstentions happen after 15+ tool calls. It's over-conservative finalization, not lazy retrieval.
2. **Prompt/gate interaction** — our abstention-biased prompts (tuned for gpt-4o) amplify Gemini's natural conservatism.
3. **Simple ensemble routing** — routing to gpt-4o when Gemini abstains yields 465-467/500 (93.0-93.4%).

## Technical Changes

### Token Auto-Refresh (Committed)
- API key stored with interior mutability (`Arc<Mutex<String>>`) for token refresh
- Configurable token command: shell command to refresh OAuth tokens
- Proactive refresh at 45 minutes, reactive refresh on 401
- 12 retries with exponential backoff capped at 60s

### Gemini Compatibility (Committed)
- `raw_json` field on `ToolCall` for Gemini thought signature preservation
- Model detection for `max_tokens` vs `max_completion_tokens`
- Configurable API key and base URL env vars for non-OpenAI providers
- UTF-8 safety fixes for multi-byte characters

## Key Insight

**Different models have fundamentally different failure modes.** Gemini fixes 40 questions gpt-4o gets wrong (especially Temporal +8, MultiSession +12) but introduces 30 new failures. An ensemble of both models could theoretically reach 470/500 (94%). This opens a new class of interventions: model-aware routing and multi-model consensus.

## Data Files

- Failure analysis: 48 failures classified by category and root cause
- Movement data: per-question comparison of Gemini vs GPT-4o results

## Leaderboard (end of Phase 6)

| Rank | System | Score |
|------|--------|-------|
| #1 | Mastra OM | 94.87% |
| #2 | Honcho | 92.6% |
| #3 | Hindsight | 91.4% |
| **#4** | **Engram (Gemini 3.1 Pro)** | **90.4%** |
| #5 | Emergence | 86.0% |

## What Came Next

The complementary failure modes discovered here led directly to [Phase 7: Ensemble Router](./phase-7-ensemble), where a simple Gemini+GPT-4o fallback router gained +15 questions (452→467), jumping Engram to #2 globally.
