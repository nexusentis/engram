use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Configuration for API-based extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiExtractorConfig {
    pub provider: ApiProvider,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_retries: u32,
    pub timeout_seconds: u64,
    /// Temperature for extraction (None = use model default)
    pub temperature: Option<f32>,
    /// Seed for deterministic extraction (None = omit)
    pub seed: Option<u64>,
    /// Directory for caching LLM extraction responses.
    /// When set, API responses are cached by SHA-256 of the request body.
    /// Cache hits skip the LLM call entirely, making ingestion deterministic.
    #[serde(skip)]
    pub cache_dir: Option<PathBuf>,
    /// Whether the model supports temperature parameter (None = auto-detect from model name)
    #[serde(default)]
    pub supports_temperature: Option<bool>,
    /// JSON field name for max tokens (None = auto-detect from model name)
    #[serde(default)]
    pub max_tokens_field: Option<String>,
}

/// Supported API providers
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ApiProvider {
    Anthropic,
    OpenAI,
    Custom,
}

impl Default for ApiExtractorConfig {
    fn default() -> Self {
        Self {
            provider: ApiProvider::Anthropic,
            model: "claude-3-haiku-20240307".to_string(),
            api_key: None,
            base_url: None,
            max_retries: 3,
            timeout_seconds: 90,
            temperature: None,
            seed: None,
            cache_dir: None,
            supports_temperature: None,
            max_tokens_field: None,
        }
    }
}

impl ApiExtractorConfig {
    /// Create config for Anthropic Claude
    pub fn anthropic(model: &str) -> Self {
        Self {
            provider: ApiProvider::Anthropic,
            model: model.to_string(),
            ..Default::default()
        }
    }

    /// Create config for OpenAI GPT
    pub fn openai(model: &str) -> Self {
        Self {
            provider: ApiProvider::OpenAI,
            model: model.to_string(),
            base_url: Some("https://api.openai.com/v1/chat/completions".to_string()),
            ..Default::default()
        }
    }

    /// Create config for custom endpoint
    pub fn custom(model: &str, base_url: &str) -> Self {
        Self {
            provider: ApiProvider::Custom,
            model: model.to_string(),
            base_url: Some(base_url.to_string()),
            ..Default::default()
        }
    }

    /// Set API key
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = seconds;
        self
    }

    /// Set max retries
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set temperature for extraction
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set seed for deterministic extraction
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Set cache directory for deterministic extraction.
    /// When set, LLM responses are cached by request hash.
    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ApiExtractorConfig::default();
        assert_eq!(config.provider, ApiProvider::Anthropic);
        assert_eq!(config.model, "claude-3-haiku-20240307");
        assert!(config.api_key.is_none());
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.timeout_seconds, 90);
    }

    #[test]
    fn test_anthropic_config() {
        let config = ApiExtractorConfig::anthropic("claude-3-sonnet-20240229");
        assert_eq!(config.provider, ApiProvider::Anthropic);
        assert_eq!(config.model, "claude-3-sonnet-20240229");
    }

    #[test]
    fn test_openai_config() {
        let config = ApiExtractorConfig::openai("gpt-4o-mini");
        assert_eq!(config.provider, ApiProvider::OpenAI);
        assert_eq!(config.model, "gpt-4o-mini");
        assert!(config.base_url.is_some());
    }

    #[test]
    fn test_custom_config() {
        let config = ApiExtractorConfig::custom("local-model", "http://localhost:8080/v1");
        assert_eq!(config.provider, ApiProvider::Custom);
        assert_eq!(
            config.base_url,
            Some("http://localhost:8080/v1".to_string())
        );
    }

    #[test]
    fn test_config_builder() {
        let config = ApiExtractorConfig::anthropic("claude-3-haiku-20240307")
            .with_api_key("test-key")
            .with_timeout(60)
            .with_max_retries(5);

        assert_eq!(config.api_key, Some("test-key".to_string()));
        assert_eq!(config.timeout_seconds, 60);
        assert_eq!(config.max_retries, 5);
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = ApiExtractorConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ApiExtractorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider, config.provider);
        assert_eq!(parsed.model, config.model);
    }
}
