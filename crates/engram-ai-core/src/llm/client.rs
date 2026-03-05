//! HTTP-based LLM client with retry, backoff, and token refresh.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json;
use tracing;

use super::config::{LlmClientConfig, ModelProfile, ModelRegistry};
use super::types::{estimate_cost, AgentResponse, CompletionResult, ToolCall};
use crate::error::LlmError;

/// How the API key is sent in HTTP requests.
#[derive(Debug, Clone)]
pub enum AuthStyle {
    /// `Authorization: Bearer {key}` (OpenAI, Gemini, etc.)
    Bearer,
    /// Custom header with key sent as-is: `{header_name}: {key}`
    Header(String),
}

/// HTTP-based LLM client for OpenAI-compatible APIs.
///
/// Supports:
/// - 429/401/5xx retry with exponential backoff
/// - Token refresh via shell command (e.g. `gcloud auth print-access-token`)
/// - ModelProfile-based construction from TOML config
/// - Tool-calling completions
/// - Custom auth styles (Bearer, x-api-key, etc.)
#[derive(Debug, Clone)]
pub struct HttpLlmClient {
    api_key: Arc<Mutex<String>>,
    base_url: String,
    client: reqwest::Client,
    /// Shell command to refresh the API key (e.g. "gcloud auth print-access-token")
    token_cmd: Option<String>,
    /// When the current token was fetched
    token_fetched_at: Arc<Mutex<Option<Instant>>>,
    /// Optional model registry for profile-based request building
    model_registry: Option<Arc<ModelRegistry>>,
    /// Transport config (retries, backoff, timeouts)
    llm_config: LlmClientConfig,
    /// The model name this client was created for (used in API requests)
    model_name: Option<String>,
    /// How the API key is sent (default: Bearer)
    auth_style: AuthStyle,
    /// Extra headers added to every request (e.g. anthropic-version)
    extra_headers: Vec<(String, String)>,
}

impl HttpLlmClient {
    /// Create a new LLM client with the given API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, LlmError> {
        let llm_config = LlmClientConfig::default();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(llm_config.request_timeout_secs))
            .build()
            .map_err(LlmError::Request)?;

        Ok(Self {
            api_key: Arc::new(Mutex::new(api_key.into())),
            base_url: "https://api.openai.com/v1/chat/completions".to_string(),
            client,
            token_cmd: None,
            token_fetched_at: Arc::new(Mutex::new(None)),
            model_registry: None,
            llm_config,
            model_name: None,
            auth_style: AuthStyle::Bearer,
            extra_headers: Vec::new(),
        })
    }

    /// Build an HttpLlmClient from a ModelProfile (TOML-driven, no vendor defaults in Rust).
    /// `base_url` and `api_key_env` must be set in the [[models]] TOML entry.
    /// If `token_cmd_env` is set, the command is used to generate the initial token
    /// (and `api_key_env` becomes optional). Token refresh runs proactively at 45 min
    /// and reactively on 401.
    pub fn from_model_profile(
        profile: &ModelProfile,
        model_name: &str,
        registry: Option<Arc<ModelRegistry>>,
        llm_config: LlmClientConfig,
    ) -> Result<Self, LlmError> {
        let base_url = profile.base_url.clone().ok_or_else(|| {
            LlmError::Config(format!(
                "No base_url in [[models]] for '{}'. Add base_url to the model's TOML entry.",
                model_name
            ))
        })?;

        // Resolve token command first — if present, use it to generate the initial token
        let token_cmd = profile
            .token_cmd_env
            .as_ref()
            .and_then(|env_name| std::env::var(env_name).ok());

        let api_key = if let Some(ref api_key_env) = profile.api_key_env {
            // Try static API key from env var
            std::env::var(api_key_env).ok()
        } else {
            None
        };

        // If we have a token_cmd but no static key, generate initial token now
        let initial_token = if let Some(ref key) = api_key {
            key.clone()
        } else if let Some(ref cmd) = token_cmd {
            eprintln!(
                "[TOKEN] No static API key for '{}', generating initial token via token_cmd...",
                model_name
            );
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output()
                .map_err(|e| {
                    LlmError::TokenRefresh(format!(
                        "Failed to run token command for '{}': {}",
                        model_name, e
                    ))
                })?;
            if !output.status.success() {
                return Err(LlmError::TokenRefresh(format!(
                    "Token command failed for '{}': {}",
                    model_name,
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if token.is_empty() {
                return Err(LlmError::TokenRefresh(format!(
                    "Token command returned empty token for '{}'",
                    model_name
                )));
            }
            eprintln!(
                "[TOKEN] Initial token generated successfully for '{}'",
                model_name
            );
            token
        } else {
            let api_key_env = profile.api_key_env.as_deref().unwrap_or("(not set)");
            let token_cmd_hint = match &profile.token_cmd_env {
                Some(env_name) => format!(
                    " (token_cmd_env='{}' is configured but env var {} is not set)",
                    env_name, env_name
                ),
                None => " and no token_cmd_env configured".to_string(),
            };
            return Err(LlmError::NoApiKey(format!(
                "No API key for model '{}': env var {} not set{}",
                model_name, api_key_env, token_cmd_hint
            )));
        };

        let mut client = Self::new(initial_token)?
            .with_base_url(base_url)
            .with_llm_config(llm_config)?;
        client.model_name = Some(model_name.to_string());

        if let Some(reg) = registry {
            client = client.with_model_registry(reg);
        }

        // Wire token refresh command for proactive/reactive refresh
        if let Some(cmd) = token_cmd {
            client = client.with_token_cmd(cmd);
        }

        Ok(client)
    }

    /// Set transport config (retries, backoff, timeouts)
    pub fn with_llm_config(mut self, config: LlmClientConfig) -> Result<Self, LlmError> {
        // Rebuild HTTP client with new timeout
        self.client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .map_err(LlmError::Request)?;
        self.llm_config = config;
        Ok(self)
    }

    /// Set model registry for profile-based request building
    pub fn with_model_registry(mut self, registry: Arc<ModelRegistry>) -> Self {
        self.model_registry = Some(registry);
        self
    }

    /// Get the model name this client was created for.
    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }

    /// Set custom base URL (e.g. for Ollama)
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Set a shell command to refresh the API key (e.g. "gcloud auth print-access-token")
    pub fn with_token_cmd(mut self, cmd: impl Into<String>) -> Self {
        self.token_cmd = Some(cmd.into());
        *self.token_fetched_at.lock().unwrap() = Some(Instant::now());
        self
    }

    /// Set the auth style (default: Bearer). Use `AuthStyle::Header("x-api-key".into())`
    /// for APIs like Anthropic that use a custom header.
    pub fn with_auth_style(mut self, style: AuthStyle) -> Self {
        self.auth_style = style;
        self
    }

    /// Add an extra header sent on every request (e.g. `("anthropic-version", "2023-06-01")`).
    pub fn with_extra_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    /// Refresh the API key if a token_cmd is set and the token is older than the configured limit.
    /// Returns true if token was refreshed.
    fn maybe_refresh_token(&self) -> bool {
        self.do_refresh_token(false)
    }

    /// Force-refresh the API key regardless of age (e.g. after a 401).
    /// Returns true if token was refreshed.
    fn force_refresh_token(&self) -> bool {
        self.do_refresh_token(true)
    }

    fn do_refresh_token(&self, force: bool) -> bool {
        let Some(ref cmd) = self.token_cmd else {
            return false;
        };
        let fetched_at = *self.token_fetched_at.lock().unwrap();
        let should_refresh = force
            || match fetched_at {
                Some(t) => {
                    t.elapsed() > Duration::from_secs(self.llm_config.token_refresh_mins * 60)
                }
                None => true,
            };
        if !should_refresh {
            return false;
        }

        eprintln!(
            "[TOKEN] Refreshing access token (force={}, age: {:?})...",
            force,
            fetched_at.map(|t| t.elapsed())
        );
        match std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
        {
            Ok(output) if output.status.success() => {
                let new_token = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !new_token.is_empty() {
                    *self.api_key.lock().unwrap() = new_token;
                    *self.token_fetched_at.lock().unwrap() = Some(Instant::now());
                    eprintln!("[TOKEN] Refreshed successfully");
                    return true;
                }
            }
            Ok(output) => {
                eprintln!(
                    "[TOKEN] Refresh failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                eprintln!("[TOKEN] Refresh command failed: {}", e);
            }
        }
        false
    }

    /// Create from environment variable.
    ///
    /// Returns `None` if `OPENAI_API_KEY` is not set.
    /// Logs a warning and returns `None` if client initialization fails.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("OPENAI_API_KEY").ok()?;
        match Self::new(key) {
            Ok(client) => Some(client),
            Err(e) => {
                tracing::warn!("Failed to initialize HttpLlmClient: {e}");
                None
            }
        }
    }

    /// Check if the client has a valid API key
    pub fn has_api_key(&self) -> bool {
        !self.api_key.lock().unwrap().is_empty()
    }

    /// Max retries for 429 rate limit errors (from config, default 12)
    fn max_retries(&self) -> u32 {
        self.llm_config.max_retries
    }

    /// Backoff cap in seconds (from config, default 60)
    fn backoff_cap_secs(&self) -> u64 {
        self.llm_config.backoff_cap_secs
    }

    /// Vertex AI regions to rotate through on 429 rate limits.
    const VERTEX_REGIONS: &'static [&'static str] =
        &["global", "us-central1", "europe-west1", "asia-southeast1"];

    /// Send a JSON body to the API with 429/401 retry and exponential backoff.
    /// Returns the parsed JSON response.
    pub async fn send_request(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, LlmError> {
        self.maybe_refresh_token();
        let mut retries = 0u32;
        let mut token_refreshed_on_401 = false;
        let model_label = self
            .model_name
            .as_deref()
            .unwrap_or("unknown");
        let is_vertex = self.base_url.contains("aiplatform.googleapis.com");
        let mut current_url = self.base_url.clone();
        // Find the initial Vertex AI region index for rotation
        let mut region_idx: usize = if is_vertex {
            Self::VERTEX_REGIONS
                .iter()
                .position(|r| current_url.contains(&format!("locations/{}/", r)))
                .unwrap_or(0)
        } else {
            0
        };
        loop {
            let api_key = self.api_key.lock().unwrap().clone();
            let mut request = self
                .client
                .post(&current_url)
                .header("Content-Type", "application/json");

            // Set auth header based on style
            request = match &self.auth_style {
                AuthStyle::Bearer => {
                    request.header("Authorization", format!("Bearer {}", api_key))
                }
                AuthStyle::Header(header_name) => request.header(header_name.as_str(), &api_key),
            };

            // Add extra headers
            for (name, value) in &self.extra_headers {
                request = request.header(name.as_str(), value.as_str());
            }

            let result = request.json(body).send().await;

            match result {
                // 401 Unauthorized — try refreshing token once
                Ok(resp)
                    if resp.status().as_u16() == 401
                        && !token_refreshed_on_401
                        && self.token_cmd.is_some() =>
                {
                    let body_text = resp.text().await.unwrap_or_default();
                    eprintln!(
                        "[TOKEN] Got 401, attempting token refresh: {}",
                        body_text.get(..200).unwrap_or(&body_text)
                    );
                    if self.force_refresh_token() {
                        token_refreshed_on_401 = true;
                        // Don't count this as a retry, just loop with new token
                        continue;
                    }
                    return Err(LlmError::ApiError {
                        status: 401,
                        body: format!("token refresh failed: {}", body_text),
                    });
                }
                Ok(resp) if resp.status().as_u16() == 429 && retries < self.max_retries() => {
                    retries += 1;
                    let headers = resp.headers().clone();
                    let body_text = resp.text().await.unwrap_or_default();
                    let delay = Self::parse_retry_after(&headers, &body_text).unwrap_or_else(
                        || {
                            // Exponential backoff capped at configured limit
                            Duration::from_secs(2u64.pow(retries).min(self.backoff_cap_secs()))
                        },
                    );
                    // Rotate Vertex AI region on 429 to spread load
                    if is_vertex {
                        let old_region = Self::VERTEX_REGIONS[region_idx];
                        region_idx = (region_idx + 1) % Self::VERTEX_REGIONS.len();
                        let new_region = Self::VERTEX_REGIONS[region_idx];
                        current_url = current_url.replace(
                            &format!("locations/{}/", old_region),
                            &format!("locations/{}/", new_region),
                        );
                        eprintln!(
                            "[RETRY] Rate limited (429) on {}, rotating region {} -> {}, retrying in {:?} (attempt {}/{})",
                            model_label, old_region, new_region, delay, retries, self.max_retries()
                        );
                    } else {
                        eprintln!(
                            "[RETRY] Rate limited (429) on {}, retrying in {:?} (attempt {}/{})",
                            model_label, delay, retries, self.max_retries()
                        );
                    }
                    tokio::time::sleep(delay).await;
                }
                Ok(resp) if resp.status().as_u16() == 429 => {
                    let body_text = resp.text().await.unwrap_or_default();
                    return Err(LlmError::RateLimited {
                        retries: self.max_retries(),
                        body: body_text,
                    });
                }
                Ok(resp)
                    if resp.status().is_server_error() && retries < self.max_retries() =>
                {
                    retries += 1;
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    let delay =
                        Duration::from_secs(2u64.pow(retries).min(self.backoff_cap_secs()));
                    tracing::warn!(
                        "Server error ({}), retrying in {:?} (attempt {}/{}): {}",
                        status,
                        delay,
                        retries,
                        self.max_retries(),
                        body_text.get(..200).unwrap_or(&body_text)
                    );
                    tokio::time::sleep(delay).await;
                }
                Ok(resp) if !resp.status().is_success() => {
                    let status = resp.status().as_u16();
                    let body_text = resp.text().await.unwrap_or_default();
                    return Err(LlmError::ApiError {
                        status,
                        body: body_text,
                    });
                }
                Ok(resp) => {
                    return resp.json::<serde_json::Value>().await.map_err(|e| {
                        LlmError::InvalidResponse(format!("JSON parse error: {}", e))
                    });
                }
                Err(e) if retries < self.max_retries() => {
                    retries += 1;
                    let delay = 2u64.pow(retries);
                    tracing::warn!(
                        "Request failed: {}, retrying in {}s (attempt {}/{})",
                        e,
                        delay,
                        retries,
                        self.max_retries()
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
                Err(e) => {
                    return Err(LlmError::Request(e));
                }
            }
        }
    }

    /// Parse retry delay from response headers and body
    pub fn parse_retry_after(
        headers: &reqwest::header::HeaderMap,
        body: &str,
    ) -> Option<Duration> {
        // Try x-ratelimit-reset-tokens header (format: "6m0s", "1s", "500ms")
        if let Some(reset) = headers.get("x-ratelimit-reset-tokens") {
            if let Ok(s) = reset.to_str() {
                if let Some(d) = Self::parse_duration_string(s) {
                    return Some(d);
                }
            }
        }
        // Retry-After header (seconds)
        if let Some(retry) = headers.get("retry-after") {
            if let Ok(s) = retry.to_str() {
                if let Ok(secs) = s.parse::<u64>() {
                    return Some(Duration::from_secs(secs));
                }
            }
        }
        // Body: "try again in Xms" or "try again in Xs"
        if let Some(idx) = body.find("try again in ") {
            let after = &body[idx + 13..];
            if let Some(ms_end) = after.find("ms") {
                if let Ok(ms) = after[..ms_end].trim().parse::<f64>() {
                    return Some(Duration::from_millis(ms as u64));
                }
            }
            if let Some(s_end) = after.find('s') {
                if let Ok(secs) = after[..s_end].trim().parse::<f64>() {
                    return Some(Duration::from_millis((secs * 1000.0) as u64));
                }
            }
        }
        None
    }

    /// Parse duration strings like "6m0s", "1s", "500ms"
    pub fn parse_duration_string(s: &str) -> Option<Duration> {
        let mut total_ms: u64 = 0;
        let mut i = 0;
        let chars: Vec<char> = s.chars().collect();
        while i < chars.len() {
            let num_start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if num_start == i {
                i += 1;
                continue;
            }
            let num_str: String = chars[num_start..i].iter().collect();
            let num: f64 = num_str.parse().ok()?;
            let unit_start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let unit: String = chars[unit_start..i].iter().collect();
            match unit.as_str() {
                "ms" => total_ms += num as u64,
                "s" => total_ms += (num * 1000.0) as u64,
                "m" => total_ms += (num * 60_000.0) as u64,
                "h" => total_ms += (num * 3_600_000.0) as u64,
                _ => {}
            }
        }
        if total_ms > 0 {
            Some(Duration::from_millis(total_ms))
        } else {
            None
        }
    }

    /// Complete a prompt using OpenAI-compatible API
    pub async fn complete(
        &self,
        model: &str,
        prompt: &str,
        temperature: f32,
    ) -> Result<(String, f32), LlmError> {
        if !self.has_api_key() {
            return Err(LlmError::NoApiKey(
                "No API key configured".to_string(),
            ));
        }

        // Use ModelRegistry profile if available, else fall back to legacy heuristic
        let (max_tokens_field, supports_temp) = if let Some(ref registry) = self.model_registry {
            if let Ok(profile) = registry.get(model) {
                (
                    profile.max_tokens_field.as_str().to_string(),
                    profile.supports_temperature,
                )
            } else {
                let is_new = model.starts_with("gpt-5") || model.starts_with("o");
                (
                    if is_new {
                        "max_completion_tokens"
                    } else {
                        "max_tokens"
                    }
                    .to_string(),
                    !is_new,
                )
            }
        } else {
            let is_new = model.starts_with("gpt-5") || model.starts_with("o");
            (
                if is_new {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                }
                .to_string(),
                !is_new,
            )
        };
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
        });
        body[&max_tokens_field] = serde_json::json!(2000);
        if supports_temp {
            body["temperature"] = serde_json::json!(temperature);
        }

        let json = self.send_request(&body).await?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::InvalidResponse("Empty response".to_string()))?
            .to_string();

        let prompt_tokens = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let completion_tokens = json["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        let cost = estimate_cost(model, prompt_tokens, completion_tokens);

        Ok((content, cost))
    }

    /// Low-level completion: send a pre-built JSON body and get the content string back
    pub async fn raw_completion(&self, body: &serde_json::Value) -> Result<String, LlmError> {
        if !self.has_api_key() {
            return Err(LlmError::NoApiKey(
                "No API key configured".to_string(),
            ));
        }

        let json = self.send_request(body).await?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::InvalidResponse("Empty response".to_string()))?
            .to_string();

        Ok(content)
    }

    /// Complete with tool-calling support (for agentic loop)
    ///
    /// Sends messages + tool definitions to OpenAI API, returns either tool calls or text.
    pub async fn complete_with_tools(
        &self,
        model: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        temperature: f32,
    ) -> Result<CompletionResult, LlmError> {
        if !self.has_api_key() {
            return Err(LlmError::NoApiKey(
                "No API key configured".to_string(),
            ));
        }

        // Use ModelRegistry profile if available, else fall back to legacy heuristic
        let (max_tokens_field, supports_temp) = if let Some(ref registry) = self.model_registry {
            if let Ok(profile) = registry.get(model) {
                (
                    profile.max_tokens_field.as_str().to_string(),
                    profile.supports_temperature,
                )
            } else {
                let is_new = model.starts_with("gpt-5") || model.starts_with("o");
                (
                    if is_new {
                        "max_completion_tokens"
                    } else {
                        "max_tokens"
                    }
                    .to_string(),
                    !is_new && !model.contains("nano"),
                )
            }
        } else {
            let is_new = model.starts_with("gpt-5") || model.starts_with("o");
            (
                if is_new {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                }
                .to_string(),
                !is_new && !model.contains("nano"),
            )
        };
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "tools": tools,
        });
        body[&max_tokens_field] = serde_json::json!(4096);

        if supports_temp {
            body["temperature"] = serde_json::json!(temperature);
        }

        let json = self.send_request(&body).await?;

        let usage = &json["usage"];
        let prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
        let completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
        let cost = estimate_cost(model, prompt_tokens, completion_tokens);

        let choice = &json["choices"][0]["message"];

        let response = if let Some(tool_calls) = choice["tool_calls"].as_array() {
            let calls = tool_calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: serde_json::from_str(
                        tc["function"]["arguments"].as_str().unwrap_or("{}"),
                    )
                    .unwrap_or(serde_json::json!({})),
                    raw_json: Some(tc.clone()),
                })
                .collect();
            AgentResponse::ToolCalls(calls)
        } else {
            let content = choice["content"].as_str().unwrap_or("").to_string();
            AgentResponse::TextResponse(content)
        };

        Ok(CompletionResult {
            response,
            prompt_tokens,
            completion_tokens,
            cost,
        })
    }

    /// Synchronous wrapper for complete (for use in sync contexts)
    pub fn complete_sync(
        &self,
        model: &str,
        prompt: &str,
        temperature: f32,
    ) -> Result<(String, f32), LlmError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| LlmError::InvalidResponse(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(self.complete(model, prompt, temperature))
    }
}

// --- LlmClient trait impl ---

#[async_trait::async_trait]
impl super::traits::LlmClient for HttpLlmClient {
    async fn complete_with_tools(
        &self,
        model: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        temperature: f32,
    ) -> Result<CompletionResult, LlmError> {
        self.complete_with_tools(model, messages, tools, temperature)
            .await
    }

    fn model_name(&self) -> Option<&str> {
        self.model_name()
    }
}
