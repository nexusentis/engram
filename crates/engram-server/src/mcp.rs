//! MCP mode: construct handler + run stdio transport.

use std::sync::Arc;

use engram_core::api::mcp::{McpHandler, StdioServer};
use engram_core::{Config, MemorySystem};

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

    let mut builder = MemorySystem::builder().qdrant_url(qdrant_url);
    if let Some(ref model) = config.extraction.api_model {
        builder = builder.extraction_model(model);
    }
    let system = builder.build().await?;

    let backend = Arc::new(system);
    let handle = tokio::runtime::Handle::current();

    let handler =
        McpHandler::with_backend("engram", env!("CARGO_PKG_VERSION"), backend, handle);

    // MCP stdio is synchronous line-based I/O. Run it on a blocking thread
    // so it doesn't starve the tokio runtime.
    let server = StdioServer::new(handler);

    tokio::task::spawn_blocking(move || server.run())
        .await
        .map_err(|e| anyhow::anyhow!("MCP task panicked: {e}"))??;

    Ok(())
}
