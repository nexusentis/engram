//! Runtime configuration for the LongMemEval benchmark.
//!
//! Loads from a TOML file (default `config/benchmark.toml`) with env var overrides.
//! Eliminates recompilation for model/threshold/gate changes.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::answerer::{AnswererConfig, GraphAugmentConfig};
use super::ingester::IngesterConfig;

// Re-export types that moved to engram-core
pub use engram::llm::{LlmClientConfig, ModelProfile, ModelRegistry};

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

/// Top-level benchmark configuration, loaded from TOML + env overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BenchmarkConfig {
    pub answerer: AnswererSection,
    pub ingester: IngesterSection,
    pub llm: LlmClientConfig,
    pub benchmark: BenchmarkSection,
    #[serde(default)]
    pub models: Vec<ModelProfile>,
    pub ensemble: Option<EnsembleConfig>,
}

// ---------------------------------------------------------------------------
// Section types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnswererSection {
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
    pub max_iterations: usize,
    pub tool_result_limit: usize,
    pub prefetch_explicit: usize,
    pub prefetch_deductive: usize,
    pub prefetch_messages: usize,
    pub use_strategy: bool,
    pub relative_dates: bool,
    pub agentic: bool,
    pub enable_consolidation: bool,
    pub consolidation_threshold: f32,
    pub enable_graph_retrieval: bool,
    pub enable_causal_links: bool,
    pub enable_cross_encoder_rerank: bool,
    pub cross_encoder_rerank_top_k: usize,
    pub gates: GateThresholds,
    pub graph_augment: GraphAugmentSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphAugmentSection {
    pub enabled: bool,
    pub seed_limit: usize,
    pub fact_limit: usize,
    pub neighbors_per_seed: usize,
    pub facts_per_entity: usize,
    pub facts_per_neighbor: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GateThresholds {
    pub preference_min_retrievals: usize,
    pub enumeration_min_retrievals: usize,
    pub update_min_retrievals: usize,
    pub abstention_min_retrievals: usize,
    pub anti_abstention_keyword_threshold: usize,
    pub preference_keyword_threshold: usize,
    pub loop_break_consecutive_dupes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IngesterSection {
    pub model: String,
    pub concurrency: usize,
    pub extraction_seed: Option<u64>,
    pub extraction_mode: String,
    pub enable_consolidation: bool,
    pub consolidation_threshold: f32,
    pub skip_messages: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BenchmarkSection {
    pub answer_concurrency: usize,
    pub qdrant_url: String,
}

// ---------------------------------------------------------------------------
// Ensemble config (P22 skeleton)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnsembleConfig {
    pub enabled: bool,
    pub primary_model: String,
    pub fallback_model: String,
    pub fallback_on_abstention: bool,
    pub fallback_on_loop_break: bool,
    /// P22b: skip fallback for Abstention-category questions (default: false)
    pub fallback_on_abs_questions: bool,
    /// P31: trigger fallback on high-iteration Enumeration questions
    pub fallback_on_enum_uncertainty: bool,
    /// P31: minimum iterations before Enumeration fallback fires
    pub enum_uncertainty_min_iterations: usize,
}

// ---------------------------------------------------------------------------
// Defaults (benchmark harness defaults, NOT library defaults)
// ---------------------------------------------------------------------------

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            answerer: AnswererSection::default(),
            ingester: IngesterSection::default(),
            llm: LlmClientConfig::default(),
            benchmark: BenchmarkSection::default(),
            models: vec![],
            ensemble: None,
        }
    }
}

impl Default for AnswererSection {
    fn default() -> Self {
        Self {
            model: "google/gemini-3.1-pro-preview".to_string(),
            temperature: 0.0,
            max_tokens: 500,
            max_iterations: 20,
            tool_result_limit: 12000,
            prefetch_explicit: 15,
            prefetch_deductive: 10,
            prefetch_messages: 20,
            use_strategy: true,
            relative_dates: true,
            agentic: true,
            enable_consolidation: false,
            consolidation_threshold: 0.92,
            enable_graph_retrieval: false,
            enable_causal_links: false,
            enable_cross_encoder_rerank: false,
            cross_encoder_rerank_top_k: 30,
            gates: GateThresholds::default(),
            graph_augment: GraphAugmentSection::default(),
        }
    }
}

impl Default for GraphAugmentSection {
    fn default() -> Self {
        Self {
            enabled: false,
            seed_limit: 5,
            fact_limit: 6,
            neighbors_per_seed: 5,
            facts_per_entity: 20,
            facts_per_neighbor: 10,
        }
    }
}

impl Default for GateThresholds {
    fn default() -> Self {
        Self {
            preference_min_retrievals: 3,
            enumeration_min_retrievals: 3,
            update_min_retrievals: 3,
            abstention_min_retrievals: 5,
            anti_abstention_keyword_threshold: 3,
            preference_keyword_threshold: 2,
            loop_break_consecutive_dupes: 3,
        }
    }
}

impl Default for IngesterSection {
    fn default() -> Self {
        Self {
            model: "gpt-4o-mini".to_string(),
            concurrency: 100, // benchmark default, NOT IngesterConfig::default() which is 10
            extraction_seed: Some(42),
            extraction_mode: "local-fast".to_string(),
            enable_consolidation: false,
            consolidation_threshold: 0.92,
            skip_messages: false,
        }
    }
}

impl Default for BenchmarkSection {
    fn default() -> Self {
        Self {
            answer_concurrency: 7,
            qdrant_url: "http://localhost:6334".to_string(),
        }
    }
}

impl Default for EnsembleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            primary_model: "gemini-3.1-pro-preview".to_string(),
            fallback_model: "gpt-4o".to_string(),
            fallback_on_abstention: true,
            fallback_on_loop_break: true,
            fallback_on_abs_questions: false,
            fallback_on_enum_uncertainty: false,
            enum_uncertainty_min_iterations: 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl BenchmarkConfig {
    /// Load config from TOML file (path from `BENCHMARK_CONFIG` env var or default).
    /// Applies env var overrides on top. Fails if no TOML file found or no [[models]] defined.
    pub fn load() -> Result<Self> {
        let path = std::env::var("BENCHMARK_CONFIG")
            .unwrap_or_else(|_| "config/benchmark.toml".to_string());

        // Try path as-is first, then relative to workspace root (for integration tests
        // where CWD may differ from repo root)
        let resolved_path = if Path::new(&path).exists() {
            path.clone()
        } else if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            // CARGO_MANIFEST_DIR is crates/engram, workspace root is ../..
            let workspace_path = format!("{}/../../{}", manifest_dir, path);
            if Path::new(&workspace_path).exists() {
                workspace_path
            } else {
                path.clone() // will fall through to "not found" error
            }
        } else {
            path.clone()
        };

        if !Path::new(&resolved_path).exists() {
            anyhow::bail!(
                "Config file not found: {}. Set BENCHMARK_CONFIG env var or create config/benchmark.toml.",
                path
            );
        }

        let contents = std::fs::read_to_string(&resolved_path)
            .with_context(|| format!("Failed to read config file: {}", resolved_path))?;
        let mut config: BenchmarkConfig = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse TOML config: {}", resolved_path))?;

        if config.models.is_empty() {
            anyhow::bail!(
                "No [[models]] entries in {}. Every model used by answerer/ingester/ensemble must have a [[models]] entry with base_url and api_key_env.",
                resolved_path
            );
        }

        eprintln!("[CONFIG] Loaded from {} ({} model profiles)", resolved_path, config.models.len());

        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    /// Apply environment variable overrides on top of TOML/default values.
    fn apply_env_overrides(&mut self) {
        // Answerer overrides
        if let Ok(v) = std::env::var("ANSWER_MODEL") {
            self.answerer.model = v;
        }
        if let Ok(v) = std::env::var("ANSWER_TEMP") {
            if let Ok(f) = v.parse() {
                self.answerer.temperature = f;
            }
        }
        if let Ok(v) = std::env::var("MAX_ITERATIONS") {
            if let Ok(n) = v.parse() {
                self.answerer.max_iterations = n;
            }
        }
        if let Ok(v) = std::env::var("TOOL_RESULT_LIMIT") {
            if let Ok(n) = v.parse() {
                self.answerer.tool_result_limit = n;
            }
        }
        if let Ok(v) = std::env::var("PREFETCH_EXPLICIT") {
            if let Ok(n) = v.parse() {
                self.answerer.prefetch_explicit = n;
            }
        }
        if let Ok(v) = std::env::var("PREFETCH_DEDUCTIVE") {
            if let Ok(n) = v.parse() {
                self.answerer.prefetch_deductive = n;
            }
        }
        if let Ok(v) = std::env::var("PREFETCH_MESSAGES") {
            if let Ok(n) = v.parse() {
                self.answerer.prefetch_messages = n;
            }
        }
        if let Ok(v) = std::env::var("USE_STRATEGY") {
            self.answerer.use_strategy = v != "0";
        }
        if let Ok(v) = std::env::var("RELATIVE_DATES") {
            self.answerer.relative_dates = v != "0";
        }
        if let Ok(v) = std::env::var("CONSOLIDATION") {
            self.answerer.enable_consolidation = v == "1";
        }
        if let Ok(v) = std::env::var("CONSOLIDATION_THRESHOLD") {
            if let Ok(f) = v.parse() {
                self.answerer.consolidation_threshold = f;
            }
        }
        if let Ok(v) = std::env::var("CROSS_ENCODER_RERANK") {
            self.answerer.enable_cross_encoder_rerank = v == "1";
        }
        if let Ok(v) = std::env::var("RERANK_TOP_K") {
            if let Ok(n) = v.parse() {
                self.answerer.cross_encoder_rerank_top_k = n;
            }
        }
        if let Ok(v) = std::env::var("GRAPH_RETRIEVAL") {
            self.answerer.enable_graph_retrieval = v == "1";
        }
        if let Ok(v) = std::env::var("GRAPH_AUGMENT") {
            self.answerer.graph_augment.enabled = v == "1";
        }
        if let Ok(v) = std::env::var("CAUSAL_LINKS") {
            self.answerer.enable_causal_links = v == "1";
        }
        if let Ok(v) = std::env::var("AGENTIC") {
            self.answerer.agentic = v == "1";
        }

        // Ingester overrides
        if let Ok(v) = std::env::var("INGESTION_MODEL") {
            self.ingester.model = v;
        }
        if let Ok(v) = std::env::var("INGESTION_CONCURRENCY") {
            if let Ok(n) = v.parse() {
                self.ingester.concurrency = n;
            }
        }

        // Benchmark overrides
        if let Ok(v) = std::env::var("ANSWER_CONCURRENCY") {
            if let Ok(n) = v.parse() {
                self.benchmark.answer_concurrency = n;
            }
        }

        // Ensemble overrides
        if let Ok(v) = std::env::var("ENSEMBLE_ENABLED") {
            let enabled = v == "1" || v == "true";
            if let Some(ref mut ens) = self.ensemble {
                ens.enabled = enabled;
            } else if enabled {
                self.ensemble = Some(EnsembleConfig {
                    enabled: true,
                    ..EnsembleConfig::default()
                });
            }
        }
        if let Ok(v) = std::env::var("ENSEMBLE_FALLBACK_MODEL") {
            if let Some(ref mut ens) = self.ensemble {
                ens.fallback_model = v;
            }
        }
    }

    /// Validate that referenced models have profiles.
    fn validate(&self) -> Result<()> {
        let registry = self.model_registry();
        registry
            .get(&self.answerer.model)
            .with_context(|| format!("answerer.model = '{}'", self.answerer.model))?;
        registry
            .get(&self.ingester.model)
            .with_context(|| format!("ingester.model = '{}'", self.ingester.model))?;
        if let Some(ref ens) = self.ensemble {
            if ens.enabled {
                // Codex fix #3: ensure answerer.model matches ensemble.primary_model
                if ens.primary_model != self.answerer.model {
                    anyhow::bail!(
                        "ensemble.primary_model ('{}') != answerer.model ('{}') — they must match",
                        ens.primary_model, self.answerer.model
                    );
                }
                registry
                    .get(&ens.primary_model)
                    .with_context(|| format!("ensemble.primary_model = '{}'", ens.primary_model))?;
                registry
                    .get(&ens.fallback_model)
                    .with_context(|| format!("ensemble.fallback_model = '{}'", ens.fallback_model))?;
            }
        }
        Ok(())
    }

    /// Build a ModelRegistry from the config's model profiles.
    pub fn model_registry(&self) -> ModelRegistry {
        ModelRegistry::from_config(self.models.clone())
    }

    /// Convert answerer section to an AnswererConfig.
    pub fn to_answerer_config(&self) -> AnswererConfig {
        let a = &self.answerer;
        AnswererConfig {
            answer_model: a.model.clone(),
            temperature: a.temperature,
            max_tokens: a.max_tokens,
            max_iterations: Some(a.max_iterations),
            tool_result_limit: a.tool_result_limit,
            prefetch_explicit: a.prefetch_explicit,
            prefetch_deductive: a.prefetch_deductive,
            prefetch_messages: a.prefetch_messages,
            use_strategy: a.use_strategy,
            relative_dates: a.relative_dates,
            agentic: a.agentic,
            enable_consolidation: a.enable_consolidation,
            consolidation_threshold: a.consolidation_threshold,
            enable_graph_retrieval: a.enable_graph_retrieval,
            enable_causal_links: a.enable_causal_links,
            enable_cross_encoder_rerank: a.enable_cross_encoder_rerank,
            cross_encoder_rerank_top_k: a.cross_encoder_rerank_top_k,
            graph_augment: GraphAugmentConfig {
                enabled: a.graph_augment.enabled,
                seed_limit: a.graph_augment.seed_limit,
                fact_limit: a.graph_augment.fact_limit,
                neighbors_per_seed: a.graph_augment.neighbors_per_seed,
                facts_per_entity: a.graph_augment.facts_per_entity,
                facts_per_neighbor: a.graph_augment.facts_per_neighbor,
            },
            gates: a.gates.clone(),
            ..AnswererConfig::default()
        }
    }

    /// Convert ingester section to an IngesterConfig.
    pub fn to_ingester_config(&self) -> IngesterConfig {
        let i = &self.ingester;
        let mut config = IngesterConfig::default()
            .with_concurrency(i.concurrency)
            .with_model(&i.model)
            .with_consolidation(i.enable_consolidation)
            .with_consolidation_threshold(i.consolidation_threshold);
        if let Some(seed) = i.extraction_seed {
            config = config.with_extraction_seed(seed);
        }
        if i.skip_messages {
            config = config.with_skip_messages(true);
        }
        config
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: builds a config with a few inline model profiles.
    /// These are test fixtures, not production data — production data lives in TOML files.
    fn test_config() -> BenchmarkConfig {
        let mut cfg = BenchmarkConfig::default();
        cfg.models = vec![
            ModelProfile {
                name: "gpt-4o".to_string(),
                base_url: Some("https://api.openai.com/v1/chat/completions".to_string()),
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                max_tokens_field: "max_tokens".to_string(),
                supports_temperature: true,
                token_cmd_env: None,
                prompt_price_per_m: 2.50,
                completion_price_per_m: 10.00,
            },
            ModelProfile {
                name: "gpt-4o-mini".to_string(),
                base_url: Some("https://api.openai.com/v1/chat/completions".to_string()),
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                max_tokens_field: "max_tokens".to_string(),
                supports_temperature: true,
                token_cmd_env: None,
                prompt_price_per_m: 0.15,
                completion_price_per_m: 0.60,
            },
            ModelProfile {
                name: "gpt-5".to_string(),
                base_url: Some("https://api.openai.com/v1/chat/completions".to_string()),
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                max_tokens_field: "max_completion_tokens".to_string(),
                supports_temperature: false,
                token_cmd_env: None,
                prompt_price_per_m: 1.25,
                completion_price_per_m: 10.00,
            },
            ModelProfile {
                name: "google/gemini-3.1-pro-preview".to_string(),
                base_url: Some("https://aiplatform.googleapis.com/v1/projects/test/locations/global/endpoints/openapi/chat/completions".to_string()),
                api_key_env: None,
                token_cmd_env: Some("GEMINI_TOKEN_CMD".to_string()),
                max_tokens_field: "max_tokens".to_string(),
                supports_temperature: true,
                prompt_price_per_m: 2.00,
                completion_price_per_m: 12.00,
            },
            ModelProfile {
                name: "google/gemini-2.5".to_string(),
                base_url: Some("https://aiplatform.googleapis.com/v1/projects/test/locations/global/endpoints/openapi/chat/completions".to_string()),
                api_key_env: None,
                token_cmd_env: Some("GEMINI_TOKEN_CMD".to_string()),
                max_tokens_field: "max_tokens".to_string(),
                supports_temperature: true,
                prompt_price_per_m: 1.25,
                completion_price_per_m: 10.00,
            },
        ];
        cfg
    }

    #[test]
    fn test_default_section_values() {
        let cfg = BenchmarkConfig::default();

        // Answerer defaults match benchmark harness
        assert_eq!(cfg.answerer.model, "google/gemini-3.1-pro-preview");
        assert_eq!(cfg.answerer.temperature, 0.0);
        assert_eq!(cfg.answerer.max_tokens, 500);
        assert_eq!(cfg.answerer.max_iterations, 20);
        assert_eq!(cfg.answerer.tool_result_limit, 12000);
        assert_eq!(cfg.answerer.prefetch_explicit, 15);
        assert_eq!(cfg.answerer.prefetch_deductive, 10);
        assert_eq!(cfg.answerer.prefetch_messages, 20);
        assert!(cfg.answerer.use_strategy);
        assert!(cfg.answerer.relative_dates);
        assert!(cfg.answerer.agentic);
        assert!(!cfg.answerer.enable_consolidation);
        assert_eq!(cfg.answerer.consolidation_threshold, 0.92);
        assert!(!cfg.answerer.enable_graph_retrieval);
        assert!(!cfg.answerer.enable_causal_links);

        // Gate defaults
        assert_eq!(cfg.answerer.gates.preference_min_retrievals, 3);
        assert_eq!(cfg.answerer.gates.enumeration_min_retrievals, 3);
        assert_eq!(cfg.answerer.gates.update_min_retrievals, 3);
        assert_eq!(cfg.answerer.gates.abstention_min_retrievals, 5);
        assert_eq!(cfg.answerer.gates.anti_abstention_keyword_threshold, 3);
        assert_eq!(cfg.answerer.gates.preference_keyword_threshold, 2);
        assert_eq!(cfg.answerer.gates.loop_break_consecutive_dupes, 3);

        // Graph augment defaults
        assert!(!cfg.answerer.graph_augment.enabled);
        assert_eq!(cfg.answerer.graph_augment.seed_limit, 5);
        assert_eq!(cfg.answerer.graph_augment.fact_limit, 6);

        // Ingester defaults (benchmark, NOT library)
        assert_eq!(cfg.ingester.model, "gpt-4o-mini");
        assert_eq!(cfg.ingester.concurrency, 100); // NOT 10
        assert_eq!(cfg.ingester.extraction_seed, Some(42));
        assert_eq!(cfg.ingester.extraction_mode, "local-fast");
        assert!(!cfg.ingester.enable_consolidation);
        assert_eq!(cfg.ingester.consolidation_threshold, 0.92);

        // LLM client defaults
        assert_eq!(cfg.llm.max_retries, 12);
        assert_eq!(cfg.llm.backoff_cap_secs, 60);
        assert_eq!(cfg.llm.token_refresh_mins, 45);
        assert_eq!(cfg.llm.request_timeout_secs, 90);

        // Benchmark defaults
        assert_eq!(cfg.benchmark.answer_concurrency, 7);
        assert_eq!(cfg.benchmark.qdrant_url, "http://localhost:6334");

        // No hardcoded models — all models come from TOML
        assert!(cfg.models.is_empty());
        assert!(cfg.ensemble.is_none());
    }

    #[test]
    fn test_model_registry_exact_match() {
        let cfg = test_config();
        let registry = cfg.model_registry();

        let profile = registry.get("gpt-4o").unwrap();
        assert_eq!(profile.prompt_price_per_m, 2.50);
        assert!(profile.supports_temperature);
        assert_eq!(profile.max_tokens_field, "max_tokens");
    }

    #[test]
    fn test_model_registry_prefix_match() {
        let cfg = test_config();
        let registry = cfg.model_registry();

        // "google/gemini-3.1-pro-preview" should match exactly
        let profile = registry.get("google/gemini-3.1-pro-preview").unwrap();
        assert_eq!(profile.prompt_price_per_m, 2.00);

        // "google/gemini-2.5-flash" should match "google/gemini-2.5" prefix
        let profile = registry.get("google/gemini-2.5-flash").unwrap();
        assert_eq!(profile.prompt_price_per_m, 1.25);
    }

    #[test]
    fn test_model_registry_fail_fast_unknown() {
        let cfg = test_config();
        let registry = cfg.model_registry();

        let result = registry.get("typo-model");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No ModelProfile for 'typo-model'"));
    }

    #[test]
    fn test_model_registry_estimate_cost() {
        let cfg = test_config();
        let registry = cfg.model_registry();

        let cost = registry.estimate_cost("gpt-4o", 1_000_000, 1_000_000).unwrap();
        assert!((cost - 12.50).abs() < 0.01); // 2.50 + 10.00
    }

    #[test]
    fn test_model_registry_gpt5_no_temperature() {
        let cfg = test_config();
        let registry = cfg.model_registry();

        let profile = registry.get("gpt-5").unwrap();
        assert!(!profile.supports_temperature);
        assert_eq!(profile.max_tokens_field, "max_completion_tokens");
    }

    #[test]
    fn test_to_answerer_config() {
        let cfg = BenchmarkConfig::default();
        let ac = cfg.to_answerer_config();

        assert_eq!(ac.answer_model, "google/gemini-3.1-pro-preview");
        assert_eq!(ac.temperature, 0.0);
        assert_eq!(ac.max_tokens, 500);
        assert_eq!(ac.max_iterations, Some(20));
        assert!(ac.agentic);
        assert!(ac.use_strategy);
        assert!(ac.relative_dates);
        assert_eq!(ac.tool_result_limit, 12000);
    }

    #[test]
    fn test_to_ingester_config() {
        let cfg = BenchmarkConfig::default();
        let ic = cfg.to_ingester_config();

        assert_eq!(ic.model, "gpt-4o-mini");
        assert_eq!(ic.concurrency, 100);
        assert_eq!(ic.extraction_seed, Some(42));
    }

    #[test]
    fn test_toml_roundtrip() {
        let cfg = BenchmarkConfig::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: BenchmarkConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.answerer.model, cfg.answerer.model);
        assert_eq!(parsed.ingester.concurrency, cfg.ingester.concurrency);
        assert_eq!(parsed.llm.max_retries, cfg.llm.max_retries);
    }

    #[test]
    fn test_empty_toml_uses_section_defaults() {
        let parsed: BenchmarkConfig = toml::from_str("").unwrap();
        assert_eq!(parsed.answerer.model, "google/gemini-3.1-pro-preview");
        assert_eq!(parsed.ingester.concurrency, 100);
        // No [[models]] entries — load() would reject this
        assert!(parsed.models.is_empty());
    }

    #[test]
    fn test_partial_toml_override() {
        let toml_str = r#"
[answerer]
model = "gpt-4o"
max_iterations = 30

[ingester]
concurrency = 50
"#;
        let parsed: BenchmarkConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.answerer.model, "gpt-4o");
        assert_eq!(parsed.answerer.max_iterations, 30);
        // Other fields keep defaults
        assert_eq!(parsed.answerer.temperature, 0.0);
        assert_eq!(parsed.answerer.tool_result_limit, 12000);
        assert_eq!(parsed.ingester.concurrency, 50);
        assert_eq!(parsed.ingester.model, "gpt-4o-mini");
    }

    #[test]
    fn test_ensemble_primary_model_mismatch_fails_validation() {
        let mut cfg = test_config();
        cfg.ensemble = Some(EnsembleConfig {
            enabled: true,
            primary_model: "gpt-4o".to_string(), // mismatch with answerer.model
            fallback_model: "gpt-4o-mini".to_string(),
            ..EnsembleConfig::default()
        });
        // answerer.model defaults to "google/gemini-3.1-pro-preview", primary says "gpt-4o"
        let result = cfg.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("ensemble.primary_model"));
        assert!(err_msg.contains("answerer.model"));
    }

    #[test]
    fn test_ensemble_disabled_skips_validation() {
        let mut cfg = test_config();
        cfg.ensemble = Some(EnsembleConfig {
            enabled: false,
            primary_model: "nonexistent-model".to_string(),
            ..EnsembleConfig::default()
        });
        // Disabled ensemble should not trigger validation
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_ensemble_config_defaults() {
        let ec = EnsembleConfig::default();
        assert!(!ec.enabled);
        assert!(ec.fallback_on_abstention);
        assert!(ec.fallback_on_loop_break);
        assert!(!ec.fallback_on_abs_questions);
    }
}
