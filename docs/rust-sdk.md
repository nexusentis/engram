---
title: Rust SDK
sidebar_position: 7
description: "Using engram-ai-core as a Rust library: MemorySystemBuilder, search, ingest, and advanced usage."
---

# Rust SDK

Use `engram-ai-core` as a Rust library to embed memory capabilities directly in your application.

## Add the dependency

```toml
[dependencies]
engram-ai-core = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Quick start with the builder

The simplest way to get started is `MemorySystemBuilder`:

```rust
use engram_core::{MemorySystem, Conversation, ConversationTurn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = MemorySystem::builder()
        .openai_api_key("sk-...")           // or set OPENAI_API_KEY env var
        .qdrant_url("http://localhost:6334") // default
        .build()
        .await?;

    // Ingest a conversation
    let conversation = Conversation::new("user_1", vec![
        ConversationTurn::user("I work at Anthropic on the safety team"),
        ConversationTurn::assistant("That's great! How long have you been there?"),
        ConversationTurn::user("About 6 months now"),
    ]);

    let ids = system.ingest(conversation).await?;
    println!("Stored {} memories", ids.len());

    // Search
    let results = system.search("user_1", "where does the user work?", 5).await?;
    for (memory, score) in &results {
        println!("[{:.3}] {}", score, memory.content);
    }

    Ok(())
}
```

### Builder options

| Method | Default | Description |
|--------|---------|-------------|
| `.openai_api_key(key)` | `OPENAI_API_KEY` env var | OpenAI API key |
| `.qdrant_url(url)` | `http://localhost:6334` | Qdrant gRPC URL |
| `.vector_size(size)` | 1536 | Vector dimension (match your embedding model) |
| `.embedding_model(model)` | `text-embedding-3-small` | OpenAI embedding model |
| `.extraction_model(model)` | `gpt-4o-mini` | LLM for fact extraction |

The builder automatically:
- Connects to Qdrant
- Creates collections if they don't exist (with retry for Docker race conditions)
- Initializes the embedding provider and extractor

## MemorySystem API

### Ingest

```rust
// From a Conversation object
let conversation = Conversation::new("user_id", vec![
    ConversationTurn::user("message content"),
]);
let ids: Vec<Uuid> = system.ingest(conversation).await?;

// Convenience: from turns directly
let turns = vec![ConversationTurn::user("hello")];
let ids = system.ingest_turns("user_id", &turns).await?;
```

### Search

```rust
let results: Vec<(Memory, f32)> = system.search("user_id", "query", 10).await?;

for (memory, score) in results {
    println!("ID: {}", memory.id);
    println!("Content: {}", memory.content);
    println!("Score: {:.3}", score);
    println!("Fact type: {:?}", memory.fact_type);
    println!("Confidence: {:.2}", memory.confidence);
}
```

### Store a fact directly

```rust
// Simple text fact
let id: Uuid = system.store_fact("user_id", "Alice works at Google").await?;

// Full control with Memory struct
let memory = Memory::new("user_id", "Alice works at Google")
    .with_fact_type(FactType::State)
    .with_epistemic_type(EpistemicType::World)
    .with_source(SourceType::UserExplicit)
    .with_entities(vec![
        ("alice".into(), "person".into(), "Alice".into()),
        ("google".into(), "organization".into(), "Google".into()),
    ]);
let id = system.store_memory("user_id", &memory).await?;
```

### Get and delete

```rust
// Get by ID
let memory: Option<Memory> = system.get_memory("user_id", uuid).await?;

// Soft-delete (marks expired, sets is_latest = false)
let deleted: bool = system.delete_memory("user_id", uuid).await?;
```

### Escape hatches

Access underlying components for advanced use cases:

```rust
// Direct Qdrant access
let qdrant: &Arc<QdrantStorage> = system.storage();

// Embedding provider
let embedder: &Arc<dyn EmbeddingProvider> = system.embedder();

// Extractor
let extractor: &Arc<ApiExtractor> = system.extractor();
```

## Constructing from components

For full control, build `MemorySystem` from individual components:

```rust
use std::sync::Arc;
use engram_core::{
    MemorySystem, QdrantConfig, QdrantStorage,
    RemoteEmbeddingProvider, EmbeddingProvider,
    ApiExtractor, ApiExtractorConfig,
};

let qdrant_config = QdrantConfig::external("http://localhost:6334");
let qdrant = Arc::new(QdrantStorage::new(qdrant_config).await?);

let embedder: Arc<dyn EmbeddingProvider> = Arc::new(
    RemoteEmbeddingProvider::new("sk-...", Some("text-embedding-3-small".into()))?
);

let extractor_config = ApiExtractorConfig::openai("gpt-4o-mini")
    .with_api_key("sk-...");
let extractor = Arc::new(ApiExtractor::new(extractor_config));

let system = MemorySystem::new(qdrant, embedder, extractor);
```

## Key types

### Memory

The core storage unit. See [Concepts](concepts) for the full mental model.

```rust
pub struct Memory {
    pub id: Uuid,                              // UUIDv7 (time-sortable)
    pub user_id: String,
    pub content: String,
    pub confidence: f32,                       // [0.0, 1.0]
    pub fact_type: FactType,                   // State, Event, Preference, Relation
    pub epistemic_type: EpistemicType,         // World, Experience, Opinion, Observation
    pub source_type: SourceType,               // UserExplicit, UserImplied, ...
    pub entity_ids: Vec<String>,
    pub t_created: DateTime<Utc>,
    pub t_valid: DateTime<Utc>,
    pub t_expired: Option<DateTime<Utc>>,
    pub supersedes_id: Option<Uuid>,
    pub is_latest: bool,
    // ... and more fields
}
```

### Conversation and ConversationTurn

```rust
let conversation = Conversation::new("user_id", vec![
    ConversationTurn::user("I like Rust"),
    ConversationTurn::assistant("Me too!"),
    ConversationTurn::system("You are a helpful assistant"),
])
.with_session("session-123");  // Optional session grouping
```

### ExtractedFact

Returned by the extractor (you rarely need this directly):

```rust
pub struct ExtractedFact {
    pub content: String,
    pub confidence: f32,
    pub fact_type: FactType,
    pub epistemic_type: EpistemicType,
    pub source_type: SourceType,
    pub entities: Vec<Entity>,
    pub t_valid: Option<DateTime<Utc>>,
    pub observation_level: String,
}
```

## Error handling

All fallible methods return `engram_core::Result<T>`, which wraps `engram_core::Error`:

```rust
use engram_core::Error;

match system.search("user", "query", 10).await {
    Ok(results) => { /* ... */ }
    Err(Error::Storage(e)) => eprintln!("Qdrant error: {e}"),
    Err(Error::Embedding(e)) => eprintln!("Embedding error: {e}"),
    Err(Error::Extraction(e)) => eprintln!("Extraction error: {e}"),
    Err(e) => eprintln!("Other error: {e}"),
}
```

## Crate structure

The workspace contains several crates:

| Crate | crates.io | Description |
|-------|-----------|-------------|
| [`engram-ai-core`](https://crates.io/crates/engram-ai-core) | `cargo add engram-ai-core` | Core library: types, storage, extraction, embedding, retrieval |
| [`engram-ai`](https://crates.io/crates/engram-ai) | `cargo add engram-ai` | Convenience facade re-exporting `engram-ai-core` |
| [`engram-agent`](https://crates.io/crates/engram-agent) | `cargo add engram-agent` | Agent framework for building AI agents with memory |
| [`engram-server`](https://crates.io/crates/engram-server) | `cargo add engram-server` | REST and MCP HTTP server (axum) |
| [`engram-cli`](https://crates.io/crates/engram-cli) | `cargo add engram-cli` | CLI tool (`engram init`, `engram status`, `engram config`) |

For most use cases, depend only on `engram-ai-core`.
