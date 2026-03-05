//! Authentication utilities
//!
//! Provides bearer token authentication utilities for the API.
//!
//! ## Token Format
//!
//! Authorization headers should use the Bearer token format:
//! ```text
//! Authorization: Bearer <token>
//! ```
//!
//! ## Security
//!
//! - Tokens are stored as SHA-256 hashes, never in plaintext
//! - Token verification uses constant-time comparison
//! - Skip paths allow certain endpoints to bypass authentication

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Whether authentication is enabled
    pub enabled: bool,
    /// Hashed tokens (use `hash_token` to create these)
    #[serde(default)]
    pub tokens: Vec<String>,
    /// Paths that skip authentication (supports wildcard suffix `*`)
    #[serde(default)]
    pub skip_paths: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tokens: Vec::new(),
            skip_paths: default_skip_paths(),
        }
    }
}

fn default_skip_paths() -> Vec<String> {
    vec![
        "/health".to_string(),
        "/metrics".to_string(),
        "/openapi.json".to_string(),
        "/mcp".to_string(),
    ]
}

impl AuthConfig {
    /// Create a new auth config with authentication enabled
    pub fn enabled(tokens: Vec<String>) -> Self {
        Self {
            enabled: true,
            tokens,
            skip_paths: default_skip_paths(),
        }
    }

    /// Create a disabled auth config
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Add a token (will be hashed)
    pub fn with_token(mut self, token: &str) -> Self {
        self.tokens.push(hash_token(token));
        self
    }

    /// Add a hashed token directly
    pub fn with_hashed_token(mut self, hash: String) -> Self {
        self.tokens.push(hash);
        self
    }

    /// Add paths to skip authentication
    pub fn with_skip_paths(mut self, paths: Vec<String>) -> Self {
        self.skip_paths.extend(paths);
        self
    }

    /// Check if authentication should be skipped for a path
    pub fn should_skip(&self, path: &str) -> bool {
        if !self.enabled {
            return true;
        }
        should_skip_auth(path, &self.skip_paths)
    }

    /// Validate a token
    pub fn validate(&self, token: &str) -> bool {
        if !self.enabled {
            return true;
        }
        verify_token(token, &self.tokens)
    }
}

/// Authentication error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthError {
    /// Error message
    pub error: String,
    /// Error code
    pub code: String,
}

impl AuthError {
    /// Create a new auth error
    pub fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
        }
    }

    /// Create an unauthorized error
    pub fn unauthorized() -> Self {
        Self::new("Unauthorized", "UNAUTHORIZED")
    }

    /// Create a missing auth header error
    pub fn missing_header() -> Self {
        Self::new("Missing Authorization header", "UNAUTHORIZED")
    }

    /// Create an invalid token error
    pub fn invalid_token() -> Self {
        Self::new("Invalid token", "UNAUTHORIZED")
    }

    /// Create an invalid header format error
    pub fn invalid_format() -> Self {
        Self::new(
            "Invalid Authorization header format. Expected: Bearer <token>",
            "UNAUTHORIZED",
        )
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.error, self.code)
    }
}

impl std::error::Error for AuthError {}

/// Authentication state for request extensions
#[derive(Debug, Clone, Default)]
pub struct AuthState {
    /// Whether the request is authenticated
    pub authenticated: bool,
    /// Hash of the token used (if authenticated)
    pub token_hash: Option<String>,
}

impl AuthState {
    /// Create authenticated state
    pub fn authenticated(token_hash: String) -> Self {
        Self {
            authenticated: true,
            token_hash: Some(token_hash),
        }
    }

    /// Create unauthenticated state
    pub fn unauthenticated() -> Self {
        Self::default()
    }

    /// Create state for skipped authentication
    pub fn skipped() -> Self {
        Self {
            authenticated: false,
            token_hash: None,
        }
    }
}

/// Hash a token using SHA-256
///
/// Tokens should be hashed before storage. This provides one-way
/// encryption so plaintext tokens are never stored.
///
/// # Example
///
/// ```rust
/// use engram_core::api::auth::hash_token;
///
/// let hash = hash_token("my-secret-token");
/// assert_eq!(hash.len(), 64); // SHA-256 hex output
/// ```
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Verify a token against a list of hashed tokens
///
/// Uses constant-time comparison to prevent timing attacks.
///
/// # Example
///
/// ```rust
/// use engram_core::api::auth::{hash_token, verify_token};
///
/// let hashed = vec![hash_token("valid-token")];
/// assert!(verify_token("valid-token", &hashed));
/// assert!(!verify_token("invalid-token", &hashed));
/// ```
pub fn verify_token(token: &str, hashed_tokens: &[String]) -> bool {
    let token_hash = hash_token(token);

    // Use constant-time comparison to prevent timing attacks
    hashed_tokens
        .iter()
        .any(|h| constant_time_eq(&token_hash, h))
}

/// Constant-time string comparison
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

/// Extract bearer token from Authorization header
///
/// Supports case-insensitive "Bearer" prefix.
///
/// # Example
///
/// ```rust
/// use engram_core::api::auth::extract_bearer_token;
///
/// assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
/// assert_eq!(extract_bearer_token("bearer ABC123"), Some("ABC123"));
/// assert_eq!(extract_bearer_token("Basic abc123"), None);
/// ```
pub fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    let parts: Vec<&str> = auth_header.splitn(2, ' ').collect();
    if parts.len() == 2 && parts[0].eq_ignore_ascii_case("bearer") {
        Some(parts[1].trim())
    } else {
        None
    }
}

/// Check if a path should skip authentication
///
/// Supports exact matches and wildcard suffix patterns (e.g., "/api/public/*").
///
/// # Example
///
/// ```rust
/// use engram_core::api::auth::should_skip_auth;
///
/// let skip_paths = vec!["/health".to_string(), "/api/public/*".to_string()];
///
/// assert!(should_skip_auth("/health", &skip_paths));
/// assert!(should_skip_auth("/api/public/anything", &skip_paths));
/// assert!(!should_skip_auth("/api/private", &skip_paths));
/// ```
pub fn should_skip_auth(path: &str, skip_paths: &[String]) -> bool {
    skip_paths.iter().any(|skip| {
        if skip.ends_with('*') {
            // Wildcard prefix match
            let prefix = &skip[..skip.len() - 1];
            path.starts_with(prefix)
        } else {
            // Exact match
            path == skip
        }
    })
}

/// Authenticate a request
///
/// Returns `Ok(AuthState)` on success, `Err(AuthError)` on failure.
///
/// # Example
///
/// ```rust
/// use engram_core::api::auth::{authenticate, hash_token, AuthConfig};
///
/// let config = AuthConfig::enabled(vec![hash_token("secret")]);
///
/// // Successful authentication
/// let result = authenticate(&config, "/api/data", Some("Bearer secret"));
/// assert!(result.is_ok());
/// assert!(result.unwrap().authenticated);
///
/// // Skip path
/// let result = authenticate(&config, "/health", None);
/// assert!(result.is_ok());
/// assert!(!result.unwrap().authenticated);
///
/// // Missing header
/// let result = authenticate(&config, "/api/data", None);
/// assert!(result.is_err());
/// ```
pub fn authenticate(
    config: &AuthConfig,
    path: &str,
    auth_header: Option<&str>,
) -> Result<AuthState, AuthError> {
    // Check if auth is disabled
    if !config.enabled {
        return Ok(AuthState::skipped());
    }

    // Check if path should skip auth
    if should_skip_auth(path, &config.skip_paths) {
        return Ok(AuthState::skipped());
    }

    // Extract auth header
    let auth_header = auth_header.ok_or_else(AuthError::missing_header)?;

    // Extract bearer token
    let token = extract_bearer_token(auth_header).ok_or_else(AuthError::invalid_format)?;

    // Verify token
    if verify_token(token, &config.tokens) {
        Ok(AuthState::authenticated(hash_token(token)))
    } else {
        Err(AuthError::invalid_token())
    }
}

// Middleware implementation requires axum which is not yet a dependency
// TODO(Task 007-04): Add middleware layer when axum is added
// The middleware will use the authenticate function above

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_token() {
        let hash1 = hash_token("test-token");
        let hash2 = hash_token("test-token");
        let hash3 = hash_token("different-token");

        // Same input produces same hash
        assert_eq!(hash1, hash2);
        // Different input produces different hash
        assert_ne!(hash1, hash3);
        // SHA-256 produces 64 character hex string
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_hash_token_empty() {
        let hash = hash_token("");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_verify_token_valid() {
        let tokens = vec![hash_token("valid-token")];
        assert!(verify_token("valid-token", &tokens));
    }

    #[test]
    fn test_verify_token_invalid() {
        let tokens = vec![hash_token("valid-token")];
        assert!(!verify_token("invalid-token", &tokens));
    }

    #[test]
    fn test_verify_token_multiple() {
        let tokens = vec![
            hash_token("token-1"),
            hash_token("token-2"),
            hash_token("token-3"),
        ];

        assert!(verify_token("token-1", &tokens));
        assert!(verify_token("token-2", &tokens));
        assert!(verify_token("token-3", &tokens));
        assert!(!verify_token("token-4", &tokens));
    }

    #[test]
    fn test_verify_token_empty_list() {
        let tokens: Vec<String> = vec![];
        assert!(!verify_token("any-token", &tokens));
    }

    #[test]
    fn test_extract_bearer_token_valid() {
        assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("Bearer   abc123  "), Some("abc123"));
    }

    #[test]
    fn test_extract_bearer_token_case_insensitive() {
        assert_eq!(extract_bearer_token("bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("BEARER abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("BeArEr abc123"), Some("abc123"));
    }

    #[test]
    fn test_extract_bearer_token_invalid() {
        assert_eq!(extract_bearer_token("Basic abc123"), None);
        assert_eq!(extract_bearer_token("abc123"), None);
        assert_eq!(extract_bearer_token("Bearer"), None);
        assert_eq!(extract_bearer_token(""), None);
    }

    #[test]
    fn test_should_skip_auth_exact() {
        let skip_paths = vec!["/health".to_string(), "/metrics".to_string()];

        assert!(should_skip_auth("/health", &skip_paths));
        assert!(should_skip_auth("/metrics", &skip_paths));
        assert!(!should_skip_auth("/healthz", &skip_paths));
        assert!(!should_skip_auth("/health/check", &skip_paths));
    }

    #[test]
    fn test_should_skip_auth_wildcard() {
        let skip_paths = vec!["/api/public/*".to_string()];

        assert!(should_skip_auth("/api/public/", &skip_paths));
        assert!(should_skip_auth("/api/public/test", &skip_paths));
        assert!(should_skip_auth("/api/public/nested/path", &skip_paths));
        assert!(!should_skip_auth("/api/public", &skip_paths));
        assert!(!should_skip_auth("/api/private", &skip_paths));
    }

    #[test]
    fn test_should_skip_auth_empty() {
        let skip_paths: Vec<String> = vec![];
        assert!(!should_skip_auth("/any/path", &skip_paths));
    }

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert!(!config.enabled);
        assert!(config.tokens.is_empty());
        assert!(config.skip_paths.contains(&"/health".to_string()));
    }

    #[test]
    fn test_auth_config_enabled() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        assert!(config.enabled);
        assert_eq!(config.tokens.len(), 1);
    }

    #[test]
    fn test_auth_config_with_token() {
        let config = AuthConfig::disabled().with_token("secret");
        assert_eq!(config.tokens.len(), 1);
        assert!(config.validate("secret"));
    }

    #[test]
    fn test_auth_config_should_skip() {
        let config = AuthConfig::enabled(vec![]);
        assert!(config.should_skip("/health"));
        assert!(!config.should_skip("/api/data"));

        let disabled = AuthConfig::disabled();
        assert!(disabled.should_skip("/any/path"));
    }

    #[test]
    fn test_auth_config_validate() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        assert!(config.validate("secret"));
        assert!(!config.validate("wrong"));

        let disabled = AuthConfig::disabled();
        assert!(disabled.validate("anything"));
    }

    #[test]
    fn test_auth_error_display() {
        let err = AuthError::unauthorized();
        assert!(err.to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_auth_state_authenticated() {
        let state = AuthState::authenticated("hash123".to_string());
        assert!(state.authenticated);
        assert_eq!(state.token_hash, Some("hash123".to_string()));
    }

    #[test]
    fn test_auth_state_unauthenticated() {
        let state = AuthState::unauthenticated();
        assert!(!state.authenticated);
        assert!(state.token_hash.is_none());
    }

    #[test]
    fn test_authenticate_disabled() {
        let config = AuthConfig::disabled();
        let result = authenticate(&config, "/api/data", None);
        assert!(result.is_ok());
        assert!(!result.unwrap().authenticated);
    }

    #[test]
    fn test_authenticate_skip_path() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        let result = authenticate(&config, "/health", None);
        assert!(result.is_ok());
        assert!(!result.unwrap().authenticated);
    }

    #[test]
    fn test_authenticate_missing_header() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        let result = authenticate(&config, "/api/data", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().error.contains("Missing"));
    }

    #[test]
    fn test_authenticate_invalid_format() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        let result = authenticate(&config, "/api/data", Some("Basic abc123"));
        assert!(result.is_err());
        assert!(result.unwrap_err().error.contains("format"));
    }

    #[test]
    fn test_authenticate_invalid_token() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        let result = authenticate(&config, "/api/data", Some("Bearer wrong"));
        assert!(result.is_err());
        assert!(result.unwrap_err().error.contains("Invalid token"));
    }

    #[test]
    fn test_authenticate_valid() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        let result = authenticate(&config, "/api/data", Some("Bearer secret"));
        assert!(result.is_ok());
        let state = result.unwrap();
        assert!(state.authenticated);
        assert!(state.token_hash.is_some());
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn test_auth_error_factories() {
        let err = AuthError::unauthorized();
        assert_eq!(err.code, "UNAUTHORIZED");

        let err = AuthError::missing_header();
        assert!(err.error.contains("Missing"));

        let err = AuthError::invalid_token();
        assert!(err.error.contains("Invalid"));

        let err = AuthError::invalid_format();
        assert!(err.error.contains("format"));
    }

    #[test]
    fn test_auth_config_with_skip_paths() {
        let config = AuthConfig::enabled(vec![]).with_skip_paths(vec!["/custom".to_string()]);

        assert!(config.should_skip("/custom"));
        assert!(config.should_skip("/health")); // Default skip path
    }

    #[test]
    fn test_auth_config_serialization() {
        let config = AuthConfig::enabled(vec![hash_token("secret")]);
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AuthConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.enabled);
        assert_eq!(parsed.tokens.len(), 1);
    }
}
