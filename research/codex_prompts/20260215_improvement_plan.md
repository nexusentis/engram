# Review Prompt: Clean-Slate Improvement Plan for LongMemEval-S

## Task
You are reviewing a memory system that scores 76-82% on the LongMemEval-S benchmark (50-question validation). The top systems score 90-95%. Design a concrete improvement plan to close this gap.

Start fresh — do not assume past approaches were correct. Review the top systems' architectures, review our code, and propose what we should actually build.

## The Benchmark
LongMemEval-S tests long-term conversational memory:
- 500 questions total, we use a 50-question stratified validation subset
- ~24K conversation sessions per user
- Categories: SingleSession-User, SingleSession-Assistant, SingleSession-Preference, Knowledge Updates, Temporal Reasoning, Multi-Session Reasoning
- Our 50q subset groups: Extraction(15), MultiSession(12), Temporal(13), Updates(7), Abstention(3)

## Top Systems (study their approaches)

### 1. Mastra OM (94.87%) — gpt-5-mini
- Stores complete sessions as formatted text blocks (no vector DB!)
- Retrieves by session relevance scoring
- Works because gpt-5-mini has huge context window
- NOT production-viable for large histories but proves formatting matters

### 2. Honcho (92.6%) — Gemini 3 Pro
- Agentic loop with fine-tuned retrieval models
- Observation-level prefetch (explicit vs deductive facts)
- Knowledge update consolidation (supersession detection)
- Category-specific prompting strategy
- 20 max iterations with smart termination

### 3. Hindsight (91.4%) — Gemini 3
- Entity knowledge graph built during ingestion
- 4-way TEMPR retrieval: semantic, entity-hop, temporal, meta-path
- Cross-encoder reranking with FULL passages (not truncated)
- Session-level scoring (not individual hit scoring)

### 4. Emergence AI (86%) — GPT-4o
- Chain-of-Note accumulator during ingestion
- Session-level NDCG retrieval
- Cross-encoder reranking

### 5. Mastra RAG (80%) — GPT-4o
- Simple RAG with good formatting
- topK=20, same as us
- Clean session formatting is the key differentiator

## Our Current System
- Score: 76-82% (non-agentic best)
- Qdrant vector DB with 5 fact collections + 1 messages collection
- gpt-4o-mini for extraction, gpt-4o for answering
- Hybrid search (vector + fulltext + messages) with RRF fusion
- Date-grouped context formatting
- Non-agentic single-pass works better than our agentic loop

## Our Code
Read these files to understand what we actually have:
- `crates/engram/src/bench/longmemeval/answerer.rs` — Answering pipeline
- `crates/engram/src/bench/longmemeval/tools.rs` — Tool implementations
- `crates/engram/src/bench/longmemeval/ingester.rs` — Ingestion pipeline
- `crates/engram/src/storage/qdrant.rs` — Storage + retrieval
- `crates/engram/src/bench/longmemeval/harness.rs` — Benchmark harness
- `crates/engram/src/retrieval/temporal_filter.rs` — Temporal filtering
- `crates/engram/src/bench/longmemeval/entity_graph.rs` — Entity graph (exists but disabled)
- `crates/engram/tests/integration_benchmark.rs` — Test configuration + env vars

## Deliverable
Write a plan to `research/improvement_plan_20260215.md` with:
1. Architecture comparison: what top systems do that we don't
2. Ranked list of changes (highest impact first)
3. For each change: what to modify, expected impact, implementation complexity
4. A phased roadmap: what to do first, second, third
5. Be specific — reference our actual code files and functions
6. Do NOT suggest "use Mastra OM approach" (storing all text without vector DB is a benchmark hack, not production-viable)
