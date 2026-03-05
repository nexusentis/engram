---
title: "Context & Landscape"
sidebar_position: 1
---

# Context & Landscape

Before diving into how we built Engram and what we learned, it helps to understand the playing field. LongMemEval-S is the first rigorous benchmark for long-term conversational memory systems, and the competitive landscape around it evolved rapidly in the period leading up to our work. This section establishes the context we were operating in.

## What is LongMemEval-S?

LongMemEval-S (the "small" variant of LongMemEval) is a benchmark designed to evaluate whether AI memory systems can retain, organize, and recall information from extended conversation histories. It consists of 500 questions across 5 categories, drawn from approximately 24,000 synthetic conversation sessions. The benchmark was introduced in an ICLR 2025 paper and quickly became the standard evaluation for production memory systems.

We chose LongMemEval-S as our north star metric because it tests the exact capabilities a production memory system needs: extracting facts from noisy conversation, tracking knowledge updates over time, reasoning about temporal relationships, synthesizing information across multiple sessions, and knowing when to abstain.

## What you will find here

- **[The LongMemEval-S Benchmark](./benchmark-overview)** --- What the benchmark tests, how it is structured, and the key academic findings that informed our approach.

- **[Competitive Landscape](./competitive-landscape)** --- The full leaderboard of 26 systems, per-category breakdowns, the three architectural camps, and where we started relative to the field.

- **[Deep Dive: Mastra OM (94.87%)](./mastra-om)** --- How the top-ranked system achieves near-perfect accuracy using context-window compression with zero retrieval.

- **[Deep Dive: Honcho (92.6%)](./honcho)** --- How an agentic tool-calling loop with dual storage (raw messages + extracted facts) reaches second place.

- **[Deep Dive: Hindsight (91.4%)](./hindsight)** --- How a 4-way parallel retrieval system with entity graphs and meta-path traversal achieves third place.

These deep dives were instrumental in shaping our engineering decisions. Understanding *why* these systems succeed --- and where their approaches would and would not transfer to our architecture --- drove every major design choice documented in the later sections.
