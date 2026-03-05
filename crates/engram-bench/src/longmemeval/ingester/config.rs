//! Ingester configuration types.

use std::path::PathBuf;

/// Ingestion mode for sessions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionMode {
    /// Real-time API calls (existing mode)
    RealTime,
    /// Generate JSONL batch file only (batch mode step 1)
    BatchGenerate,
    /// Submit generated JSONL to OpenAI (batch mode step 2)
    BatchSubmit,
    /// Poll for batch completion and process results (batch mode step 3)
    BatchPoll,
}

/// Result from batch poll operation
#[derive(Debug, Clone)]
pub enum BatchPollResult {
    /// Batch is still in progress
    InProgress {
        completed: usize,
        failed: usize,
        total: usize,
    },
    /// Batch completed, results have been processed
    Completed {
        sessions_processed: usize,
        facts_extracted: usize,
        errors: Vec<String>,
    },
    /// Batch failed
    Failed { error: String },
}

/// Configuration for session ingestion
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct IngesterConfig {
    /// Maximum concurrent user workers during ingestion
    pub concurrency: usize,
    /// Extraction mode to use
    pub extraction_mode: String,
    /// Whether to clear existing data before ingestion
    pub clear_before_ingest: bool,
    /// Progress callback interval (number of sessions)
    pub progress_interval: usize,
    /// Use single-pass extraction (1 LLM call) instead of two-pass (2 LLM calls)
    /// Faster but less accurate entity resolution
    pub single_pass: bool,
    /// Model to use for extraction (default: gpt-4o-mini)
    pub model: String,
    /// Temperature for extraction (None = use model default)
    pub extraction_temperature: Option<f32>,
    /// Seed for deterministic extraction (None = omit)
    pub extraction_seed: Option<u64>,
    /// Enable knowledge update consolidation (detect contradictions, mark old facts)
    pub enable_consolidation: bool,
    /// Embedding similarity threshold for contradiction detection
    pub consolidation_threshold: f32,
    /// Enable causal link extraction during ingestion
    pub enable_causal_links: bool,
    /// Directory for caching LLM extraction responses (deterministic ingestion).
    /// When set, identical sessions produce identical extracted facts across runs.
    #[serde(skip)]
    pub extraction_cache_dir: Option<PathBuf>,
    /// Skip raw message storage (useful for additive ingestion where messages already exist)
    pub skip_messages: bool,
}

/// Maximum concurrency for parallel ingestion
/// Set to 10 to stay safely under OpenAI's 200K TPM limit for Tier 1
/// (each session uses ~4K tokens × 2 passes × 10 = ~80K TPM with headroom)
pub const MAX_CONCURRENCY: usize = 10;

impl Default for IngesterConfig {
    fn default() -> Self {
        Self {
            concurrency: MAX_CONCURRENCY,
            extraction_mode: "local-fast".to_string(),
            clear_before_ingest: true,
            progress_interval: 10,
            single_pass: false,
            model: "gpt-4o-mini".to_string(),
            extraction_temperature: None,
            extraction_seed: None,
            enable_consolidation: false,
            consolidation_threshold: 0.92,
            enable_causal_links: false,
            extraction_cache_dir: None,
            skip_messages: false,
        }
    }
}

impl IngesterConfig {
    /// Create a new config with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set concurrency level
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Set extraction mode
    pub fn with_extraction_mode(mut self, mode: impl Into<String>) -> Self {
        self.extraction_mode = mode.into();
        self
    }

    /// Set whether to clear before ingest
    pub fn with_clear_before_ingest(mut self, clear: bool) -> Self {
        self.clear_before_ingest = clear;
        self
    }

    /// Set progress interval
    pub fn with_progress_interval(mut self, interval: usize) -> Self {
        self.progress_interval = interval;
        self
    }

    /// Use single-pass extraction (faster, 1 LLM call instead of 2)
    pub fn with_single_pass(mut self, single_pass: bool) -> Self {
        self.single_pass = single_pass;
        self
    }

    /// Set the model for extraction
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set extraction temperature
    pub fn with_extraction_temperature(mut self, temp: f32) -> Self {
        self.extraction_temperature = Some(temp);
        self
    }

    /// Set extraction seed for determinism
    pub fn with_extraction_seed(mut self, seed: u64) -> Self {
        self.extraction_seed = Some(seed);
        self
    }

    /// Enable knowledge update consolidation
    pub fn with_consolidation(mut self, enable: bool) -> Self {
        self.enable_consolidation = enable;
        self
    }

    /// Set consolidation similarity threshold
    pub fn with_consolidation_threshold(mut self, threshold: f32) -> Self {
        self.consolidation_threshold = threshold;
        self
    }

    /// Enable causal link extraction
    pub fn with_causal_links(mut self, enable: bool) -> Self {
        self.enable_causal_links = enable;
        self
    }

    /// Set extraction cache directory for deterministic ingestion
    pub fn with_extraction_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.extraction_cache_dir = Some(dir.into());
        self
    }

    /// Skip raw message storage (additive ingestion where messages already exist)
    pub fn with_skip_messages(mut self, skip: bool) -> Self {
        self.skip_messages = skip;
        self
    }
}
