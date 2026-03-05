//! Configuration for answer generation.

use engram::retrieval::AbstentionConfig;

/// Configuration for answer generation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AnswererConfig {
    /// Model to use for answer generation
    pub answer_model: String,
    /// Maximum tokens in response
    pub max_tokens: usize,
    /// Temperature for generation
    pub temperature: f32,
    /// Number of memories to retrieve
    pub top_k: usize,
    /// Whether to use LLM for answer generation (vs just returning retrieved text)
    pub use_llm: bool,
    /// Whether to enable abstention detection
    pub enable_abstention: bool,
    /// Abstention configuration
    pub abstention_config: AbstentionConfig,
    /// Whether to use agentic answering (tool-calling loop) vs single-pass
    pub agentic: bool,
    /// Max iterations for agentic loop (default 10)
    pub max_iterations: Option<usize>,
    /// Whether to enable LLM-based reranking (gpt-4o-mini)
    pub enable_llm_reranking: bool,
    /// Lambda for MMR diversity filtering (0.0 = max diversity, 1.0 = no diversity)
    pub mmr_lambda: f32,
    /// Enable session-level NDCG retrieval (aggregate message scores by session, fetch full sessions)
    pub session_ndcg: bool,
    /// Number of top sessions to expand in NDCG retrieval (default: 5)
    pub ndcg_top_sessions: usize,
    /// Number of initial message candidates for NDCG scoring (default: 100)
    pub ndcg_message_candidates: usize,
    /// Max total messages to include from session expansion (default: 60)
    pub ndcg_max_messages: usize,
    /// Enable Chain-of-Note per-session extraction before answering
    pub enable_chain_of_note: bool,
    /// Enable dedicated temporal RRF channel (4th parallel search with date filter)
    pub enable_temporal_rrf: bool,
    /// Enable entity-linked secondary retrieval
    pub enable_entity_linked: bool,
    /// Max chars per tool result in agentic loop (default: 12000)
    pub tool_result_limit: usize,
    /// Activate QuestionStrategy guidance in agentic prompt
    pub use_strategy: bool,
    /// Number of explicit-level facts in prefetch
    pub prefetch_explicit: usize,
    /// Number of deductive-level facts in prefetch
    pub prefetch_deductive: usize,
    /// Number of messages in prefetch
    pub prefetch_messages: usize,
    /// Add relative dates "(X days ago)" to date headers
    pub relative_dates: bool,
    /// Enable knowledge update consolidation (is_latest filter)
    pub enable_consolidation: bool,
    /// Embedding similarity threshold for contradiction detection
    pub consolidation_threshold: f32,
    /// Enable cross-encoder reranking with LLM
    pub enable_cross_encoder_rerank: bool,
    /// Keep top-K after cross-encoder reranking
    pub cross_encoder_rerank_top_k: usize,
    /// Enable graph-based retrieval (requires entity graph)
    pub enable_graph_retrieval: bool,
    /// Enable causal link retrieval boost
    pub enable_causal_links: bool,
    /// P20: Behind-the-scenes graph augmentation config
    pub graph_augment: GraphAugmentConfig,
    /// Gate thresholds for agentic loop
    #[serde(default)]
    pub gates: super::super::benchmark_config::GateThresholds,
}

/// P20: Configuration for behind-the-scenes graph augmentation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GraphAugmentConfig {
    /// Enable graph augmentation (default: false)
    pub enabled: bool,
    /// Max seed entities extracted from prefetch fact_ids (default: 5)
    pub seed_limit: usize,
    /// Max additional facts to inject (default: 6)
    pub fact_limit: usize,
    /// Max 1-hop neighbors per seed entity (default: 5)
    pub neighbors_per_seed: usize,
    /// Max fact_ids per seed entity (default: 20)
    pub facts_per_entity: usize,
    /// Max fact_ids per neighbor entity (default: 10)
    pub facts_per_neighbor: usize,
}

impl Default for GraphAugmentConfig {
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

impl Default for AnswererConfig {
    fn default() -> Self {
        Self {
            answer_model: "gpt-4o".to_string(),
            max_tokens: 500,
            temperature: 0.0,
            top_k: 20,
            use_llm: true,
            enable_abstention: false, // use prompt-based abstention instead of threshold-based
            abstention_config: AbstentionConfig::default(),
            agentic: false,
            max_iterations: None,
            enable_llm_reranking: false,
            mmr_lambda: 1.0,     // 1.0 = disabled (no diversity filtering)
            session_ndcg: false, // Disabled: hurts MultiSession on clean data
            ndcg_top_sessions: 5,
            ndcg_message_candidates: 100,
            ndcg_max_messages: 60,
            enable_chain_of_note: false, // Disabled: no measurable benefit
            enable_temporal_rrf: false,  // Disabled: no measurable benefit
            enable_entity_linked: false, // Disabled: adds noise
            tool_result_limit: 12000,
            use_strategy: true,
            prefetch_explicit: 15,
            prefetch_deductive: 10,
            prefetch_messages: 20,
            relative_dates: true,
            enable_consolidation: false,
            consolidation_threshold: 0.92,
            enable_cross_encoder_rerank: false,
            cross_encoder_rerank_top_k: 30,
            enable_graph_retrieval: false,
            enable_causal_links: false,
            graph_augment: GraphAugmentConfig::default(),
            gates: super::super::benchmark_config::GateThresholds::default(),
        }
    }
}

impl AnswererConfig {
    /// Create a new config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set answer model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.answer_model = model.into();
        self
    }

    /// Set max tokens
    pub fn with_max_tokens(mut self, max: usize) -> Self {
        self.max_tokens = max;
        self
    }

    /// Set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = temp;
        self
    }

    /// Set top_k
    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    /// Set whether to use LLM
    pub fn with_use_llm(mut self, use_llm: bool) -> Self {
        self.use_llm = use_llm;
        self
    }

    /// Enable or disable abstention detection
    pub fn with_abstention(mut self, enable: bool) -> Self {
        self.enable_abstention = enable;
        self
    }

    /// Set the abstention configuration
    pub fn with_abstention_config(mut self, config: AbstentionConfig) -> Self {
        self.abstention_config = config;
        self.enable_abstention = true;
        self
    }

    /// Enable or disable agentic answering
    pub fn with_agentic(mut self, agentic: bool) -> Self {
        self.agentic = agentic;
        self
    }

    /// Set max iterations for agentic loop
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = Some(max);
        self
    }

    /// Enable or disable LLM-based reranking
    pub fn with_llm_reranking(mut self, enable: bool) -> Self {
        self.enable_llm_reranking = enable;
        self
    }

    /// Set MMR diversity lambda (0.0 = max diversity, 1.0 = no diversity)
    pub fn with_mmr_lambda(mut self, lambda: f32) -> Self {
        self.mmr_lambda = lambda;
        self
    }

    /// Enable or disable session-level NDCG retrieval
    pub fn with_session_ndcg(mut self, enable: bool) -> Self {
        self.session_ndcg = enable;
        self
    }

    /// Set number of top sessions for NDCG expansion
    pub fn with_ndcg_top_sessions(mut self, n: usize) -> Self {
        self.ndcg_top_sessions = n;
        self
    }
}
