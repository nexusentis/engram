---
title: "Graph Tools & Retrieval Augmentation"
sidebar_position: 2
---

# Graph Tools & Retrieval Augmentation

We built a SurrealDB knowledge graph with 317K entities and 88K relationships. Then we tried four different ways to use it. All four failed or were neutral. This page documents the graph saga and explains why, at 498/500 retrieval recall, graph-based augmentation has no room to help.

## The Graph Infrastructure

Before the experiments, we invested approximately two weeks and $8 in ingestion building a SurrealDB-backed knowledge graph (`GraphStore` with RocksDB backend). During ingestion of 23,867 sessions, the system extracted entities, linked relationships, and tracked mentions:

- 317K entities (people, places, topics, events)
- 88K relationships between entities
- 152K entity mentions linked to source facts

The graph itself was well-built and correct. The failures were in how we tried to use it.

## Attempt 1: P18 -- Graph Tools Exposed to Agent (Day 16)

**What it did**: Added 4 SurrealDB graph tools to the agent's tool set:

- `graph_enumerate` -- list all entities connected to a topic
- `graph_lookup` -- find specific entity details
- `graph_relationships` -- traverse entity relationships
- `graph_disambiguate` -- resolve ambiguous entity references

This expanded the agent's tool set from 8 to 12 tools. All questions received the graph tools when `GRAPH_RETRIEVAL=1` was set.

**Result**: FL scored 54/60 (+1 vs baseline, noise). Gate scored 199/231 (-5 vs 204/231 baseline).

**The critical finding: the agent made zero graph tool calls.** Across all questions in both the Fast Loop and the Gate run, not a single `graph_*` function was invoked. The regressions were caused entirely by the presence of the tool schemas in the function-calling API.

### Why the agent ignored the graph tools

Five barriers prevented adoption, each sufficient on its own:

1. **Prefetch gives early wins.** 45 text results are loaded before the agentic loop starts. The agent already has enough context to begin answering, and its existing tools (search_facts, search_messages) are familiar and productive.

2. **Enumeration guidance does not mention graph.** The 40-line Enumeration strategy procedure references only text search tools. The agent follows its guidance.

3. **Graph tools are invisible to quality gates.** The retrieval tracking system did not count graph calls, so the gates that push the agent to "search more" could not credit graph tool usage.

4. **Abstract schema descriptions.** The tool descriptions used terms like "knowledge graph" and "entity relationships" rather than concrete descriptions like "facts with dates from conversations."

5. **No early-success mechanism.** The agent had no reason to try an unfamiliar tool when familiar ones were working. Without prompt guidance or demonstrated value, the tools sat unused.

### The schema bloat mechanism

The deeper problem was that adding 4 tool schemas changed the model's planning behavior even though the tools were never called. We validated this with an A/B test on 16 questions that regressed:

- `GRAPH_RETRIEVAL=0` (8 tools): 9/16 correct
- `GRAPH_RETRIEVAL=1` (12 tools): 8/16 correct

The tools were never called in either run. The regression came from the expanded action space alone. This is consistent with LLM research showing that tool/function surface area directly affects model policy -- more options lead to different planning decisions, not necessarily better ones.

## Attempt 2: P18.1 -- Scoped Graph Tools (Day 16)

**What it did**: Narrowed graph tool activation to only Enumeration-strategy questions (after strategy detection). This reduced the blast radius from all questions to approximately 30% of questions.

**Result**: FL scored 52/60 (-2 vs baseline). Still zero graph tool calls.

The scoping reduced the damage but did not eliminate it. Even with tools available on only ~16 of 60 Fast Loop questions, the schema expansion still changed agent behavior on those questions enough to cause 2 regressions.

P18.1 also removed the graph-specific prompt guidance that P18 had included, reasoning that if the agent would not use the tools voluntarily, forcing it via prompts would be worse. This created a catch-22: without guidance the agent ignores the tools, but with guidance the prompt bloat may cause its own regressions.

## Attempt 3: P20 -- Behind-the-Scenes Graph Prefetch (Day 17)

**What it did**: Inspired by Hindsight (the only SOTA system where graph demonstrably helps, at 91.4%), P20 implemented silent graph augmentation. The agent never sees or queries the graph. Instead:

1. **Seed extraction**: For each fact in the prefetch results, reverse-lookup entities via the `mention` table. Cap at 5 seed entities.
2. **1-hop spreading activation**: For each seed entity, get 1-hop neighbors via graph edges. Score: seed mention = 1.0, neighbor mention = 0.5, multi-link bonus x 1.5. Remove facts already in prefetch.
3. **Fetch and format**: Look up top 6 scored facts from Qdrant, format as date-grouped text appended to prefetch.

Zero tool schema changes. Zero prompt changes. The agent receives slightly richer initial context without knowing it came from a graph.

Gating: only fires when `question.category == MultiSession` or `strategy == Enumeration` (approximately 183/500 questions).

**Result**: FL scored 54/60 = exact baseline. P20 fired correctly on 17 of 20 targeted questions, injecting 253-1773 characters of graph-linked context per question. The two MultiSession questions that flipped were stochastic LLM counting variance, not P20-related.

### Why P20 was neutral

The root cause is straightforward: **graph facts overlap almost entirely with vector search results.**

Qdrant recall is 498/500. The vector search already finds the relevant facts. Graph spreading activation discovers the same facts via a different path (entity -> mention -> fact_id), but they are the same facts. The deduplication step removes most graph-found facts because they are already in the prefetch. The 6 additional facts that survive deduplication are typically low-relevance peripheral mentions.

This is fundamentally different from Hindsight's situation. Hindsight uses graph to compensate for unreliable base retrieval -- their semantic search may miss items, and graph traversal catches them. Our Qdrant search is already near-perfect at recall. Adding a redundant retrieval channel adds nothing.

## Attempt 4: P21 -- Temporal Scatter Search (Day 17, Rejected Pre-Implementation)

**What it did** (proposed): Add a 4th prefetch channel for Enumeration questions. Split the user's time range into 4-6 temporal buckets, run `search_facts` per bucket, deduplicate against existing prefetch, append unique results.

**Why it was rejected**: A pre-implementation review identified five problems before any code was written:

1. Only 8 of 13 aggregation failures route to Enumeration strategy (5 route to Default or Temporal)
2. 6 of 13 aggregation failures have all relevant items in a single session or day, making temporal bucketing useless
3. `search_facts` has no date-range parameter; adding one would require API changes
4. P20's lesson applies: at 498/500 recall, temporal scatter will rediscover already-found facts
5. Realistic impact estimate: +0 to +1, with -1 to -3 downside risk from context dilution

**Verdict**: Do not implement. The savings from not running a $13 Fast Loop on a likely-neutral change are worth more than the negligible upside.

## SOTA Comparison: Nobody Exposes Graph Tools to Agents

After P18's failure, we surveyed every system on the LongMemEval leaderboard:

| System | Score | Uses Graph? | How |
|---|---|---|---|
| Mastra OM | 94.87% | No | No retrieval at all -- observation log in prompt |
| Honcho | 92.6% | No | Reasoning trees + agentic vector search |
| Hindsight | 91.4% | Yes | Behind the scenes -- 1 of 4 retrieval channels, RRF-fused |
| **Engram (ours)** | **88.4%** | **Attempted** | **Exposed as agent tools -- no SOTA does this** |
| Emergence AI | 86.0% | No | Session-level NDCG + cross-encoder reranking |
| SGMem | 73.0% | Yes | Sentence KNN graph, modest +3.5% over non-graph baseline |
| Zep/Graphiti | 71.2% | Yes (behind scenes) | Bi-temporal KG + community summaries |
| Mem0 | 68.4% | Yes | Vector + graph dual memory |

The pattern is clear:

- The top 2 systems use no graph at all.
- The only system where graph demonstrably helps (Hindsight) uses it silently as one of four retrieval channels, fused via RRF. The agent never sees the graph.
- Graph-heavy systems (Zep 71.2%, SGMem 73.0%, Mem0 68.4%) dramatically underperform non-graph systems.
- The correlation between graph complexity and benchmark score is negative.

The number-one system (Mastra OM at 94.87%) uses no retrieval whatsoever. Two background LLM agents compress conversations into a structured event log that lives entirely in the system prompt. This suggests that the gap from 88% to 95% is about reasoning quality, not data architecture.

## What We Learned

### Tool schema count is a first-class engineering constraint

Adding tool schemas to the function-calling API costs approximately 50 tokens per tool and changes the model's action space. The cost is not just tokens -- it is decision quality. Going from 8 to 12 tools caused measurable regressions with zero new tool calls. This is not fixable with prompt engineering. It is a fundamental property of LLM function calling.

**Rule**: Never add tools "just in case." Every tool must earn its schema cost with demonstrated per-question value.

### High recall makes additional retrieval channels redundant

At 498/500 recall, any "find more facts" intervention will mostly rediscover already-found facts. The problem is not finding facts -- it is reasoning over them correctly. P20 proved this experimentally: the graph found the same facts the vector search found, via a different path.

**Rule**: Before investing in retrieval augmentation, measure recall. If recall is above 99%, the bottleneck is elsewhere.

### "Build it and they will come" does not apply to LLM tool use

Agents adopt tools when: (a) the prompt tells them to, (b) the tool offers a clear advantage over existing tools, and (c) the feedback loop rewards tool usage. P18 had none of these. The agent had no reason to try unfamiliar graph tools when familiar search tools were working adequately.

### Graph data has value -- but not as agent tools

The SurrealDB graph (317K entities, 88K relationships) remains in the codebase as infrastructure. Future uses that may have value include entity disambiguation at retrieval time (resolving "tennis" vs. "table tennis" before the agent sees results), ingestion-time knowledge consolidation (identifying contradictions across sessions), and query-time entity linking (expanding retrieval to include co-occurring entities). These are all behind-the-scenes uses where the agent never interacts with the graph directly.
