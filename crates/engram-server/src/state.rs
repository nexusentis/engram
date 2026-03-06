//! Application state shared across all handlers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use engram_agent::MemoryAgent;
use engram_core::api::mcp::McpHandler;
use engram_core::api::AuthConfig;
use engram_core::config::SecurityConfig;
use engram_core::extraction::{ApiExtractor, ApiExtractorConfig};
use engram_core::llm::HttpLlmClient;
use engram_core::storage::QdrantStorage;
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
    pub auth_config: AuthConfig,
    pub security: SecurityConfig,
    /// Backend for MCP HTTP sessions (reuses same Qdrant/embedder/extractor).
    pub mcp_backend: Arc<MemorySystem>,
    /// Active MCP sessions keyed by session ID.
    pub mcp_sessions: Arc<tokio::sync::RwLock<HashMap<String, McpSession>>>,
    /// Optional memory answering agent (enabled when `[agent]` config present + LLM key set).
    pub memory_agent: Option<Arc<MemoryAgent>>,
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

        // MemorySystem for MCP handler backend (shares Qdrant/embedder/extractor)
        let mcp_backend = Arc::new(MemorySystem::new(
            qdrant.clone(),
            embedder.clone(),
            extractor.clone(),
        ));

        // Optional MemoryAgent (requires [agent] config section + LLM API key)
        let memory_agent = Self::build_memory_agent(config, &qdrant, &embedder);

        Ok(Self {
            qdrant,
            embedder,
            extractor,
            auth_config,
            security,
            mcp_backend,
            mcp_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_agent,
        })
    }

    /// Build a MemoryAgent from config if `[agent]` section is present and an LLM key is available.
    /// Returns None gracefully if config or credentials are missing.
    fn build_memory_agent(
        config: &Config,
        qdrant: &Arc<QdrantStorage>,
        embedder: &Arc<dyn EmbeddingProvider>,
    ) -> Option<Arc<MemoryAgent>> {
        let agent_config = config.agent.as_ref()?;

        // Build LLM client — need OPENAI_API_KEY from env
        let api_key = match std::env::var("OPENAI_API_KEY") {
            Ok(key) => key,
            Err(_) => {
                tracing::warn!(
                    "Memory agent disabled: OPENAI_API_KEY not set. \
                     Set the env var to enable the memory_answer tool."
                );
                return None;
            }
        };

        let llm: Arc<dyn engram_core::llm::LlmClient> = match HttpLlmClient::new(&api_key) {
            Ok(client) => {
                tracing::info!(
                    model = %agent_config.model,
                    "Memory agent enabled (memory_answer tool available)"
                );
                Arc::new(client)
            }
            Err(e) => {
                tracing::warn!("Memory agent disabled: LLM client init failed: {}", e);
                return None;
            }
        };

        let agent = MemoryAgent::new(
            agent_config.clone(),
            Arc::clone(qdrant),
            Arc::clone(embedder),
            llm,
        );

        // Optional: build fallback LLM for ensemble
        let agent = if let Some(ref ensemble) = agent_config.ensemble {
            if ensemble.enabled {
                match HttpLlmClient::new(&api_key) {
                    Ok(fallback) => {
                        tracing::info!(
                            fallback_model = %ensemble.fallback_model,
                            "Ensemble fallback enabled"
                        );
                        agent.with_fallback_llm(Arc::new(fallback))
                    }
                    Err(e) => {
                        tracing::warn!("Ensemble fallback disabled: {}", e);
                        agent
                    }
                }
            } else {
                agent
            }
        } else {
            agent
        };

        Some(Arc::new(agent))
    }

    /// MCP tool definition for `memory_answer`.
    pub fn memory_answer_tool_def() -> engram_core::api::mcp::Tool {
        engram_core::api::mcp::Tool::new(
            "memory_answer",
            "Answer a natural language question using the user's stored memories. Uses multi-strategy agentic search with quality validation.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string",
                        "description": "User identifier"
                    },
                    "question": {
                        "type": "string",
                        "description": "Natural language question"
                    },
                    "reference_time": {
                        "type": "string",
                        "format": "date-time",
                        "description": "Optional temporal anchor for relative date questions (ISO 8601)"
                    }
                },
                "required": ["user_id", "question"]
            }),
        )
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
