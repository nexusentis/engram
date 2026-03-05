//! Debug retrieval to understand why "Target" wasn't found
//!
//! Run with: cargo test --test debug_retrieval -- --nocapture

use engram_core::storage::{QdrantConfig, QdrantStorage};

#[tokio::test]
async fn debug_coupon_memories() {
    // Connect to Qdrant
    let config = QdrantConfig::external("http://localhost:6334").with_vector_size(1536);
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");

    // Q3 question_id is "51a45a95" -> user_id = "user_51a45a95"
    let user_id = "user_51a45a95";

    println!(
        "=== Searching for coupon-related memories for {} ===\n",
        user_id
    );

    // List all memories for the user
    let memories = storage
        .list_user_memories(user_id, None, 1000)
        .await
        .expect("Failed to list memories");

    println!("Total memories for {}: {}\n", user_id, memories.len());

    // Search for any memory containing "coupon" or "Target"
    println!("=== Memories containing 'coupon' ===\n");
    let mut coupon_count = 0;
    for mem in &memories {
        let content_lower = mem.content.to_lowercase();
        if content_lower.contains("coupon") {
            coupon_count += 1;
            println!("Memory ID: {}", mem.id);
            println!("Content: {}", mem.content);
            println!("Entities: {:?}", mem.entity_ids);
            println!("---");
        }
    }
    println!("Found {} memories with 'coupon'\n", coupon_count);

    println!("=== Memories containing 'Target' (case-insensitive) ===\n");
    let mut target_count = 0;
    for mem in &memories {
        let content_lower = mem.content.to_lowercase();
        if content_lower.contains("target") {
            target_count += 1;
            println!("Memory ID: {}", mem.id);
            println!("Content: {}", mem.content);
            println!("Entities: {:?}", mem.entity_ids);
            println!("---");
        }
    }
    println!("Found {} memories with 'Target'\n", target_count);

    // Also check if "Target" appears in exact case
    println!("=== Memories containing 'Target' (exact case) ===\n");
    for mem in &memories {
        if mem.content.contains("Target") {
            println!("Content: {}", mem.content);
        }
    }
}

#[tokio::test]
async fn list_all_user_ids() {
    // Connect to Qdrant
    let config = QdrantConfig::external("http://localhost:6334").with_vector_size(1536);
    let storage = QdrantStorage::new(config)
        .await
        .expect("Failed to connect to Qdrant");

    println!("=== Collection counts ===");
    let counts = storage
        .get_collection_counts()
        .await
        .expect("Failed to get counts");

    for (coll, count) in counts {
        println!("{}: {} memories", coll, count);
    }
}
