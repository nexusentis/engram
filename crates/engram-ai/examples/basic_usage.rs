//! Basic usage of the Engram memory system.
//!
//! Prerequisites:
//!   - Qdrant running on localhost:6334
//!   - OPENAI_API_KEY environment variable set
//!
//! Run:
//!   cargo run --example basic_usage -p engram

use engram_ai::{Conversation, ConversationTurn, Memory, MemorySystem};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the memory system (connects to Qdrant, initializes collections)
    let system = MemorySystem::builder()
        .qdrant_url("http://localhost:6334")
        .build()
        .await?;

    let user_id = "user_demo";

    // 1. Ingest a conversation — extracts facts automatically
    let conversation = Conversation::new(
        user_id,
        vec![
            ConversationTurn::user("I just started working at Anthropic on the safety team"),
            ConversationTurn::assistant("That's exciting! How are you finding it so far?"),
            ConversationTurn::user("It's great. I moved to San Francisco last month for the role"),
        ],
    );

    let result = system.ingest(conversation).await?;
    println!("Ingested {} memories: {:?}", result.memory_ids.len(), result.memory_ids);

    // 2. Search for relevant memories
    let results = system.search(user_id, "where does the user work?", 5).await?;
    println!("\nSearch results for 'where does the user work?':");
    for (memory, score) in &results {
        println!("  [{:.3}] {}", score, memory.content);
    }

    // 3. Store a simple fact directly (no extraction)
    let fact_id = system.store_fact(user_id, "User prefers dark mode").await?;
    println!("\nStored fact: {}", fact_id);

    // 4. Store a rich memory with full metadata control
    let mut memory = Memory::new(user_id, "User's favorite programming language is Rust");
    memory.entity_names = vec!["Rust".to_string()];
    let memory_id = system.store_memory(user_id, &memory).await?;
    println!("Stored rich memory: {}", memory_id);

    // 5. Retrieve by ID
    if let Some(retrieved) = system.get_memory(user_id, memory_id).await? {
        println!("\nRetrieved: {}", retrieved.content);
    }

    // 6. Delete
    let deleted = system.delete_memory(user_id, memory_id).await?;
    println!("Deleted: {}", deleted);

    Ok(())
}
