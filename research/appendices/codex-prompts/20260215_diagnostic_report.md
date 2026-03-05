# Review Prompt: Diagnostic Report on LongMemEval-S Benchmark Performance

## Task
Analyze our LongMemEval-S benchmark results across 19 runs and produce a diagnostic report. Focus on:
1. Why non-agentic consistently beats agentic (5 out of 6 comparisons)
2. What's causing the 6pp gap between our best run (#17, 82%) and latest (#19, 76%)
3. Where the biggest category-level weaknesses are and what's driving them
4. Whether our ingestion variance (12pp across runs) is normal or a fixable problem

## Our System Architecture
- **Ingestion**: gpt-4o-mini extracts facts from conversation sessions → stored in Qdrant (5 collections: opinion, world, observation, experience + messages)
- **Message storage**: Raw conversation turns stored with embeddings in separate messages collection
- **Retrieval**: Hybrid search (vector + fulltext + messages) with RRF fusion, top_k=20
- **Non-agentic path**: Single-pass retrieve → build date-grouped context → LLM answers
- **Agentic path**: LLM gets tools (search_facts, search_messages, grep_messages, get_session_context, get_by_date_range, search_entity, done), prefetch initial context, then iteratively calls tools up to 20 iterations
- **Answering model**: gpt-4o (temp=0.0)

## Run History (numbered, all 50-question validation subset)

IMPORTANT: Runs #1-#16 did NOT have numeric answer parsing fix — scores were artificially lower.

| # | Mode | Ingestion | Key Changes | Multi(12) | Abst(3) | Upd(7) | Temp(13) | Extr(15) | Total(50) |
|---|------|-----------|-------------|-----------|---------|--------|----------|----------|-----------|
| 1 | non-ag | fresh t=0.1 | Timestamp fix | 9 | 3 | 6 | 11 | 7 | 36 (72%) |
| 2 | non-ag | fresh t=0.1 | + Raw messages | 9 | 2 | 6 | 9 | 11 | 37 (74%) |
| 3 | **agent** | reused #2 | + Agentic loop | 7 | 2 | 5 | 9 | 12 | 35 (70%) |
| 5 | non-ag | reused #2 | + Date-grouped | 8 | 3 | 5 | 11 | 12 | 39 (78%) |
| 7 | **agent** | reused #2 | Enhanced agentic | 7 | 2 | 6 | 6 | 12 | 33 (66%) |
| 10 | **agent** | fresh t=0.0 | 9-phase, P5 ON | 7 | 2 | 5 | 9 | 12 | 35 (70%) |
| 12 | non-ag | reused #10 | Non-ag same data | 4 | 3 | 6 | 9 | 10 | 32 (64%) |
| 13 | **agent** | fresh t=0.1 | + Loop detection | 7 | 2 | 5 | 8 | 12 | 34 (68%) |
| 14 | non-ag | reused #13 | Non-ag same data | 7 | 3 | 5 | 8 | 12 | 35 (70%) |
| 15 | non-ag | reused #13 | + Judge fix + abstention | 8 | 3 | 5 | 10 | 11 | 37 (74%) |
| 16 | **agent** | reused #13 | Agent same fixes | 6 | 3 | 5 | 8 | 12 | 34 (68%) |
| 17 | non-ag | reused #13 | + Numeric answer fix | 9 | 3 | 7 | 10 | 12 | **41 (82%)** |
| 18 | **agent** | fresh t=0.1 | + user_id scoping | 8 | 3 | 6 | 5 | 13 | 35 (70%) |
| 19 | non-ag | reused #18 | Non-ag same data | 7 | 3 | 7 | 8 | 13 | 38 (76%) |

Head-to-head on same data:
- #2 data: agent 70% vs non-ag 74% → non-ag wins
- #2 data: agent 66% vs non-ag 78% → non-ag wins (+12pp!)
- #10 data: agent 70% vs non-ag 64% → agent wins (ONLY win)
- #13 data: agent 68% vs non-ag 70% → non-ag wins
- #13 data: agent 68% vs non-ag 74% → non-ag wins
- #18 data: agent 70% vs non-ag 76% → non-ag wins

## Key Code Files
Read these files for architecture understanding:
- `crates/engram/src/bench/longmemeval/answerer.rs` — Both non-agentic and agentic answering paths
- `crates/engram/src/bench/longmemeval/tools.rs` — Agentic tool implementations
- `crates/engram/src/bench/longmemeval/ingester.rs` — Fact extraction + message ingestion
- `crates/engram/src/storage/qdrant.rs` — Qdrant storage layer (hybrid search, RRF)
- `crates/engram/src/bench/longmemeval/harness.rs` — Benchmark runner + judge
- `crates/engram/src/bench/longmemeval/judge.rs` — Answer evaluation

## Deliverable
Write a report to `research/diagnostic_report_20260215.md` covering:
1. Root cause analysis of agentic vs non-agentic gap
2. Ingestion variance diagnosis
3. Per-category failure pattern analysis
4. Specific code-level issues found
5. Ranked list of fixes with expected impact
