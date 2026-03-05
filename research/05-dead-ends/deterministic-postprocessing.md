---
title: "Deterministic Post-Processing: A Dead End"
sidebar_position: 1
---

# Deterministic Post-Processing: A Dead End

We tried three times to deterministically correct agent answers after the agent finished its reasoning loop. All three failed. This page documents each attempt and explains why the approach is fundamentally flawed at 88%+ accuracy.

The core intuition was reasonable: the agent sometimes gets the right evidence but produces the wrong answer (off-by-one count, stale value, wrong date arithmetic). A deterministic post-processor could catch and fix these mechanical errors. In practice, the errors that remain at 88% are not mechanical.

## Attempt 1: The Resolver (FL-A8a, Day 12)

**What it did**: A deterministic override system (`DETERMINISTIC_RESOLVE=1`) that intercepted the agent's final answer and applied rule-based corrections. For temporal questions, it extracted dates and recomputed intervals. For update questions, it checked recency.

**Result**: -2 questions on Fast Loop (52/60 vs 55/60 baseline).

The resolver changed a correct temporal answer of "9 months" to "111 months" by misidentifying the date anchor. On update questions, it injected raw message snippets into the answer instead of extracting the value. The experiment was clear enough that the resolver was disabled immediately.

**Key data point**: With the resolver removed and all other fixes kept (FL-A8d), the score returned to 55/60, confirming the resolver was the sole cause of the regression.

| Run | Resolver | Score |
|---|---|---|
| FL-A8a | Enabled (v1) | 52/60 |
| FL-A8b | Baseline (no resolver) | 55/60 |
| FL-A8c | Enabled (fixed update path) | 53/60 |
| FL-A8d | Disabled (final) | 55/60 |

**Lesson**: Post-hoc overrides destroy correct contextual reasoning. The agent at 88% uses nuanced context to arrive at answers. A deterministic system that pattern-matches dates and applies arithmetic has no access to that context and will confidently produce wrong answers.

## Attempt 2: P12-P15b Answerer Quality Fixes (Day 14)

**What it did**: Four targeted fixes to the answering pipeline, each addressing a specific diagnosed failure mode:

- **P12 (truncation fix)**: The `BTreeMap` in `tools.rs` grouped results oldest-first. Combined with front-truncation in `answerer.rs`, this meant the newest evidence got cut. For Update questions, the latest value was systematically dropped. Fix: reverse the truncation to keep newest evidence.
- **P13 (strategy routing fix)**: Improved the A2 threshold for Update strategy detection and fixed routing edge cases where Update questions were misclassified.
- **P14 (preference gate)**: Enhanced preference-question prompts to push the agent toward personalized rather than generic answers.
- **P15b (count reducer enforcement)**: The count reducer detected when the agent's claimed count did not match the number of items it listed. Previously observe-only. P15b considered promoting it to enforce corrections.

**Result**: Gate-neutral. 203/231 vs 202/231 baseline (within stochastic variance of +/-20 questions).

The detailed analysis told a clear story: all four fixes worked correctly at a mechanical level. Truncation was reversed. Routing was improved. The preference prompt was better. But the Gate run showed 21 regressions and 20 fixes that canceled out perfectly. The fixes did not move the needle because the failures they targeted were not the failures they appeared to be.

The critical finding was about the count reducer. On the 5 parseable Enumeration questions where the reducer could extract both a claimed count and a listed count, the agent was "consistent" in all 5 cases -- the claimed count matched the listed items. The agent was not miscounting; it was finding the wrong items. Fixing the count does nothing when the count is already consistent with what the agent found.

**Lesson**: Aggregation failures are "agent finds wrong items" (a search strategy problem), not "agent miscounts found items" (a post-processing problem). Post-processing can only fix errors that occur after the evidence is gathered. At 88%, the errors occur during evidence gathering.

## Attempt 3: P16 Evidence-Table Finalizer (Day 15)

**What it did**: The most sophisticated attempt. 750 lines of code implementing three "kernels" that built a structured evidence table from the agent's tool call history and applied deterministic corrections:

1. **COUNT kernel**: Parsed evidence rows from tool results, filtered by keyword relevance (requiring 2+ keyword hits), deduplicated by content similarity, compared evidence count vs. the agent's claimed count. Would override on off-by-one undercounts (confidence 0.94).

2. **LATEST_VALUE kernel**: Extracted concrete values (dollar amounts, time patterns) from dated evidence groups. Detected when the agent's answer contained a value from an older date and a newer value existed. Would override with the newest value (confidence 0.93).

3. **DATE_DIFF kernel**: Compared the agent's stated number against the `date_diff` tool's computed result. Would override if they differed (confidence 0.92).

A pre-implementation review identified 5 issues (DATE_DIFF extracting year instead of diff, UTF-8 slicing panic risk, COUNT overcounting, LATEST_VALUE false positives, time pattern artifacts). All were fixed before the Fast Loop.

A 5-question smoke test ($1.05) caught a catastrophic bug: LATEST_VALUE matched the generic token "gallon" on a tanks-counting question and replaced the entire answer with raw evidence about "rainwater harvesting." This was fixed by restricting value extraction to dollar amounts and time patterns.

**Result**: FL-P16 scored 54/60 vs. 53/60 baseline. The +1 was stochastic noise.

Only 1 override fired across all 60 questions. The COUNT kernel changed "3 days" to "4 days" on a faith-activities question. It was wrong. Two evidence rows mentioned the same bible study on December 17th with different wording, and the kernel counted them as distinct items.

| Kernel | Fired on | Overrides | Correct | Harmful |
|---|---|---|---|---|
| COUNT | 14 Enumeration questions | 1 | 0 | 1 |
| LATEST_VALUE | 3 Update questions | 0 | -- | -- |
| DATE_DIFF | 14 Temporal questions | 0 | -- | -- |

The COUNT kernel's evidence counts were wildly noisy: it reported 28 evidence rows for a question where the agent claimed 5 items, and 14 rows for another 5-item claim. Evidence rows are raw conversation messages, not distinct items.

The LATEST_VALUE kernel never fired because restricting to dollar amounts and time patterns made it too narrow. The actual update failures involve locations (Chicago to suburbs) and entity names that cannot be extracted with regex patterns.

The DATE_DIFF kernel never overrode because in 100% of cases where the agent called the date_diff tool, it correctly used the result. The temporal failures are upstream: the agent either did not call the tool or called it with wrong dates.

**The finalizer was reverted.** Code preserved in git history for reference.

## Root Cause Analysis: Why Deterministic Post-Processing Fails at 88%+

The three attempts failed for different specific reasons but share a common root cause. The remaining 58 failures at 88.4% accuracy are semantic reasoning errors, not mechanical errors. Here is why deterministic correction cannot address them:

### 1. Evidence table rows are not items

Tool results contain raw conversation messages, search result snippets, and fact extractions. When you count keyword-matching rows, you count mentions, not distinct real-world items. Two messages about the same bible study produce 2 rows. Two sessions mentioning the same trip produce 2 rows. Solving this requires NLU-level entity resolution (fuzzy deduplication, coreference resolution), which is exactly what the LLM agent is already doing -- and doing better than any regex-based system could.

### 2. Correction kernels face an impossible tradeoff

A correction kernel needs to be both sensitive enough to fire on real errors and specific enough not to fire on correct answers. In practice:

- **Generic matching** (e.g., LATEST_VALUE v1 with any token) causes catastrophic false positives. The "rainwater harvesting" incident was caught in smoke testing; subtler false positives would not be.
- **Narrow matching** (e.g., LATEST_VALUE v2 restricted to dollar amounts and time patterns) never fires because the actual failures involve locations, entity names, and other values that cannot be extracted with patterns.

There is no middle ground. The signal-to-noise ratio of raw tool results is too low for deterministic correction without full natural language understanding.

### 3. The agent is already correct when correction would be easy

The DATE_DIFF kernel found that in 100% of cases where the agent called the date_diff tool, it correctly used the result. The tool produces an unambiguous number; the agent copies it into the answer. There is nothing to correct.

The failures are upstream: the agent did not call the tool, or called it with wrong dates, or did not find the right events in the first place. Post-hoc correction cannot fix decisions the agent made (or failed to make) during its reasoning loop.

### 4. Off-by-one correction is especially dangerous

The most common real count error is +/-1 (agent missed one item). But +/-1 is also the most common noise from evidence deduplication errors (two rows for one item, or one row spanning two items). A correction system cannot distinguish "agent missed one real item" from "evidence table double-counted one item" without understanding the items semantically.

## What This Rules Out

After three independent attempts spanning two weeks and approximately $90 in compute:

- Silent deterministic override of agent answers based on evidence parsing
- Counting evidence rows as a proxy for counting real-world items
- Generic token matching for stale-value detection
- Post-hoc numeric comparison for date_diff correction
- Any intervention that does not address agent search strategy (what queries it issues, what tools it calls)

The remaining failures need changes to how the agent searches and reasons, not to how its answers are formatted after the fact.
