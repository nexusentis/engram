//! Application state shared across all handlers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use engram_core::api::mcp::McpHandler;
use engram_core::api::AuthConfig;
use engram_core::config::SecurityConfig;
use engram_core::extraction::{ApiExtractor, ApiExtractorConfig};
use engram_core::storage::{Database, QdrantStorage};
use engram_core::{Config, EmbeddingProvider, MemorySystem, RemoteEmbeddingProvider};

/// An MCP session: handler + last-active timestamp for TTL reaping.
pub struct McpSession {
    pub handler: Arc<std::sync::Mutex<McpHandler>>,
    pub last_active: Instant,
}

/// Shared application state.
///
/// All fields are `Arc`-wrapped so the struct is cheaply cloneable
/// (axum requires `Clone` on state).
#[derive(Clone)]
pub struct AppState {
    pub qdrant: Arc<QdrantStorage>,
    pub embedder: Arc<dyn EmbeddingProvider>,
    pub extractor: Arc<ApiExtractor>,
    pub database: Arc<std::sync::Mutex<Database>>,
    pub auth_config: AuthConfig,
    pub security: SecurityConfig,
    /// Backend for MCP HTTP sessions (reuses same Qdrant/embedder/extractor).
    pub mcp_backend: Arc<MemorySystem>,
    /// Active MCP sessions keyed by session ID.
    pub mcp_sessions: Arc<tokio::sync::RwLock<HashMap<String, McpSession>>>,
}

impl AppState {
    /// Build state from the engram Config + environment.
    ///
    /// Requires `OPENAI_API_KEY` to be set (used for embeddings and extraction).
    pub async fn from_config(config: &Config, require_auth: bool) -> anyhow::Result<Self> {
        // Auth — check first so --require-auth fails before slow external init
        let auth_config = Self::load_auth_config();
        if require_auth && !auth_config.enabled {
            anyhow::bail!(
                "--require-auth is set but ENGRAM_API_TOKENS is not configured. \
                 Set ENGRAM_API_TOKENS=token1,token2 before starting."
            );
        }
        if !auth_config.enabled {
            tracing::warn!("Auth is DISABLED — set ENGRAM_API_TOKENS to enable");
        }

        let security = config.server.security.clone();

        // Qdrant — env var overrides TOML config (matches MCP path behavior)
        let mut qdrant_config = config.qdrant.clone();
        if let Ok(url) = std::env::var("ENGRAM_QDRANT_URL") {
            qdrant_config.url = Some(url);
        }
        let qdrant = Arc::new(QdrantStorage::new(qdrant_config).await?);

        // Embedder (OpenAI)
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
        let embedder: Arc<dyn EmbeddingProvider> =
            Arc::new(RemoteEmbeddingProvider::new(api_key.clone(), None)?);

        // Extractor
        let model = config
            .extraction
            .api_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());
        let extractor_config = ApiExtractorConfig::openai(&model).with_api_key(api_key);
        let extractor = Arc::new(ApiExtractor::new(extractor_config));

        // SQLite
        let database = Database::open(&config.sqlite)?;
        let database = Arc::new(std::sync::Mutex::new(database));

        // MemorySystem for MCP handler backend (shares Qdrant/embedder/extractor)
        let mcp_backend = Arc::new(MemorySystem::new(
            qdrant.clone(),
            embedder.clone(),
            extractor.clone(),
        ));

        Ok(Self {
            qdrant,
            embedder,
            extractor,
            database,
            auth_config,
            security,
            mcp_backend,
            mcp_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    fn load_auth_config() -> AuthConfig {
        match std::env::var("ENGRAM_API_TOKENS") {
            Ok(tokens_str) => {
                let tokens: Vec<String> = tokens_str
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .map(|t| engram_core::api::hash_token(&t))
                    .collect();
                if tokens.is_empty() {
                    tracing::warn!(
                        "ENGRAM_API_TOKENS is set but contains no valid tokens; auth disabled"
                    );
                    AuthConfig::disabled()
                } else {
                    AuthConfig::enabled(tokens)
                }
            }
            Err(_) => AuthConfig::disabled(),
        }
    }
}
