set dotenv-load

# Build
build:
    cargo build --release

# Run unit tests (no API key needed)
test:
    cargo test --lib

# Initialize Qdrant collections + SQLite
init:
    cargo run --bin engram -- init

# Check system status
status:
    cargo run --bin engram -- status

# Start Qdrant (native binary)
qdrant-up:
    ./bin/qdrant &
    @echo "Waiting for Qdrant..."
    @for i in $(seq 1 15); do curl -sf http://localhost:6333/collections > /dev/null 2>&1 && echo "Qdrant is ready" && exit 0; sleep 1; done; echo "Qdrant failed to start"; exit 1

# Start Qdrant (Docker)
qdrant-docker:
    docker compose up -d --wait

# Benchmark: fast loop (60q, ~$13)
bench-fast:
    QUESTION_IDS=@data/longmemeval/fast_60.txt \
      cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Benchmark: gate (231q, ~$49)
bench-gate:
    QUESTION_IDS=@data/longmemeval/gate_231.txt \
      cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Benchmark: truth (500q, ~$106)
bench-truth:
    FULL_BENCHMARK=1 \
      cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Benchmark: fixcheck only (81q)
bench-fixcheck:
    QUESTION_IDS=@data/longmemeval/fixcheck_81.txt \
      cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Benchmark: fast loop, skip ingestion
bench-fast-skip:
    INGESTION=skip QUESTION_IDS=@data/longmemeval/fast_60.txt \
      cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Ingest only (no answering)
ingest:
    INGESTION_ONLY=1 FULL_BENCHMARK=1 \
      cargo test --release --test integration_benchmark test_validation_benchmark -- --ignored --nocapture

# Recall harness: SinglePass on fast_60 (~$0.10)
recall-fast:
    QUESTION_IDS=@data/longmemeval/fast_60.txt \
      cargo test --release --test integration_benchmark test_recall_harness -- --ignored --nocapture

# Recall harness: AgenticSynthetic on fast_60 (~$0.30)
recall-fast-agentic:
    RECALL_MODE=agentic_synthetic QUESTION_IDS=@data/longmemeval/fast_60.txt \
      cargo test --release --test integration_benchmark test_recall_harness -- --ignored --nocapture

# Recall harness: SinglePass on full 500q (~$0.50)
recall-full:
    FULL_BENCHMARK=1 \
      cargo test --release --test integration_benchmark test_recall_harness -- --ignored --nocapture

# Recall harness: AgenticSynthetic on full 500q (~$1.50)
recall-full-agentic:
    RECALL_MODE=agentic_synthetic FULL_BENCHMARK=1 \
      cargo test --release --test integration_benchmark test_recall_harness -- --ignored --nocapture

# Check: fmt + clippy
check:
    cargo fmt --check
    cargo clippy -- -D warnings
