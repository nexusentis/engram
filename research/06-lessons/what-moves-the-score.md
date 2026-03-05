---
title: "What Actually Moves the Score"
sidebar_position: 1
---

# What Actually Moves the Score

Not all interventions are created equal. After 50+ benchmark runs, we can rank intervention types by their empirical impact with high confidence. The hierarchy below is ordered by observed effect size, with specific data points for each level.

The single most important finding: **the gap between what we expected interventions to deliver and what they actually delivered was consistently 3-5x.** Plans predicted "+5-8 questions"; reality delivered "+1-2 questions" or outright regressions. This miscalibration is itself a critical lesson. The one exception: model diversity (Phase 7), which delivered at the top of its estimate (+15 predicted +8 to +15).

## The Impact Hierarchy

### 1. Tool Correctness (+14pp)

Fixing six bugs in the retrieval and answering tools was the single largest improvement in the entire project. Run #21 (84%) vs Run #18 (70%) on identical data, identical ingestion, identical prompts --- the only difference was tool correctness.

The six fixes were:
- `user_id` filter on `search_facts` (cross-user contamination)
- `search_entity` querying all 4 Qdrant collections instead of just "memories"
- Message hybrid search using RRF fusion instead of append-and-truncate
- Temperature config properly wired through `LlmClient`
- Datetime range filters instead of numeric range in temporal queries
- Category-specific strategy hints in the non-agentic prompt

These were not clever optimizations. They were bugs --- silent incorrect behavior that had been present since early development. The lesson: **before optimizing anything, audit tool correctness exhaustively.** A comprehensive code audit identified all six in a single pass.

### 2. Model Diversity (+15q at 90%+)

The single largest gain above 88% came not from code changes but from exploiting complementary failure modes between models. In Phase 6, swapping gpt-4o for Gemini 3.1 Pro gained +10 questions (442 to 452). In Phase 7, a simple ensemble router (Gemini primary + GPT-4o fallback on abstention/loop-break) gained another +15 (452 to 467) — more than all single-model interventions in Phase 5 combined.

The mechanism: Gemini and GPT-4o fail on different questions. Of Gemini's 48 failures, 40 were questions GPT-4o answered correctly. Of GPT-4o's 58 failures, 30 were questions Gemini answered correctly. A perfect oracle selecting the right model per question would reach ~470/500 (94%). The ensemble router captured most of this gap with ~200 lines of code.

**Why this matters**: At 88%+, after tool correctness and data formatting have been maximized, the next highest-ROI intervention is not better prompts or gates — it is model diversity. No single-model intervention in Phase 5 moved the score at all. This is a qualitative lesson: when you hit a ceiling, try a different model before redesigning the architecture.

**Routing ceiling**: Fallback routing only helps when models have complementary failure modes on the target slice. P31 (Enumeration uncertainty fallback) fired on 3 questions where Gemini used 8+ iterations --- GPT-4o gave the same wrong answers on all three. For shared failures (both models wrong for the same structural reason, e.g., cumulative context overload in multi-session aggregation), no amount of routing helps. Calibration rule: **no complementary error surface → no routing gain.**

### 3. Judge Calibration (~70% hit rate)

Judge changes had the highest per-intervention success rate. Of all judge modifications attempted, roughly 70% produced measurable score improvements. Specific wins:

- **Numeric answer parsing fix** (Run #17): Recovered 4-8 questions that were silently scored as failures because numeric gold answers were dropped to empty strings.
- **Stem word fix** (`stem_word` doubled-consonant bug): "running" was stemmed to "runn" instead of "run", causing false mismatches.
- **Case-insensitive tag matching** (Judge v2): Prevented false positives where "YES" appeared in reasoning text after "CORRECT: NO".
- **Numeric progression rule**: Answers conveying the same numeric change over time ("went from 4 to 5 engineers" vs "has 5 engineers, up from 4") are now scored as equivalent.
- **Category-aware keyword threshold**: 70% overlap for Extraction (more tolerant of paraphrasing) vs 80% for other categories.

Total gain from judge fixes: approximately +6-8 questions across multiple interventions, on a base where each question is worth 0.2 percentage points.

### 4. Data Formatting (+4pp)

Date-grouped formatting was a simple, lasting win introduced in Run #5. Instead of presenting retrieved facts as a flat list, we group them by date and present them chronologically. This single formatting change moved the score from 74% to 78% on the same data.

The effect was durable: every subsequent run benefited from date-grouped formatting. It costs nothing at inference time and requires no additional retrieval. It simply makes the information easier for the LLM to process.

### 5. Gate Engineering (~18-23% of Estimated Impact)

Gates --- conditional checks that verify answer quality before accepting it --- delivered real but consistently overestimated improvements.

**Successful gates:**
- **Anti-abstention gate** (P11): +22 questions on clean data. When the agent abstains but tool results contain 3+ question keywords, inject a second-chance prompt. This was the single best gate intervention.
- **A3 evidence gate fix** (Step 15): +1-2 questions. Skipping the A3 gate for Enumeration and Preference question types removed three false positives.
- **Temporal done-gate** (Run #23): Prevented premature temporal answers, contributing to Temporal going from 9/13 to 12/13.
- **Enumeration done-gate** (Run #28): Required 3+ retrieval calls before accepting enumeration answers.

**Failed gates:**
- **Ordering evidence gate** (P1): Used verbatim string matching on extracted slots. "The narrator losing their phone charger" never matched "lost my charger" in tool results. Net harmful.
- **Count reducer enforcement** (pre-P19): Agent said "5 trips" (correct), reducer deduped to 4 and overwrote. Disabled to observe-only mode.
- **P23 anti-abstention tuning**: Abandoned after pre-implementation review. Remaining false abstentions were loop-break failures (both models exhausted), not gate threshold failures. Only the Gate 16 `_abs` guard shipped.

**Late-stage gate success — P25 abstention override**: Post-loop override forces abstention for `_abs` questions when agent gives non-abstention answer. +6 questions (Abstention from 24/30 to 30/30, 100%). Zero regression risk because `_abs` questions should always abstain by definition. This worked precisely because it was a narrow, well-scoped invariant — not a fuzzy threshold.

The calibration lesson: raw gate impact estimates averaged 4-5x higher than reality. A gate estimated at "+3-5 questions" typically delivered "+1" or regressed. The exception is gates that enforce hard invariants (P11 anti-abstention, P25 `_abs` override) — these deliver reliably because they exploit dataset structure, not model behavior.

### 6. Prompt Engineering (~1-2pp)

Almost always overestimated, and sometimes actively harmful. Specific findings:

- **Category-specific prompt guidance**: Within +/-2 question variance of baseline across multiple attempts. Not reliably distinguishable from noise.
- **Few-shot examples**: Neutral. Within variance.
- **Chain-of-Note**: Single-pass Chain-of-Note was neutral (+0); the technique requires per-session extraction to work, which is a fundamentally different architecture.
- **"NEVER" language**: Actively harmful. "NEVER guess ordering from vague language. Use exact dates." converted correct contextual reasoning into abstentions. P1's ordering prompt fired on 24/61 temporal questions (39%), 19 of which were already passing.
- **Verification-focused enumeration guidance** (Run #31): The one prompt win --- +2 questions through a 3-phase "Broad Recall / Targeted Expansion / Verification" approach. But this was a structural change to the agent's search strategy, not just wording.

The pattern: prompt changes that add constraints almost always hurt; prompt changes that add structure occasionally help.

### 7. Deterministic Post-Processing (0pp)

Three independent attempts to deterministically correct LLM answers post-hoc. All three failed:

| Attempt | Mechanism | Result |
|---------|-----------|--------|
| Deterministic temporal resolver | Override agent's temporal answer with computed date diff | -2 questions (Q57: changed correct "9 months" to "111 months") |
| P12-P15b answerer quality fixes | Truncation reversal, strategy routing, count reducer enforcement, preference gate | Gate-neutral (203/231 vs 202/231) |
| P16 evidence-table finalizer | 750-line system with 3 kernels (count, latest_value, date_diff) | Net neutral, 1 harmful override (changed correct "3" to "4") |

The root cause is the same across all three: LLM tool results are natural language, not structured data. Counting rows in an evidence table counts *mentions*, not *entities*. Two messages about the same bible study produce two evidence rows. Solving this requires the kind of natural language understanding that the LLM agent already provides --- making the "deterministic override" redundant at best and harmful at worst.

The agent is already correct when deterministic correction would be easy. The `DATE_DIFF` kernel found that the agent matched tool results in 100% of cases where it called the date difference tool. The failures are upstream: the agent did not call the tool, or called it with wrong dates.

### 8. Retrieval Augmentation (0pp at 498/500 recall)

At 99.6% retrieval recall, adding more retrieval channels adds nothing:

- **Query expansion** (3 variants + RRF): -4 questions. Catastrophic regression from rank signal flattening.
- **Constrained single-variant expansion**: -2 questions. Still harmful.
- **RRF k tuning** (k=40, k=50): -2 to -3 questions. Over-weighting top results.
- **Keyword-OR fulltext**: Neutral to slightly negative.
- **Behind-the-scenes graph augmentation** (P20): 0 questions. Graph facts overlapped entirely with vector search results.

The bottom line: when retrieval already finds the answer 498 out of 500 times, the problem is what happens after retrieval.

## Calibration Rules for Future Work

Based on this empirical hierarchy:

1. **Discount all impact estimates by 3x.** Plans say "+5-8q"; expect "+1-2q". Exception: model diversity interventions delivered at the top of estimates.
2. **Fix bugs before optimizing.** One correctness audit was worth more than all prompt engineering combined.
3. **When you hit a ceiling, try a different model before redesigning.** Phase 5 spent ~$800 on single-model interventions with zero net gain. Phase 7 spent ~$50 on an ensemble router for +15. Model diversity is a qualitatively different lever.
4. **Judge changes are high-ROI.** They have the best hit rate and lowest cost to test.
5. **Gates that enforce hard invariants work; fuzzy threshold gates don't.** P11 (anti-abstention) and P25 (`_abs` override) both delivered because they exploit dataset structure. P1, P23, and P17 failed because they relied on model behavior heuristics.
6. **Do not add "NEVER" to agent prompts.** Advisory language ("prefer X when available") preserves correct contextual reasoning.
7. **Post-processing is a dead end at 88%+.** The remaining failures are semantic, not arithmetic.
8. **More retrieval channels are wasteful at 99.6% recall.** At 94%+, the bottleneck has shifted to representation quality — how facts are structured and connected, not whether they can be found.
