---
title: "Lessons & Future Directions"
sidebar_position: 6
description: "What we learned and where to go next"
---

# Lessons & Future Directions

Over the course of the project, ~$1,500 of experiments, and 50+ benchmark runs, we have a clear empirical picture of what drives performance in long-term memory systems --- and what does not. This section distills those findings into actionable principles.

The most important takeaway evolved through three phases. Phase 7 showed that model diversity is the highest-ROI intervention at 88%+ (ensemble routing gained +15 with ~200 lines of code). Phase 9's architecture review revealed that the remaining failures are shared across models --- representation quality is the path beyond 94%. Then Phase 11 showed that a better fallback model (GPT-5.2 replacing GPT-4o) pushed to 479/500 (95.8%) --- **#1 globally**, surpassing Mastra OM (94.87%).

## What you will find here

- **[What Actually Moves the Score](./what-moves-the-score)** --- A ranked hierarchy of intervention types by empirical impact. Tool correctness dwarfs prompt engineering; deterministic post-processing is a dead end.

- **[The Retrieval vs. Reasoning Gap](./ingestion-vs-reasoning)** --- Why 99.6% retrieval recall is not enough, how ingestion duplication created false signal reinforcement, and what the SOTA leaderboard tells us about where the real ceiling lies.

- **[Path to 90%+ (Historical)](./path-to-90-percent)** --- The original roadmap from 442/500 (88.4%) with gpt-4o, written during Phase 5. Now annotated with outcomes: model upgrade worked (+10q), ensemble was not predicted but delivered +15q, GPT-5.2 fallback reached 95.8%. A useful case study in prediction vs. reality.

- **[Path to 96%+: Multi-Model Roadmap](./path-to-95-percent)** --- The living roadmap from the Gemini/GPT-5.2 ensemble. Updated after Phase 11 Truth run (479/500, 95.8%, #1 globally). Architecture changes now target 481+ from the new baseline.

- **[Single-Model Architecture: Path Beyond Ensemble](./single-model-architecture)** --- The full architectural deep dive from the Phase 9 review. Competitor comparison (Mastra, Honcho, Hindsight), the five architecture gaps, ranked interventions, and the Hybrid Observation Memory proposal. Updated with Phase 11 at 479/500 --- we are now #1 globally.
