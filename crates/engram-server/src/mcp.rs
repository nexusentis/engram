//! MCP mode: construct handler + run stdio transport.

use std::sync::Arc;

use engram_agent::MemoryAgent;
use engram_core::api::mcp::{McpHandler, StdioServer, ToolResult};
use engram_core::llm::HttpLlmClient;
use engram_core::storage::QdrantStorage;
use engram_core::{Config, EmbeddingProvider, MemorySystem, RemoteEmbeddingProvider};

use crate::state::AppState;

/// Run the MCP server over stdio (blocking the current task).
///
/// Builds a real `MemorySystem` backend from the supplied config and wires
/// it into the MCP handler. Requires `OPENAI_API_KEY` to be set.
pub async fn serve_mcp(config: &Config) -> anyhow::Result<()> {
    tracing::info!("Starting engram MCP server (stdio)");

    let qdrant_url = std::env::var("ENGRAM_QDRANT_URL")
        .ok()
        .or_else(|| config.qdrant.url.clone())
        .unwrap_or_else(|| "http://localhost:6334".to_string());

    let mut builder = MemorySystem::builder().qdrant_url(qdrant_url.clone());
    if let Some(ref model) = config.extraction.api_model {
        builder = builder.extraction_model(model);
    }
    let system = builder.build().await?;

    let backend = Arc::new(system);
    let handle = tokio::runtime::Handle::current();

    let mut handler =
        McpHandler::with_backend("engram", env!("CARGO_PKG_VERSION"), backend, handle.clone());

    // Inject memory_answer tool if [agent] config + LLM key available.
    if let Some(ref agent_config) = config.agent {
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            let mut qdrant_config = config.qdrant.clone();
            qdrant_config.url = Some(qdrant_url);
            let qdrant = Arc::new(QdrantStorage::new(qdrant_config).await?);
            let embedder: Arc<dyn EmbeddingProvider> =
                Arc::new(RemoteEmbeddingProvider::new(api_key.clone(), None)?);

            if let Ok(llm_client) = HttpLlmClient::new(&api_key) {
                let llm: Arc<dyn engram_core::llm::LlmClient> = Arc::new(llm_client);
                let agent = Arc::new(MemoryAgent::new(
                    agent_config.clone(),
                    qdrant,
                    embedder,
                    llm,
                ));

                tracing::info!("memory_answer tool enabled (MCP stdio)");
                handler = handler.with_extra_tools(
                    vec![AppState::memory_answer_tool_def()],
                    move |_name, args| {
                        let user_id = match args.get("user_id").and_then(|v| v.as_str()) {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => return ToolResult::error(
                                "missing required parameter: user_id",
                            ),
                        };
                        let question = match args.get("question").and_then(|v| v.as_str()) {
                            Some(q) if !q.is_empty() => q.to_string(),
                            _ => return ToolResult::error(
                                "missing required parameter: question",
                            ),
                        };
                        let ref_time: Option<chrono::DateTime<chrono::Utc>> = args
                            .get("reference_time")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok());

                        let agent = Arc::clone(&agent);
                        match handle.block_on(agent.answer(&question, &user_id, ref_time)) {
                            Ok(result) => ToolResult::text(
                                serde_json::json!({
                                    "answer": result.answer,
                                    "abstained": result.abstained,
                                    "strategy": format!("{:?}", result.strategy),
                                    "iterations": result.iterations,
                                    "total_time_ms": result.total_time_ms,
                                    "fallback_used": result.fallback_used,
                                })
                                .to_string(),
                            ),
                            Err(e) => ToolResult::error(
                                format!("memory_answer failed: {e}"),
                            ),
                        }
                    },
                );
            }
        }
    }

    // MCP stdio is synchronous line-based I/O. Run it on a blocking thread
    // so it doesn't starve the tokio runtime.
    let server = StdioServer::new(handler);

    tokio::task::spawn_blocking(move || server.run())
        .await
        .map_err(|e| anyhow::anyhow!("MCP task panicked: {e}"))??;

    Ok(())
}
