# Stage 1: Build
FROM rust:1.84-bookworm AS builder

WORKDIR /usr/src/engram
COPY . .

RUN cargo build --release -p engram-server

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home engram
USER engram

COPY --from=builder /usr/src/engram/target/release/engram-server /usr/local/bin/engram-server

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["engram-server"]
CMD ["--mode", "rest"]
