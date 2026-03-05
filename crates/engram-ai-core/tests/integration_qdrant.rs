//! Integration tests for Qdrant storage and retrieval
//!
//! Run with: OPENAI_API_KEY=... cargo test --test integration_qdrant -- --ignored

use engram_core::embedding::{EmbeddingProvider, RemoteEmbeddingProvider, EMBEDDING_DIMENSION};
use engram_core::storage::{QdrantConfig, QdrantStorage};
use engram_core::types::EpistemicType;
use engram_core::types::Memory;
use uuid::Uuid;

fn create_qdrant_config() -> QdrantConfig {
    // Port 6334 is gRPC, 6333 is REST - the Rust client uses gRPC
    QdrantConfig::external("http://localhost:6334").with_vector_size(EMBEDDING_DIMENSION as u64)
}

/// Test that we can connect to Qdrant and initialize collections
#[tokio::test]
#[ignore]
async fn test_qdrant_connection() {
    let config = create_qdrant_config();
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");
    let healthy = storage.health_check().await.expect("Health check failed");
    assert!(healthy, "Qdrant should be healthy");
}

/// Test collection initialization
#[tokio::test]
#[ignore]
async fn test_qdrant_initialize_collections() {
    let config = create_qdrant_config();
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");
    storage
        .initialize()
        .await
        .expect("Failed to initialize collections");

    // Verify collections exist by getting counts
    let counts = storage
        .get_collection_counts()
        .await
        .expect("Failed to get counts");
    assert_eq!(counts.len(), 4, "Should have 4 collections");

    for (name, _count) in &counts {
        assert!(
            ["world", "experience", "opinion", "observation"].contains(&name.as_str()),
            "Unexpected collection: {}",
            name
        );
    }
}

/// Test end-to-end: embed with OpenAI, store in Qdrant, retrieve
#[tokio::test]
#[ignore]
async fn test_end_to_end_embed_store_retrieve() {
    // Setup
    let embedding_provider =
        RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY must be set");

    let config = create_qdrant_config();
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");
    storage.initialize().await.expect("Failed to initialize");

    // Create a test memory
    let user_id = format!("test-user-{}", Uuid::now_v7());
    let content = "I love learning Rust programming language";

    let memory = Memory::new(&user_id, content)
        .with_epistemic_type(EpistemicType::Opinion)
        .with_entities(vec![(
            "rust".to_string(),
            "TECHNOLOGY".to_string(),
            "Rust".to_string(),
        )]);

    // Generate embedding
    let embedding = embedding_provider
        .embed_document(content)
        .await
        .expect("Failed to generate embedding");

    assert_eq!(
        embedding.len(),
        EMBEDDING_DIMENSION,
        "Embedding should have correct dimension"
    );

    // Verify embedding is normalized
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 0.01, "Embedding should be normalized");

    // Store in Qdrant
    storage
        .upsert_memory(&memory, embedding.clone())
        .await
        .expect("Failed to upsert memory");

    // Retrieve by ID
    let retrieved = storage
        .get_memory(&user_id, memory.id)
        .await
        .expect("Failed to get memory");
    assert!(retrieved.is_some(), "Should find the memory");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, memory.id);
    assert_eq!(retrieved.content, content);
    assert_eq!(retrieved.user_id, user_id);

    // List user memories
    let memories = storage
        .list_user_memories(&user_id, None, 100)
        .await
        .expect("Failed to list memories");
    assert!(!memories.is_empty(), "Should have at least one memory");
    assert!(
        memories.iter().any(|m| m.id == memory.id),
        "Should contain our memory"
    );

    println!("End-to-end test passed!");
    println!("  - Memory ID: {}", memory.id);
    println!("  - Embedding dimension: {}", embedding.len());
    println!("  - Content: {}", content);
}

/// Test storing and retrieving multiple memories
#[tokio::test]
#[ignore]
async fn test_multiple_memories() {
    let embedding_provider =
        RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY must be set");

    let config = create_qdrant_config();
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");
    storage.initialize().await.expect("Failed to initialize");

    let user_id = format!("test-user-multi-{}", Uuid::now_v7());

    let memories_data = vec![
        ("I work at Acme Corporation", EpistemicType::World),
        (
            "I enjoyed the new Star Wars movie",
            EpistemicType::Experience,
        ),
        ("I think Python is easier than C++", EpistemicType::Opinion),
        ("The weather today is sunny", EpistemicType::Observation),
    ];

    for (content, epistemic_type) in &memories_data {
        let memory = Memory::new(&user_id, *content).with_epistemic_type(*epistemic_type);

        let embedding = embedding_provider
            .embed_document(content)
            .await
            .expect("Failed to generate embedding");

        storage
            .upsert_memory(&memory, embedding)
            .await
            .expect("Failed to upsert memory");
    }

    // Retrieve all memories for user
    let retrieved = storage
        .list_user_memories(&user_id, None, 100)
        .await
        .expect("Failed to list memories");
    assert_eq!(retrieved.len(), 4, "Should have 4 memories");

    // Retrieve from specific collection
    for (name, expected_type) in [
        ("world", EpistemicType::World),
        ("experience", EpistemicType::Experience),
        ("opinion", EpistemicType::Opinion),
        ("observation", EpistemicType::Observation),
    ] {
        let coll_memories = storage
            .list_user_memories(&user_id, Some(name), 100)
            .await
            .expect("Failed to list");
        assert_eq!(
            coll_memories.len(),
            1,
            "Should have 1 memory in {} collection",
            name
        );
        assert_eq!(coll_memories[0].epistemic_type, expected_type);
    }

    println!("Multiple memories test passed!");
    println!("  - User ID: {}", user_id);
    println!(
        "  - Stored {} memories across 4 collections",
        memories_data.len()
    );
}

/// Test soft delete
#[tokio::test]
#[ignore]
async fn test_soft_delete() {
    let embedding_provider =
        RemoteEmbeddingProvider::from_env().expect("OPENAI_API_KEY must be set");

    let config = create_qdrant_config();
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");
    storage.initialize().await.expect("Failed to initialize");

    let user_id = format!("test-user-delete-{}", Uuid::now_v7());
    let content = "This memory will be deleted";

    let memory = Memory::new(&user_id, content).with_epistemic_type(EpistemicType::Observation);

    let embedding = embedding_provider
        .embed_document(content)
        .await
        .expect("Failed to generate embedding");

    storage
        .upsert_memory(&memory, embedding)
        .await
        .expect("Failed to upsert memory");

    // Verify it exists
    let before = storage
        .list_user_memories(&user_id, None, 100)
        .await
        .expect("Failed to list");
    assert_eq!(before.len(), 1, "Should have 1 memory before delete");

    // Soft delete
    let deleted = storage
        .delete_memory(&user_id, memory.id)
        .await
        .expect("Delete failed");
    assert!(deleted, "Should return true for successful delete");

    // After delete, list_user_memories should not return it (is_latest = false)
    let after = storage
        .list_user_memories(&user_id, None, 100)
        .await
        .expect("Failed to list");
    assert_eq!(after.len(), 0, "Should have 0 memories after soft delete");

    // But get_memory still finds it (it's not physically deleted)
    let still_exists = storage
        .get_memory(&user_id, memory.id)
        .await
        .expect("Get failed");
    assert!(
        still_exists.is_some(),
        "Memory should still exist physically"
    );
    assert!(
        !still_exists.unwrap().is_latest,
        "Memory should have is_latest = false"
    );

    println!("Soft delete test passed!");
}
