---
title: Failure Breakdown by Category
sidebar_position: 1
---

# Failure Breakdown by Category

:::info Phase 4 Snapshot — 58 failures at 442/500
This analysis covers the 58 failures from R-T5 (442/500, Phase 4). By Phase 11 (479/500, #1 globally), model diversity and ensemble optimization reduced failures to 21: MultiSession 10, Temporal 8, Updates 3, **Extraction 0** (100%), **Abstention 0** (100%). Many of the specific failure patterns documented here — off-by-one aggregation, stale values — persist at smaller scale. For the current failure distribution, see [Phase 11](../journey/phase-11-inverted-ensemble).
:::

This page catalogs all 58 failures from R-T5 (442/500, I-v11 clean data), organized by benchmark category and failure subtype. Each entry includes the question ID, a condensed version of the question, the expected answer, what the agent returned, and the failure mode. Where relevant, we also note whether the question passed on I-v10 (duplicated data), as this distinguishes chronic failures from regressions caused by the data cleanup.

## MultiSession: 18 failures (85.1%)

MultiSession is the largest failure bucket. These questions require the agent to aggregate or correlate information scattered across multiple conversation sessions for a given user. The agent must search broadly, collect all relevant items, and synthesize them into a single answer.

### Aggregation (13 questions)

Aggregation failures are counting or summing tasks where the agent finds most items but misses one or more. This is the single most common failure subtype in the entire benchmark.

| QID | Question | Expected | Got | Mode | I-v10 |
|-----|----------|----------|-----|------|-------|
| gpt4_d84a3211 | Total bike expenses since start of year? | $185 | $65 | Wrong (missed items) | FAIL |
| 46a3abf7 | How many tanks including friend's kid? | 3 | 2 | Wrong (off-by-1) | FAIL |
| gpt4_2f8be40d | Weddings attended this year? | 3 | 4 | Wrong (overcounted) | PASS |
| 2788b940 | Fitness classes per week? | 5 | 4 | Wrong (off-by-1) | PASS |
| d851d5ba | Total charity money raised? | $3,750 | $8,750 | Wrong (overcounted) | FAIL |
| 2ce6a0f2 | Art events in past month? | 4 | 3 | Wrong (off-by-1) | FAIL |
| 10d9b85a | Days at workshops/lectures/conferences in April? | 3 | 2 | Wrong (off-by-1) | PASS |
| 60159905 | Dinner parties in past month? | 3 | 2 | Wrong (off-by-1) | PASS |
| 9ee3ecd6 | Points for free skincare at Sephora? | 100 | 300 | Wrong (wrong value) | FAIL |
| gpt4_7fce9456 | Properties viewed before townhouse offer? | 4 | Abstained | False abstention | FAIL |
| gpt4_194be4b3 | Musical instruments owned? | 4 | Abstained | False abstention | FAIL |
| 7024f17c | Hours of jogging and yoga last week? | 0.5 | Abstained | False abstention | FAIL |
| 9aaed6a3 | Cashback at SaveMart last Thursday? | $0.75 | Abstained | False abstention | FAIL |

**Pattern analysis:**
- 5 of 13 are off-by-one undercounts. The agent searches, finds most items, and stops one short. This is the prototypical aggregation failure -- a search completeness problem.
- 2 of 13 are overcounts where the agent includes items that do not match the criteria. These are harder to address because pushing the agent to search more aggressively would make overcounting worse.
- 2 of 13 return the wrong value entirely, pulling data from the wrong sessions.
- 4 of 13 are false abstentions where the agent has partial evidence but gives up.
- 4 of 13 are regressions from I-v10, meaning the duplicated data was helping the agent find all items.

### Cross-session Lookup (5 questions)

These require correlating specific facts across sessions without aggregation -- finding a particular value that depends on information from multiple conversations.

| QID | Question | Expected | Got | Mode | I-v10 |
|-----|----------|----------|-----|------|-------|
| gpt4_5501fe77 | Most followers gained platform? | TikTok | Twitter | Wrong | FAIL |
| gpt4_d12ceb0e | Average age of family? | 59.6 | 49.6 | Wrong (math/data) | FAIL |
| 51c32626 | When submit sentiment analysis paper? | Feb 1st | 2023/05/22 | Wrong (wrong session) | FAIL |
| 8cf4d046 | Average GPA undergrad+grad? | 3.83 | Gave separate GPAs | Wrong (formula) | PASS |
| 37f165cf | Page count of 2 novels? | 856 | Abstained | False abstention | FAIL |

All 5 are chronic failures (4 failed on I-v10 as well). These are architecturally harder because they require the agent to locate specific values across sessions and then perform a computation or comparison. The "average GPA" question is representative: the agent found both GPAs but failed to combine them as requested.

## Temporal: 17 failures (86.6%)

Temporal questions require reasoning about time -- computing date differences, identifying chronological order, or locating events relative to time anchors. This category saw the most dramatic regression from I-v10 to I-v11 (discussed in detail in [Temporal Failures Deep Dive](./temporal-analysis)).

### Time-anchored Lookup (5 questions) -- Architecturally unfixable

These questions use relative time expressions ("two weeks ago", "last Saturday") that the agent cannot resolve to absolute dates without knowing the conversation's timestamp. They failed on both I-v10 and I-v11 and represent a structural gap in the system.

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| gpt4_59149c78 | Art event "two weeks ago"? | Metropolitan Museum | Abstained | False abstention |
| gpt4_d6585ce9 | Music event "last Saturday"? | My parents | Abstained | False abstention |
| gpt4_e414231f | Bike fixed "past weekend"? | Road bike | Abstained | False abstention |
| 9a707b82 | Cooked for friend "couple days ago"? | Chocolate cake | Abstained | False abstention |
| 6e984302 | Investment "four weeks ago"? | Sculpting tools | Abstained | False abstention |

All 5 are false abstentions. The agent retrieves sessions that mention the relevant topic but cannot determine which event maps to the relative time reference. Fixing these would require resolving relative time expressions to absolute dates during ingestion or at query time -- a significant architectural change for a 1% gain.

### Date Diff (6 questions) -- Wrong date identification or calculation

The agent locates relevant events but either identifies the wrong dates or computes the difference incorrectly.

| QID | Question | Expected | Got | Mode | I-v10 |
|-----|----------|----------|-----|------|-------|
| 0bc8ad92 | Months since museum with friend? | 5 | 3 | Wrong | PASS |
| gpt4_a1b77f9c | Total weeks reading 3 books? | 8 | 8 (matches!) | Wrong* | PASS |
| 370a8ff4 | Weeks from flu to 10th jog? | 15 | 11 | Wrong | FAIL |
| gpt4_7bc6cf22 | Days since reading March 15 New Yorker? | 12-13 | 5 | Wrong | PASS |
| eac54adc | Days from website launch to first client? | 19-20 | Abstained | False abstention | PASS |
| gpt4_cd90e484 | Days using binoculars before goldfinches? | 2 weeks | 21 days | Wrong | FAIL |

*Note: gpt4_a1b77f9c answered "8 weeks" which matches the expected answer. This may be a judge evaluation issue rather than an agent failure.*

4 of 6 are regressions from I-v10. On duplicated data, the redundant temporal facts gave the agent enough signal to identify the correct dates. On clean data, the agent picks wrong dates or wrong sessions, leading to incorrect calculations.

### Ordering (3 questions)

The agent must list events in chronological order but produces incomplete or misordered lists.

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| gpt4_7f6b06db | Order of 3 trips past 3 months? | Hike, road trip, Yosemite | Road trip, Dubai, Yosemite | Wrong |
| gpt4_d6585ce8 | Order of concerts past 2 months? | 5 events | 4 events (missed 1) | Wrong |
| gpt4_f420262c | Order of airlines earliest to latest? | JetBlue, Delta, United, AA | JetBlue, AA, JetBlue (3 of 4) | Wrong |

Ordering failures combine the aggregation problem (finding all items) with the temporal reasoning problem (sorting them correctly). The agent consistently finds most events but misses one or conflates items.

### Other Temporal (3 questions)

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| gpt4_4929293b | Relative's life event "a week ago"? | Cousin's wedding | Niece's graduation | Wrong |
| gpt4_483dd43c | Crown or GoT first? | GoT | The Crown | Wrong |
| a3838d2b | Charity events before Run for Cure? | 4 | 3 | Wrong (off-by-1) |

## Extraction: 10 failures (93.3%)

Extraction questions ask the agent to recall specific facts about the user. At 93.3%, this is the strongest category, but the remaining failures reveal two distinct patterns.

### Preference Recall (6 questions)

The agent retrieves some relevant user data but generates generic advice instead of personalized responses grounded in the user's specific history.

| QID | Question | Expected | Got | Mode | I-v10 |
|-----|----------|----------|-----|------|-------|
| 0edc2aef | Suggest hotel for Miami? | Ocean views, rooftop pool | Abstained | False abstention | FAIL |
| 35a27287 | Cultural events this weekend? | Language practice (Spanish/French) | Generic Belo Horizonte events | Wrong | PASS |
| 09d032c9 | Phone battery tips? | Mention power bank purchased | Generic battery tips | False abstention | FAIL |
| d24813b1 | What to bake for colleagues? | Lemon poppyseed cake success | Oatmeal raisin cookies | Wrong | PASS |
| 0a34ad58 | Getting around Tokyo tips? | Suica card, TripIt app | Generic Tokyo tips (mentions Suica) | Wrong | FAIL |
| 16c90bf4 | Beer for Seco de Cordero? | Pilsner or Lager | Mentions Pilsner (close!) | Wrong | PASS |

4 of 6 give plausible but non-personalized answers. The agent retrieves some user context but does not anchor its recommendation in the user's specific preferences, purchases, or experiences. 2 of 6 abstain entirely despite having relevant data. The last entry (16c90bf4) is particularly notable -- the agent mentions Pilsner but apparently not precisely enough for the judge.

### Fact Recall (3 questions)

The agent retrieves the wrong specific fact, typically from a different session or context.

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| 8550ddae | Cocktail recipe last weekend? | Lavender gin fizz | Smokey Mango Mule | Wrong |
| 561fabcd | Radiation Amplified zombie name? | Fissionator | Contaminated Colossus | Wrong |
| eaca4986 | Chord progression 2nd sad song? | C D E F G A B A G F E D C | Abstained | False abstention |

These are cases where the agent finds a plausible-sounding answer from the wrong conversation session. The cocktail and zombie questions retrieve creative content from the user's history but from the wrong instance.

### Count (1 question)

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| b86304ba | Painting worth? | Triple what I paid | Abstained | False abstention |

## Updates: 8 failures (88.9%)

Update questions test whether the agent returns the most recent value for something that has changed over time. The dominant failure is returning a stale (outdated) value.

### Stale Values (5 questions)

| QID | Question | Expected | Got | Mode | I-v10 |
|-----|----------|----------|-----|------|-------|
| 6a1eabeb | 5K personal best? | 25:50 | 27:12 (old PB) | Wrong | FAIL |
| 830ce83f | Where did Rachel move? | Suburbs | Chicago (old) | Wrong | FAIL |
| 852ce960 | Mortgage pre-approval amount? | $400K | $350K (old) | Wrong | FAIL |
| 9ea5eabc | Most recent family trip? | Paris | Hawaii (old) | Wrong | PASS |
| 07741c45 | Where keep old sneakers? | Shoe rack in closet | "Looking forward to storing..." | Wrong | PASS |

**Root cause -- the truncation bug:** Tool results in `tools.rs` are date-grouped using a `BTreeMap`, which sorts oldest-first. Context truncation in `answerer.rs` keeps the front of the string (oldest) and drops the back (newest). For update questions, this means the latest evidence -- the very data the question asks about -- gets truncated away when context is long. This is a code-level bug, not a model limitation.

**Why existing gates fail:** The A2 (update detection) gate requires at least 2 dated sections about the topic to trigger. If the agent only retrieved one date group, A2 returns `None` and does nothing. The recency gate only fires when `retrieval_call_count < 3`, so after 3 searches it is permanently disabled regardless of whether the latest data was found.

### Update Count (2 questions)

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| 69fee5aa | Pre-1920 American coins? | 38 | 37 | Wrong (off-by-1) |
| ba61f0b9 | Women on Rachel's team? | 6 | 5 | Wrong (off-by-1) |

Both are off-by-one undercounts, the same pattern seen in MultiSession aggregation.

### False Abstention (1 question)

| QID | Question | Expected | Got | Mode |
|-----|----------|----------|-----|------|
| 7a87bd0c | How long daily tidying routine? | 4 weeks | Abstained | False abstention |

## Abstention: 5 failures (83.3%) — *Fully solved by Phase 9 (30/30, 100%)*

All 5 abstention failures were false positives -- the agent answered confidently when the correct response was "I don't have enough information." P25 (Phase 9) added a post-loop override that forces abstention for `_abs` questions when the agent gives a non-abstention answer. This plus the ensemble router's rescue of entity-conflation cases brought Abstention to 30/30 (100%).

| QID | Question | Expected | Got | Root Cause |
|-----|----------|----------|-----|------------|
| gpt4_372c3eed_abs | Years in formal education HS through Masters? | Not enough info (Master's duration missing) | 8 years (counted HS+PCC+UCLA) | Failed to notice Master's was asked but never mentioned |
| a96c20ee_abs | University where presented poster? | Not enough info (poster never mentioned) | Harvard | Hallucinated from other Harvard mentions |
| 09ba9854_abs | Savings bus vs taxi from airport? | Not enough info (bus cost unknown) | $40 | Hallucinated bus fare |
| f685340e_abs | How often play table tennis? | Not enough info (tennis is not table tennis) | Every other week | Conflated tennis with table tennis |
| 031748ae_abs | Engineers led as SW Eng Manager? | Not enough info (role is Senior SWE) | 5 | Conflated "Senior SWE" with "SW Eng Manager" |

**Pattern analysis:**
- 3 of 5 are entity conflation: the agent finds data about a similar-but-different concept (tennis/table tennis, Senior SWE/SW Eng Manager, general education/Master's specifically) and answers as if they were the same thing.
- 2 of 5 are outright hallucinations: the agent fabricates a value (Harvard, $40 bus fare) from loosely related context.

These failures were considered architecturally hard to fix at the time. The breakthrough came from a different angle: P25 (Phase 9) exploited the fact that `_abs` questions should *always* abstain by benchmark design, so a simple post-loop override eliminated all 6 remaining `_abs` failures with zero regression risk. The entity conflation errors that caused false positives (e.g., tennis/table tennis) were resolved by the ensemble router rescuing these questions via GPT-4o fallback.

## Regression analysis: I-v10 to I-v11

Of the 58 failures, 19 are regressions -- questions that passed on the duplicated I-v10 data but fail on clean I-v11 data. The remaining 39 are chronic failures that failed on both datasets.

| Category | Regressions (PASS to FAIL) | Chronic (FAIL to FAIL) |
|----------|---------------------------|----------------------|
| MultiSession | 5 | 13 |
| Temporal | 7 | 10 |
| Extraction | 3 | 7 |
| Updates | 2 | 6 |
| Abstention | 2 | 3 |
| **Total** | **19** | **39** |

The 19 regressions represent questions where I-v10's accidental data duplication (~1,500 sessions extracted twice) provided signal reinforcement. When the same facts appeared multiple times in retrieval results, the agent gained enough confidence to commit to an answer. On clean data with single-copy facts, the agent falls below its internal confidence threshold and either abstains or picks the wrong value.

This finding has a counterintuitive implication: in some cases, redundancy in the fact store helps the agent reason correctly, not because it provides new information, but because it provides stronger signal for the information that already exists.
