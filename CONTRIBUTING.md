# Contributing to Engram

## Prerequisites

- **Rust 1.75+** — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Qdrant** — via `./scripts/setup-qdrant.sh` (native binary) or `docker compose up -d`
- **OpenAI API key** — for extraction (gpt-4o-mini) and answering (gpt-4o)
- **just** (optional) — `cargo install just` for shorthand commands

## Setup

```bash
git clone https://github.com/user/engram.git
cd engram
cp .env.example .env          # set OPENAI_API_KEY
cargo build --release
./scripts/setup-qdrant.sh     # or: docker compose up -d --wait
cargo run --bin engram -- init
cargo run --bin engram -- status
```

## Running Tests

**Unit tests** (no API key or Qdrant needed):

```bash
cargo test --lib
# or: just test
```

**Integration benchmark** (requires API key + Qdrant):

```bash
# Fast loop — 60 questions, ~$13
just bench-fast

# Gate — 231 questions, ~$49
just bench-gate

# Truth — 500 questions, ~$106
just bench-truth
```

Use `INGESTION=skip` to iterate on query-time changes without re-ingesting:

```bash
just bench-fast-skip
```

See the [README](README.md) for full benchmark documentation.

## Project Layout

See the [Project Structure](README.md#project-structure) section in the README.

## Benchmark Workflow

Follow the 3-tier testing discipline:

1. **Fast Loop** (60q) — run after every code change
2. **Gate** (231q) — run before promoting a change; check net delta is positive
3. **Truth** (500q) — run after accumulating 2-3 gate-passing changes

Be cost-aware: each run costs real money. Always note the estimated cost when reporting benchmark results.

## Code Style

- `cargo fmt` — format before committing
- `cargo clippy -- -D warnings` — no warnings allowed
- **TODO policy**: every TODO must reference a task — `// TODO(Task XX-YY): Description`
- Keep commit messages concise and professional

## Pull Requests

1. Branch from `main`
2. Include benchmark results if your change touches retrieval or extraction
3. Ensure `cargo fmt --check && cargo clippy -- -D warnings && cargo test --lib` pass
4. Keep PRs focused — one logical change per PR
