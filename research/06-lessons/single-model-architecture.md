---
title: "Single-Model Architecture: Path Beyond Ensemble"
sidebar_position: 5
---

# Single-Model Architecture: Path Beyond Ensemble

## Why Single-Model Matters

The P22 ensemble router reached 467/500 (93.4%), Phase 9 quick wins pushed to 472/500 (94.4%), and Phase 11 (Gemini+GPT-5.2 ensemble) reached 479/500 (95.8%) --- #1 globally, surpassing Mastra OM (94.87%). But ensemble routing has fundamental limitations:

1. **Cost**: Every fallback question costs 2x (run Gemini, fail, run GPT-4o). At scale, this doubles the API budget for the hardest questions.
2. **Complexity**: Two model profiles, two authentication systems, fallback logic, timeout handling, model-specific prompt tuning.
3. **No competitor uses multi-model**: Mastra OM (94.87%), Honcho (92.6%), and Hindsight (91.4%) all use single-model architectures. Multi-model is not the path the field is taking.
4. **Shared failures**: 8 of 21 remaining failures (Phase 11) are shared across all three models. These need architecture changes, not more model routing.

The strategic question: can a single-model Engram maintain #1 through better representation, reducing dependence on multi-model?

## Competitor Architecture Comparison

| Dimension | Mastra OM (94.87%) | Honcho (92.6%) | Hindsight (91.4%) | Engram (95.8%) |
|-----------|-------------------|----------------|-------------------|----------------|
| **Core approach** | Compress sessions → observation logs → single LLM pass | Structured memory graph + reasoning trees | Entity graph + 4-way retrieval fusion | Atomic fact extraction + agentic search |
| **Storage** | None (context window only) | Graph database | Entity graph + vector store | Qdrant vector store |
| **Compression ratio** | ~50:1 (sessions → observations) | Unknown | Unknown | ~100:1 (messages → facts) |
| **Temporal model** | Implicit (observation order preserves time) | Unknown | Temporal edges in entity graph | `t_event` field extracted per fact |
| **Raw message access** | Yes (compressed but complete) | Via `get_observation_context` | No | No |
| **Provenance** | N/A (no extraction step) | Observation → source conversation link | Entity → source session | **None** (fact has no link to source message) |
| **Agent iterations** | 1 (single-pass, no tools) | Unknown | Unknown | Up to 10 (agentic loop with tools) |
| **Observation levels** | Multi-level (session summaries, topic clusters, entity profiles) | Multi-level (reasoning trees at different granularities) | Entity-centric (one level) | Partial (prefetch splits explicit vs deductive facts) |

### What This Table Reveals

Mastra's advantage is not a better model or better retrieval --- it is **better representation**. By compressing entire conversation histories into structured observation logs that preserve temporal order and entity relationships, Mastra gives the LLM everything it needs in a single pass. No tools, no iterations, no retrieval failures.

Honcho's edge comes from **provenance** --- the ability to trace an observation back to its source conversation for additional context. This is exactly what our agent cannot do when it finds a fact but needs surrounding context.

Hindsight's contribution is **temporal structure** --- explicit temporal edges in the entity graph that capture "before/after/during" relationships, not just timestamps.

## The Core Diagnosis

Two independent analyses of the full codebase, research history, and failure data converged on the same conclusion (written at Phase 9, 472/500):

> **Representation quality, not model diversity, is the limiting factor beyond 95%.**

*Note: Phase 11 disproved the "not model diversity" part — GPT-5.2 fallback gained +7 to reach 95.8%. But representation quality remains the path beyond 96%.*

The evidence:
- 8 of 21 remaining failures (Phase 11) are shared across all three models (representation problem, not reasoning)
- Retrieval recall is 498/500 (99.6%) --- we find the right facts, we just do not represent them well enough
- The former #1 system (Mastra OM) uses zero retrieval — we surpassed it in Phase 11 through model diversity alone
- Our fact extraction compresses 100:1 but loses temporal relationships, entity connections, and conversational context
- The agent has no holistic view of a user --- it searches atomically and assembles understanding from fragments

## The Five Architecture Gaps

### 1. No Holistic User Memory

**The gap**: Mastra generates per-user observation briefs at ingestion time --- multi-level summaries that give the LLM a complete picture of each user before answering. We have 282K atomic facts (avg ~12 facts per user) with no summary or aggregation layer.

**Impact**: For questions like "how many countries has user X visited?" the agent must find and count individual trip facts scattered across sessions. A user-level summary would have "countries visited: [list]" ready.

**Proposed fix**: Generate a per-user compressed memory brief at ingestion time. Inject as initial context at query time, before the agent begins searching.

### 2. Underused Temporal Metadata

**The gap**: We extract `t_event` (absolute timestamp) per fact, but lack relative temporal annotations. "User visited Paris" has a date, but there is no link to "this was during the Europe trip" or "this was after the job change."

**Impact**: 10 of 33 failures are temporal-category questions. Many require reasoning about event ordering or duration that cannot be derived from isolated timestamps.

**Proposed fix**: Three-date temporal model: `t_event` (when it happened), `t_context` (session/conversation time), `t_relative` (annotations like "after X", "during Y trip", "the weekend before Z").

### 3. No Fact-to-Message Provenance

**The gap**: When the agent finds a fact via `search_facts`, it gets the extracted text but cannot trace back to the original conversation message. Honcho's `get_observation_context` tool lets agents "zoom in" from a summary to the source dialogue.

**Impact**: The agent cannot verify context, find adjacent facts mentioned in the same conversation turn, or distinguish between similar entities that were discussed in different conversations.

**Proposed fix**: Store `(session_id, message_index)` provenance on each fact. Add an `expand_fact` tool that retrieves the surrounding conversation context for any fact. This is also a prerequisite for verifying Hybrid OM brief generation --- without provenance, we cannot confirm that summaries are grounded in actual extracted facts.

### 4. No Coverage-Adaptive Control

**The gap**: The agent uses the same search strategy regardless of how many results it has found. A question with 2 results needs broader search; a question with 200 results needs filtering and counting.

**Impact**: Under-search leads to false abstention (not enough evidence found). Over-search leads to wrong aggregation (agent overwhelmed by results, miscounts items).

**Proposed fix**: Coverage-adaptive controller that monitors result count after each search iteration and adjusts strategy: switch to broader queries when coverage is low, switch to deduplication/counting when coverage is high.

### 5. Observation Level Is Partially Used

**The gap**: The `observation_level` field exists and is used in the prefetch-level split (explicit vs deductive facts), but strategy selection is not adaptive end-to-end based on observation granularity. The agent does not distinguish between high-level summary facts and low-level detail facts when planning its search.

**Impact**: The agent may waste iterations searching for details when a summary-level fact already answers the question, or miss details when only summary-level facts are returned.

**Proposed fix**: Strategy-adaptive observation levels --- route to different search patterns based on the granularity needed by the question type.

## Ranked Interventions

### Tier 0: Quick Wins --- TRUTH_VALIDATED (472/500)

All committed and validated by Truth run (472/500, 94.4%). See [Phase 9 narrative](../journey/phase-9-architecture-review) for details.

| Item | Status | Actual Impact |
|------|--------|---------------|
| Judge fix (temporal total) | TRUTH_VALIDATED | +1 (deterministic) |
| P-NEW-C routing fix | TRUTH_VALIDATED | +1 (targeted) |
| P25 abstention override | TRUTH_VALIDATED | +6 Abstention (24/30 → 30/30) |
| P23 Gate 16 _abs guard | TRUTH_VALIDATED | optimization (no score impact) |

### Tier 1: Representation Changes (Single-Model Path)

These are the architecture changes that address the five gaps above. They form the path to 96%+ without multi-model.

#### 1. Provenance Links + `expand_fact` Tool

**Expected**: +2 to +4 | **Effort**: Medium | **Priority**: First (prerequisite for Hybrid OM)

Store `(session_id, message_index)` on each fact during extraction. Add an `expand_fact` tool that retrieves surrounding conversation context. This enables the agent to "zoom in" from a fact to its source, resolving entity disambiguation and temporal context questions.

Critical prerequisite for Hybrid OM: provenance links are needed to verify that generated briefs are grounded in actual facts, not hallucinated summaries.

#### 2. Hybrid Observation Memory Brief

**Expected**: +5 to +10 | **Effort**: Medium-High | **Priority**: Second (after provenance)

Generate per-user compressed memory briefs at ingestion time. Inject as initial context at query time. Combines Mastra's holistic user understanding with our scalable retrieval architecture.

Brief structure (per user):
- Key facts and preferences (extracted from fact store)
- Temporal arc (major events in chronological order)
- Entity relationships (people, places, activities)
- Session count and date range

#### 3. Three-Date Temporal Model

**Expected**: +2 to +4 | **Effort**: Medium

Extend the fact schema with `t_context` (conversation timestamp) and `t_relative` (temporal annotations). Requires extraction prompt changes and storage schema update.

#### 4. Coverage-Adaptive Controller

**Expected**: +1 to +3 | **Effort**: Medium

Monitor result count after each search iteration. Adjust strategy dynamically:
- Low coverage (0-3 results): broaden search terms, try synonyms
- Medium coverage (4-15 results): continue current strategy
- High coverage (15+ results): switch to counting/dedup mode, stop searching

### Tier 2: Exploration

These are lower-confidence ideas worth investigating but not committing to.

- **Graph-assisted entity disambiguation**: Use the existing graph data behind the scenes (not as agent tools --- that is a confirmed dead end from P18/P18.1) to disambiguate similar entities at retrieval time.
- **Honcho-style user profile card**: A lightweight alternative to full Hybrid OM --- a fixed-schema profile card per user (name, location, interests, key relationships) injected as context.
- **Increased iterations (10 to 20)**: May help for aggregation questions where the agent needs more search rounds. But Gemini already uses 15+ iterations on hard questions, so the limit may not be binding.

## Dead Ends Revisited with Gemini

### Might Work Now (Worth Retesting)

- **Model-aware anti-abstention thresholds**: P11's gates were tuned for gpt-4o. Gemini abstains at different thresholds. Model-aware flags (not global retune) could recover false abstentions.
- **Targeted graph disambiguation**: Not schema expansion (confirmed dead end), but using graph data behind the scenes to resolve "tennis vs table tennis" type ambiguities at retrieval time.
- **Strategy-aware second query family**: When the first search strategy returns poor results, automatically try a complementary strategy. Failed with gpt-4o due to gate interactions, but simpler implementation may work.

### Still Dead

- **Deterministic post-processing**: Three attempts failed (resolver, P12-P15b, P16). Evidence counting ≠ item counting. The agent is already correct when deterministic correction would be easy.
- **Global graph tool exposure**: Schema bloat causes regressions even when tools are never called (P18: -5 Gate). This is a fundamental LLM function-calling property, not fixable with prompt engineering.
- **Broad RRF/query-expansion retuning**: Dilutes precision. Dead at 88%, still dead at 93%.

### Already Tested, Currently Parked

These features exist in the codebase but are disabled in config because they showed no measurable benefit:

- **Session NDCG** (disabled in config): Session-level relevance scoring. No impact on benchmark score.
- **Temporal RRF** (disabled): Reciprocal rank fusion with temporal weighting. Adds noise without improving ordering.
- **Entity-linked retrieval** (disabled): Retrieve facts linked to recognized entities. Redundant with vector search at 99.6% recall.
- **Chain-of-Note** (neutral in single-pass mode): Emergence AI's approach. Tested but neutral when combined with our agentic loop.

## Risks & Gotchas for Hybrid OM

Four specific risks were identified for the Hybrid Observation Memory approach:

1. **Summary hallucination/staleness**: Without provenance verification, generated briefs may contain hallucinated facts or stale information. This is why provenance links are a prerequisite --- every claim in the brief must trace to an actual extracted fact.

2. **Token/cost blow-up**: If brief growth is not budgeted, per-user summaries could grow to thousands of tokens, consuming context window space that the agent needs for search results. Must enforce a token budget per brief (e.g., 500-1000 tokens).

3. **Abstention drift from over-confident brief injection**: If the brief says "user plays tennis" and the question asks about "table tennis", the agent may answer confidently from the brief instead of correctly abstaining. The brief must use precise entity language and flag uncertainty.

4. **Benchmark variance in the +1 to +3 band**: Gains of +1 to +3 questions are within stochastic noise (Gate variance is +/-20 questions across runs). Need a stricter validation protocol --- either multiple runs with confidence intervals, or a larger targeted test set.

## Independent Review Summary

An independent review of the full architecture analysis provided 7 findings:

**Areas of agreement**:
- Representation quality is the limiting factor, not model diversity
- Provenance links should be prerequisite for Hybrid OM (moved up in priority)
- Hybrid OM is the highest-ceiling single intervention

**Additional nuances identified**:
- `observation_level` is not unused --- the prefetch split already uses it. Reworded to "partially used, not strategy-adaptive end-to-end"
- Disabled retrieval variants (Session NDCG, Temporal RRF, entity-linked retrieval, Chain-of-Note) should be documented as "already tested, currently parked"
- Evidence-tier status tags (SHIPPED_CODE / FL_VALIDATED / TRUTH_VALIDATED) needed for clarity

**Flagged risks**:
- The four Hybrid OM risks listed above
- Phase 9 thesis ("representation quality") appears to contradict Phase 7 thesis ("model diversity") --- needs explicit bridge paragraph (added to Phase 9 narrative)

## Expected Ceiling

| Path | Expected Score | Range | Status |
|------|---------------|-------|--------|
| P22 ensemble | 467/500 (93.4%) | — | Achieved |
| + Tier 0 quick wins | 472/500 (94.4%) | — | TRUTH_VALIDATED |
| + Phase 11 (GPT-5.2 fallback) | 479/500 (95.8%) | — | **TRUTH_VALIDATED — #1 globally** |
| + Provenance links | ~481/500 (96.2%) | +1 to +3 | Proposed |
| + Hybrid OM brief | ~486/500 (97.2%) | +3 to +7 | Proposed |
| + Three-date temporal | ~488/500 (97.6%) | +1 to +3 | Proposed |
| Raw estimate | **481-491 (96.2-98.2%)** | | |
| **Discounted ceiling (rule #12: 1/3 of predicted)** | **481-483 (96.2-96.6%)** | | |

Phase 11 reached the top of the previously predicted discounted ceiling (479/500 = 95.8%) through model upgrade alone. Architecture changes now start from 479, not 472. The gap to Mastra OM is settled — we are #1 globally.
