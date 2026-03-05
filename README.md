<p align="center">
  <img src="research/static/img/logo.svg" width="80" alt="Engram">
</p>

<h1 align="center">Engram</h1>

<p align="center">
  Rust-native AI agent memory system achieving <strong>95.8% accuracy on LongMemEval-S</strong> — #1 globally, surpassing all known commercial systems.
</p>

## Architecture

```
Conversation ──► Extraction ──► Qdrant ──► Retrieval ──► Answer
                 (gpt-4o-mini)  (vector)   (hybrid+RRF)  (Gemini + GPT-5.2 ensemble)
```

**Extraction** breaks conversations into typed facts across 4 epistemic collections:
- `world` — objective facts about the world
- `experience` — personal experiences and events
- `opinion` — preferences, beliefs, evaluations
- `observation` — behavioral patterns and inferred traits

**Retrieval** combines semantic search, keyword matching, temporal filtering, and entity-linked lookup, fused via Reciprocal Rank Fusion (RRF).

## Features

- 5-dimensional fact extraction (entities, epistemic type, temporality, source, confidence)
- 4 epistemic Qdrant collections with type-specific schemas
- Hybrid retrieval: semantic + keyword + temporal + entity-linked
- RRF fusion with configurable k parameter
- Agentic answering loop with tool use
- **MCP server** for integration with Claude Desktop, Cursor, and other MCP clients
- Full LongMemEval-S benchmark harness with 3-tier testing

## MCP Quickstart

Engram exposes an MCP server (`engram-server --mode mcp`) that any MCP-compatible client can use to store and query long-term memory.

### 1. Start Qdrant

```bash
docker compose up -d qdrant --wait
```

### 2. Build engram-server

```bash
cargo build --release -p engram-server
```

### 3. Configure your MCP client

**Claude Desktop** — copy `config/claude_desktop_config.example.json` into `~/Library/Application Support/Claude/claude_desktop_config.json` and update the binary path and API key.

**Cursor** — copy `config/cursor_mcp.example.json` into your Cursor MCP settings and update the binary path and API key.

**Docker** — start Qdrant via Docker Compose, then point your MCP client at the local binary:

```bash
docker compose up -d qdrant --wait   # start Qdrant only
# Then configure Claude Desktop / Cursor to run engram-server directly
```

### Available Tools

| Tool | Description |
|------|-------------|
| `memory_add` | Extract and store facts from a conversation |
| `memory_search` | Semantic search across stored memories |
| `memory_get` | Retrieve a specific memory by ID |
| `memory_delete` | Soft-delete a memory |

## Quickstart (Development)

### 1. Build

```bash
git clone https://github.com/nexusentis/engram.git
cd engram
cargo build --release
```

### 2. Set up Qdrant

**Option A: Native binary (no Docker)**

```bash
./scripts/setup-qdrant.sh
./bin/qdrant  # starts on :6333 (REST) + :6334 (gRPC)
```

**Option B: Docker**

```bash
docker compose up -d --wait
```

### 3. Configure

```bash
cp .env.example .env
# Edit .env and set OPENAI_API_KEY
```

### 4. Initialize

```bash
cargo run --bin engram -- init
cargo run --bin engram -- status
```

## Crates

All crates are published to [crates.io](https://crates.io):

| Crate | Description |
|-------|-------------|
| [`engram-ai-core`](https://crates.io/crates/engram-ai-core) | Core library: types, storage, extraction, embedding, retrieval |
| [`engram-agent`](https://crates.io/crates/engram-agent) | Reusable LLM agent loop with tool-calling and lifecycle hooks |
| [`engram-ai`](https://crates.io/crates/engram-ai) | Convenience facade re-exporting `engram-ai-core` |
| [`engram-server`](https://crates.io/crates/engram-server) | REST + MCP server binary |
| [`engram-cli`](https://crates.io/crates/engram-cli) | CLI binary (`engram init`, `engram status`) |

```bash
cargo add engram-ai-core   # core library
cargo add engram-ai         # or the facade
```

## Project Structure

```
engram/
├── crates/
│   ├── engram-ai-core/            # Core library
│   │   ├── src/
│   │   │   ├── api/               # HTTP + MCP API layers
│   │   │   ├── config/            # Configuration loading & validation
│   │   │   ├── embedding/         # Remote (OpenAI) embeddings
│   │   │   ├── extraction/        # LLM-based fact extraction pipeline
│   │   │   ├── retrieval/         # Hybrid search engine, RRF, reranking
│   │   │   ├── storage/           # Qdrant backend
│   │   │   ├── temporal/          # Temporal parsing & filtering
│   │   │   └── types/             # Core data types (Entity, Memory, Session)
│   │   └── tests/
│   ├── engram-agent/              # LLM agent loop
│   ├── engram-ai/                 # Facade crate
│   ├── engram-server/             # Server binary (MCP + HTTP modes)
│   └── engram-cli/                # CLI binary (init, status, config)
├── config/                        # TOML configs + MCP client examples
├── data/
│   └── longmemeval/               # Benchmark data & question sets
└── scripts/                       # Setup & utility scripts
```

## Configuration

Configuration is via environment variables. See [`.env.example`](.env.example) for common settings.

Key variables:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OPENAI_API_KEY` | Yes | — | OpenAI API key |
| `ENGRAM_QDRANT_URL` | No | `http://localhost:6334` | Qdrant gRPC endpoint |

## Benchmark

Engram includes a full LongMemEval-S benchmark harness with 3 testing tiers:

| Tier | Questions | Cost (Gemini) | Use |
|------|-----------|---------------|-----|
| Fast Loop | 60 | ~$4 | Every tweak |
| Gate | 231 | ~$15 | Before promoting a change |
| Truth | 500 | ~$30 | Definitive score |

```bash
# Load env vars
set -a; source .env; set +a

# Fast loop (60 questions)
BENCHMARK_CONFIG=config/benchmark.toml INGESTION=skip \
  QUESTION_IDS=@data/longmemeval/fast_60.txt \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Full truth run (500 questions)
BENCHMARK_CONFIG=config/benchmark.toml INGESTION=skip FULL_BENCHMARK=1 \
  cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture
```

Control ingestion with `INGESTION=full|skip|incremental`. Use `INGESTION=skip` to iterate on query-time changes without re-ingesting.

## Documentation

- **[Research](https://engram.nexusentis.ie/research/)** — The full research narrative: from 0% to 95.8% across eleven phases, failed experiments, engineering discipline rules, and the path forward.
- **[Developer Docs](https://engram.nexusentis.ie/docs/)** — API reference, configuration, and integration guides.

## License

MIT
