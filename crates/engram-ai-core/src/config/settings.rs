use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::ConfigError;
use crate::storage::{QdrantConfig, SqliteConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub data_dir: String,
    pub qdrant: QdrantConfig,
    pub sqlite: SqliteConfig,
    pub server: ServerConfig,
    pub extraction: ExtractionConfig,
    pub retrieval: RetrievalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Max request body size in bytes (default: 2 MiB)
    #[serde(default = "default_body_limit")]
    pub body_limit_bytes: usize,
    /// CORS allowed origins. Empty = same-origin only. ["*"] = allow all.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Request timeout in seconds (default: 60)
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
    /// MCP session idle TTL in seconds (default: 1800 = 30 min)
    #[serde(default = "default_session_ttl")]
    pub mcp_session_ttl_secs: u64,
}

fn default_body_limit() -> usize {
    2 * 1024 * 1024
}

fn default_request_timeout() -> u64 {
    60
}

fn default_session_ttl() -> u64 {
    1800
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            body_limit_bytes: default_body_limit(),
            cors_origins: Vec::new(),
            request_timeout_secs: default_request_timeout(),
            mcp_session_ttl_secs: default_session_ttl(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionConfig {
    /// "local-fast", "local-accurate", or "api-sota"
    pub mode: String,
    pub api_provider: Option<String>,
    pub api_model: Option<String>,
    pub confidence_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalConfig {
    /// RRF k parameter for fusion
    pub rrf_k: u32,
    /// Number of results to return
    pub top_k: u32,
    /// Confidence threshold for abstention
    pub abstention_threshold: f32,
}

impl RetrievalConfig {
    /// Load retrieval config from environment variables with fallback to defaults
    pub fn from_env() -> Self {
        use std::env;

        Self {
            rrf_k: env::var("RRF_K")
                .map(|v| v.parse().unwrap_or(60))
                .unwrap_or(60),
            top_k: env::var("RETRIEVAL_TOP_K")
                .map(|v| v.parse().unwrap_or(20))
                .unwrap_or(20),
            abstention_threshold: env::var("ABSTENTION_THRESHOLD")
                .map(|v| v.parse().unwrap_or(0.4))
                .unwrap_or(0.4),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            qdrant: QdrantConfig::default(),
            sqlite: SqliteConfig::default(),
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                security: SecurityConfig::default(),
            },
            extraction: ExtractionConfig {
                mode: "local-accurate".to_string(),
                api_provider: None,
                api_model: None,
                confidence_threshold: 0.5,
            },
            retrieval: RetrievalConfig {
                rrf_k: 60,
                top_k: 20,
                abstention_threshold: 0.4,
            },
        }
    }
}

impl Config {
    /// Load config from file, falling back to defaults
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                    tracing::warn!("Failed to parse config: {}, using defaults", e);
                    Self::default()
                }),
                Err(e) => {
                    tracing::warn!("Failed to read config: {}, using defaults", e);
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }

    /// Save config to file
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let content = toml::to_string_pretty(self).map_err(std::io::Error::other)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)
    }

    /// Get a config value by dot-separated key
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "data_dir" => Some(self.data_dir.clone()),
            "server.host" => Some(self.server.host.clone()),
            "server.port" => Some(self.server.port.to_string()),
            "qdrant.mode" => Some(self.qdrant.mode.clone()),
            "extraction.mode" => Some(self.extraction.mode.clone()),
            "extraction.confidence_threshold" => {
                Some(self.extraction.confidence_threshold.to_string())
            }
            "retrieval.top_k" => Some(self.retrieval.top_k.to_string()),
            _ => None,
        }
    }

    /// Set a config value by dot-separated key
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "data_dir" => self.data_dir = value.to_string(),
            "server.host" => self.server.host = value.to_string(),
            "server.port" => self.server.port = value.parse().map_err(|_| "Invalid port")?,
            "qdrant.mode" => self.qdrant.mode = value.to_string(),
            "extraction.mode" => self.extraction.mode = value.to_string(),
            "extraction.confidence_threshold" => {
                self.extraction.confidence_threshold =
                    value.parse().map_err(|_| "Invalid threshold")?;
            }
            "retrieval.top_k" => {
                self.retrieval.top_k = value.parse().map_err(|_| "Invalid number")?;
            }
            _ => return Err(format!("Unknown config key: {}", key)),
        }
        Ok(())
    }

    /// Load config from a file, returning an error if the file is missing or invalid.
    ///
    /// Unlike [`load`](Self::load), this does not silently fall back to defaults.
    /// After loading, runs [`validate`](Self::validate) to catch semantic errors.
    pub fn load_strict(path: &Path) -> std::result::Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            ConfigError::FileRead(format!("{}: {}", path.display(), e))
        })?;
        let config: Self = toml::from_str(&content).map_err(|e| {
            ConfigError::Parse(format!("{}: {}", path.display(), e))
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Validate semantic constraints that deserialization alone cannot enforce.
    pub fn validate(&self) -> std::result::Result<(), ConfigError> {
        let mut errors = Vec::new();

        // Qdrant
        if self.qdrant.vector_size == 0 {
            errors.push("qdrant.vector_size must be > 0".to_string());
        }
        match self.qdrant.mode.as_str() {
            "external" => {
                if self.qdrant.url.is_none() {
                    errors.push("qdrant.url is required when mode = \"external\"".to_string());
                }
            }
            "embedded" => {
                if self.qdrant.path.is_none() {
                    errors.push("qdrant.path is required when mode = \"embedded\"".to_string());
                }
            }
            other => {
                errors.push(format!(
                    "qdrant.mode must be \"external\" or \"embedded\", got \"{}\"",
                    other
                ));
            }
        }

        // Retrieval
        if self.retrieval.rrf_k == 0 {
            errors.push("retrieval.rrf_k must be > 0".to_string());
        }
        if self.retrieval.top_k == 0 {
            errors.push("retrieval.top_k must be > 0".to_string());
        }
        if !is_valid_probability(self.retrieval.abstention_threshold) {
            errors.push(format!(
                "retrieval.abstention_threshold must be in [0.0, 1.0], got {}",
                self.retrieval.abstention_threshold
            ));
        }

        // Security
        if self.server.security.body_limit_bytes == 0 {
            errors.push("server.security.body_limit_bytes must be > 0".to_string());
        }
        if self.server.security.request_timeout_secs == 0 {
            errors.push("server.security.request_timeout_secs must be > 0".to_string());
        }
        if self.server.security.mcp_session_ttl_secs == 0 {
            errors.push("server.security.mcp_session_ttl_secs must be > 0".to_string());
        }

        // Extraction
        if !is_valid_probability(self.extraction.confidence_threshold) {
            errors.push(format!(
                "extraction.confidence_threshold must be in [0.0, 1.0], got {}",
                self.extraction.confidence_threshold
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(errors.join("; ")))
        }
    }
}

/// Check that a float is a finite number in [0.0, 1.0].
fn is_valid_probability(v: f32) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.data_dir, "./data");
        assert_eq!(config.server.port, 8080);
    }

    #[test]
    fn test_config_get() {
        let config = Config::default();
        assert_eq!(config.get("data_dir"), Some("./data".to_string()));
        assert_eq!(config.get("server.port"), Some("8080".to_string()));
        assert_eq!(config.get("unknown"), None);
    }

    #[test]
    fn test_config_set() {
        let mut config = Config::default();
        config.set("server.port", "9000").unwrap();
        assert_eq!(config.server.port, 9000);
    }

    #[test]
    fn test_config_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let mut config = Config::default();
        config.server.port = 9999;
        config.save(&config_path).unwrap();

        let loaded = Config::load(&config_path);
        assert_eq!(loaded.server.port, 9999);
    }

    #[test]
    fn test_config_load_nonexistent() {
        let config = Config::load(Path::new("/nonexistent/config.toml"));
        assert_eq!(config.server.port, 8080); // Default
    }

    #[test]
    fn test_retrieval_config_default() {
        let config = Config::default();
        assert_eq!(config.retrieval.rrf_k, 60);
        assert_eq!(config.retrieval.top_k, 20);
        assert_eq!(config.retrieval.abstention_threshold, 0.4);
    }

    #[test]
    fn test_retrieval_config_from_env() {
        use std::env;

        env::set_var("RETRIEVAL_TOP_K", "15");

        let config = RetrievalConfig::from_env();
        assert_eq!(config.top_k, 15);

        // Clean up
        env::remove_var("RETRIEVAL_TOP_K");
    }

    #[test]
    fn test_validate_default_ok() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_zero_vector_size() {
        let mut config = Config::default();
        config.qdrant.vector_size = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("vector_size"));
    }

    #[test]
    fn test_validate_external_needs_url() {
        let mut config = Config::default();
        config.qdrant.mode = "external".to_string();
        config.qdrant.url = None;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("url is required"));
    }

    #[test]
    fn test_validate_embedded_needs_path() {
        let mut config = Config::default();
        config.qdrant.mode = "embedded".to_string();
        config.qdrant.path = None;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("path is required"));
    }

    #[test]
    fn test_validate_bad_thresholds() {
        let mut config = Config::default();
        config.retrieval.abstention_threshold = 1.5;
        config.extraction.confidence_threshold = -0.1;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("abstention_threshold"));
        assert!(msg.contains("confidence_threshold"));
    }

    #[test]
    fn test_validate_nan_threshold() {
        let mut config = Config::default();
        config.retrieval.abstention_threshold = f32::NAN;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_zero_rrf_k() {
        let mut config = Config::default();
        config.retrieval.rrf_k = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("rrf_k"));
    }

    #[test]
    fn test_load_strict_nonexistent() {
        let result = Config::load_strict(Path::new("/nonexistent/config.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_strict_valid() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let config = Config::default();
        config.save(&config_path).unwrap();

        let loaded = Config::load_strict(&config_path).unwrap();
        assert_eq!(loaded.server.port, 8080);
    }

    #[test]
    fn test_load_strict_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(&config_path, "not valid { toml").unwrap();

        let result = Config::load_strict(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_zero_request_timeout() {
        let mut config = Config::default();
        config.server.security.request_timeout_secs = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("request_timeout_secs"));
    }

    #[test]
    fn test_validate_zero_session_ttl() {
        let mut config = Config::default();
        config.server.security.mcp_session_ttl_secs = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("mcp_session_ttl_secs"));
    }
}
