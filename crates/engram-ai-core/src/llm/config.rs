//! LLM client configuration types: model profiles, registry, and transport config.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Model profiles
// ---------------------------------------------------------------------------

/// Per-model configuration — replaces all `if model.contains(...)` branching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub name: String,
    /// Base URL for API calls. Required — must be set in TOML.
    #[serde(default)]
    pub base_url: Option<String>,
    /// JSON field name for max tokens: "max_tokens" or "max_completion_tokens"
    #[serde(default = "default_max_tokens_field")]
    pub max_tokens_field: String,
    /// Whether the model supports the temperature parameter
    #[serde(default = "default_true")]
    pub supports_temperature: bool,
    /// Env var name containing the API key (e.g. "OPENAI_API_KEY"). Required — must be set in TOML.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Env var name containing a shell command to refresh the token.
    #[serde(default)]
    pub token_cmd_env: Option<String>,
    /// Price per 1M prompt tokens in USD
    #[serde(default = "default_prompt_price")]
    pub prompt_price_per_m: f32,
    /// Price per 1M completion tokens in USD
    #[serde(default = "default_completion_price")]
    pub completion_price_per_m: f32,
}

fn default_max_tokens_field() -> String {
    "max_tokens".to_string()
}
fn default_true() -> bool {
    true
}
fn default_prompt_price() -> f32 {
    1.00
}
fn default_completion_price() -> f32 {
    2.00
}

// ---------------------------------------------------------------------------
// ModelRegistry
// ---------------------------------------------------------------------------

/// Registry of model profiles — fail-fast lookup, no silent fallback.
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    profiles: HashMap<String, ModelProfile>,
}

impl ModelRegistry {
    pub fn from_config(models: Vec<ModelProfile>) -> Self {
        let mut profiles = HashMap::new();
        for m in models {
            profiles.insert(m.name.clone(), m);
        }
        Self { profiles }
    }

    /// Look up a model profile. Tries exact match, then longest prefix match.
    /// Returns an error if no profile matches (fail-fast, no silent default).
    pub fn get(&self, model_name: &str) -> anyhow::Result<&ModelProfile> {
        if let Some(profile) = self.profiles.get(model_name) {
            return Ok(profile);
        }
        // Deterministic longest-match: sort keys by length desc to avoid HashMap iteration order
        let mut candidates: Vec<_> = self
            .profiles
            .iter()
            .filter(|(k, _)| model_name.starts_with(k.as_str()))
            .collect();
        candidates.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()));
        if let Some((_, profile)) = candidates.first() {
            return Ok(profile);
        }
        Err(anyhow::anyhow!(
            "No ModelProfile for '{}'. Add a [[models]] entry to your benchmark config.",
            model_name
        ))
    }

    /// Estimate API cost using the model's pricing profile.
    pub fn estimate_cost(
        &self,
        model: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) -> anyhow::Result<f32> {
        let profile = self.get(model)?;
        let prompt_cost = (prompt_tokens as f32 / 1_000_000.0) * profile.prompt_price_per_m;
        let completion_cost =
            (completion_tokens as f32 / 1_000_000.0) * profile.completion_price_per_m;
        Ok(prompt_cost + completion_cost)
    }

    /// Check if a model name has a profile registered.
    pub fn has(&self, model_name: &str) -> bool {
        self.get(model_name).is_ok()
    }
}

// ---------------------------------------------------------------------------
// Transport config
// ---------------------------------------------------------------------------

/// LLM HTTP client transport configuration (retries, backoff, timeouts).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmClientConfig {
    pub max_retries: u32,
    pub backoff_cap_secs: u64,
    pub token_refresh_mins: u64,
    pub request_timeout_secs: u64,
}

impl Default for LlmClientConfig {
    fn default() -> Self {
        Self {
            max_retries: 12,
            backoff_cap_secs: 60,
            token_refresh_mins: 45,
            request_timeout_secs: 90,
        }
    }
}
