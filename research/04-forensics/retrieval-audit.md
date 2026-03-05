---
title: Retrieval & Code Audit
sidebar_position: 3
---

# Retrieval and Code Audit

Early in the project, we conducted a systematic audit of the entire retrieval and answering pipeline. The goal was to move beyond symptom-level analysis ("this question failed") to root-cause analysis ("this code path produces incorrect behavior"). The audit examined ingestion, storage, retrieval, tool implementation, context construction, and judging.

The audit found 6 correctness bugs. Fixing them produced the single largest accuracy improvement in the project's history: +14 percentage points, from ~74% to ~88%. This dwarfed every subsequent prompt engineering, gate tuning, or post-processing intervention combined.

## Audit methodology

The audit was conducted in two passes:

**Pass 1: Independent analysis.** Two independent code reviews audited the codebase, each starting from the same data: a 37/50 non-agentic run with per-question traces. Each produced a ranked list of findings with code references and expected impact estimates.

**Pass 2: Cross-review.** Each analysis was reviewed against the other's findings. Agreement between the independent analyses increased confidence; disagreements were investigated by reading the code directly.

This two-pass cross-review process proved valuable. Several findings were identified in one review and confirmed in the other, while a few false positives were caught where one analysis contradicted the other's code reading.

## The 6 correctness bugs

### 1. User ID filter missing from fact search

**File:** `tools.rs:255-260`

The `search_facts` tool filtered on `observation_level` but not `user_id`. In a multi-user benchmark, this meant the agent could retrieve facts belonging to other users -- facts about different people's jobs, locations, preferences, and experiences. The message search tools had user scoping (added in Run 18), but the fact search did not.

**Impact:** Cross-user contamination in the evidence pool. The agent might find "Rachel moved to Chicago" from User A and present it as the answer for User B, who has a different Rachel. This directly caused wrong-entity and wrong-value failures across multiple categories.

**Fix:** Add `user_id` filter to `search_facts` tool, matching the scoping already present in message tools.

### 2. Broken `search_entity` collection target

**File:** `tools.rs:521`

The `search_entity` tool queried a collection called `"memories"`, but the storage layer defined fact collections as `world`, `experience`, `opinion`, and `observation` (in `qdrant.rs:15`). No `"memories"` collection existed. The tool would silently return no results for entity-based fact retrieval, forcing the agent to rely entirely on semantic search, which is less precise for entity lookups.

**Impact:** An entire retrieval channel was non-functional. The agent had a tool that appeared to work but produced no useful output.

**Fix:** Route `search_entity` through the same collection resolution as other fact tools.

### 3. Message hybrid fusion was append-then-truncate

**File:** `qdrant.rs:574-597`

The `search_messages_hybrid` function appended fulltext results after vector results and then truncated to `top_k`. When vector search filled the `top_k` budget (which it typically did), fulltext results were discarded entirely. This meant exact-match retrieval for names, numbers, and specific phrases was effectively disabled despite appearing to be implemented.

**Impact:** The system advertised hybrid search but delivered vector-only search in practice. This particularly hurt temporal and multi-session questions where exact entity names needed to match.

**Fix:** Implement proper reciprocal rank fusion (RRF) that interleaves vector and fulltext results by rank before truncation.

### 4. Temperature inconsistency between agentic and non-agentic paths

**Files:** `answerer.rs:2337-2340` (agentic), `answerer.rs:2238-2239` (non-agentic)

The agentic tool-calling path used temperature 0.1, while the non-agentic single-pass path used temperature 0.0. This introduced unnecessary variance in agentic search behavior: the agent's tool call planning was non-deterministic between runs, making it harder to reproduce results and diagnose failures.

**Impact:** Run-to-run variance in agent behavior. The same question could produce different tool call sequences across runs, not because of different data but because of sampling randomness.

**Fix:** Align agentic temperature to 0.0 for benchmark runs.

### 5. Temporal filter using wrong comparison type

**File:** `temporal_filter.rs:41-58`

The retrieval-layer temporal filter used numeric `Condition::range` against `t_valid` datetime payloads, while the tools path correctly used `Condition::datetime_range` (in `tools.rs:435-447`). Depending on how Qdrant interpreted the mismatched condition type, this could either over-filter (excluding valid facts) or under-filter (including irrelevant facts).

**Impact:** Temporal queries through the retrieval layer could miss valid facts or include noise. The tools path was unaffected because it used the correct condition type.

**Fix:** Use `datetime_range` consistently across all temporal filtering paths.

### 6. Strategy hints not reaching the agent

**File:** `answerer.rs` (multiple locations)

The question classification system (`QuestionStrategy`) correctly identified question types (Update, Temporal, MultiSession, etc.) and had strategy-specific logic, but several strategy hints were not propagated to the agent's system prompt or tool configuration. For example, Update questions were supposed to receive recency-biased retrieval, but the strategy routing had gaps where certain phrasings of update questions mapped to the Default strategy instead.

**Impact:** Strategy-specific optimizations (A2 update gate, recency gate, temporal scaffolding) did not fire for a subset of questions that needed them.

**Fix:** Audit strategy routing and ensure all question types reach their intended code paths.

## Aggregate impact

The combined effect of fixing these 6 bugs was approximately +14 percentage points. This was measured across multiple runs with controlled data:

| State | Accuracy | Notes |
|-------|----------|-------|
| Pre-audit (best non-agentic) | 37/50 (74%) | 50-question subset |
| Post-audit (agentic, I-v10) | ~44/50 (~88%) | Same subset, all fixes applied |
| Post-audit (full 500q, I-v10) | 442/500 (88.4%) | Full benchmark, R-T5 |

The +14pp gain from bug fixes exceeded the combined impact of every subsequent intervention: P11 anti-abstention gate (+4.4pp on temporal), P12-P15b answerer quality fixes (net neutral), P16 evidence-table finalizer (neutral), P17 evidence arrays (harmful), P18 graph tools (harmful), P20 graph prefetch (neutral).

This established a foundational lesson for the project: **tool correctness dominates prompt engineering by an order of magnitude.** The 6 bugs were not subtle edge cases -- they were fundamental correctness issues (querying the wrong collection, missing tenant isolation, broken fusion) that no amount of prompt tuning could compensate for.

## The cross-review

The two independent analyses agreed on the top findings:

**Strong agreement:**
- User ID scoping was the highest-priority fix (both ranked it first or second)
- Message hybrid fusion was broken (both identified append-then-truncate)
- `search_entity` collection mismatch (identified by one review, confirmed by the other)
- Ingestion non-determinism was a primary source of run-to-run variance

**Findings unique to one review:**
- Identified the temperature inconsistency between agentic paths (0.1 vs 0.0)
- Provided specific line references for the agentic loop termination conditions that caused premature stopping
- Quantified the head-to-head agentic vs. non-agentic category deltas, showing that agentic lost primarily on Temporal (-1.67/13) and gained only on Extraction (+0.67/15)

**Findings unique to the other review:**
- Identified the numeric answer parsing bug in the harness loader (`as_str()` dropping numeric gold answers)
- Proposed the priority ordering that became the actual execution plan

The cross-review process validated that independent analysis converges on the same structural issues, increasing confidence that the findings were genuine rather than artifacts of a single reviewer's biases.

## Retrieval recall: the P9 verification

After fixing the 6 bugs, we ran a dedicated retrieval recall audit (P9) to determine whether the remaining failures were caused by missing data or incorrect reasoning. P9 tested whether the relevant conversation sessions for each question existed in Qdrant and were findable by the retrieval pipeline.

**Result: 498/500 (99.6%) retrieval recall.**

Only 2 of 500 questions had relevant sessions that could not be retrieved at all. For the remaining 498, the data was in the database and reachable through the retrieval pipeline. This finding was consistent across both I-v10 (duplicated data) and I-v11 (clean data).

The P9 result closed the door on retrieval coverage as an optimization vector. With 99.6% recall, the ceiling for improvement through better retrieval is at most +2 questions. All remaining gains must come from how the agent searches (tool call strategy), reasons (temporal computation, aggregation), and calibrates confidence (abstention thresholds).

## Implications for the project trajectory

The audit findings established the project's strategic direction for everything that followed:

1. **Fix correctness first, tune behavior second.** The +14pp from bug fixes took approximately 3 days of engineering. The subsequent 6 weeks of gate engineering, prompt tuning, and post-processing interventions produced approximately +0pp net.

2. **Independent cross-review catches more bugs than single-pass analysis.** The two-pass audit process found issues that neither review would have found alone. This pattern was repeated in later forensics work.

3. **Retrieval is solved; answering is not.** At 498/500 recall, the data pipeline is delivering what it needs to. The remaining 58 failures at 88.4% are reasoning failures -- the agent has the evidence and cannot use it correctly.

4. **Tool correctness is a first-class engineering constraint.** Adding tools, modifying tool schemas, or changing tool behavior has outsized effects on agent performance. A broken tool (search_entity querying a nonexistent collection) silently degrades the entire system. Every tool must be tested for correctness independently of the agent that calls it.
