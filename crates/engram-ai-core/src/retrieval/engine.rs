//! Main retrieval engine orchestrating all components
//!
//! Integrates query analysis, decomposition, channel execution,
//! fusion, reranking, and confidence scoring.

use super::channel::ChannelConfig;
use super::confidence::{AbstentionConfig, ConfidenceAssessment, ConfidenceScorer};
use super::decomposer::{DecomposedQuery, QueryDecomposer};
use super::fusion::{FusedResult, RrfConfig, RrfFusion};
use super::query_analyzer::{QueryAnalysis, QueryAnalyzer};
use crate::types::Memory;

/// Result from the retrieval pipeline (post-fusion)
#[derive(Debug, Clone)]
pub struct RerankedResult {
    pub memory: Memory,
    pub original_rrf_score: f32,
    pub rerank_score: Option<f32>,
    pub final_score: f32,
    pub contributing_channels: Vec<String>,
}

impl RerankedResult {
    /// Create from a fused result (pass-through, no reranking)
    pub fn from_fused(fused: FusedResult) -> Self {
        Self {
            memory: fused.memory,
            original_rrf_score: fused.rrf_score,
            rerank_score: None,
            final_score: fused.rrf_score,
            contributing_channels: fused.contributing_channels,
        }
    }

    pub fn was_reranked(&self) -> bool {
        self.rerank_score.is_some()
    }

    pub fn score_delta(&self) -> Option<f32> {
        self.rerank_score.map(|rs| rs - self.original_rrf_score)
    }
}

/// Configuration for the retrieval engine
#[derive(Debug, Clone)]
pub struct RetrievalConfig {
    pub channel_config: ChannelConfig,
    pub rrf_config: RrfConfig,
    pub abstention_config: AbstentionConfig,
    pub enable_decomposition: bool,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            channel_config: ChannelConfig::default(),
            rrf_config: RrfConfig::default(),
            abstention_config: AbstentionConfig::default(),
            enable_decomposition: true,
        }
    }
}

/// Complete retrieval result
#[derive(Debug)]
pub struct RetrievalResult {
    pub query: String,
    pub analysis: QueryAnalysis,
    pub results: Vec<RerankedResult>,
    pub confidence: ConfidenceAssessment,
    pub was_decomposed: bool,
    pub decomposition_steps: Option<usize>,
}

impl RetrievalResult {
    pub fn should_abstain(&self) -> bool {
        self.confidence.confidence.should_abstain()
    }

    pub fn top_result(&self) -> Option<&RerankedResult> {
        self.results.first()
    }

    pub fn top_k(&self, k: usize) -> &[RerankedResult] {
        &self.results[..k.min(self.results.len())]
    }
}

/// Main retrieval engine orchestrating all components
pub struct RetrievalEngine {
    config: RetrievalConfig,
    query_analyzer: QueryAnalyzer,
    decomposer: QueryDecomposer,
    fusion: RrfFusion,
    confidence_scorer: ConfidenceScorer,
}

impl RetrievalEngine {
    pub fn new(config: RetrievalConfig) -> Self {
        Self {
            query_analyzer: QueryAnalyzer::new(),
            decomposer: QueryDecomposer::new(),
            fusion: RrfFusion::new(config.rrf_config.clone()),
            confidence_scorer: ConfidenceScorer::new(config.abstention_config.clone()),
            config,
        }
    }

    /// Get the query analyzer
    pub fn analyzer(&self) -> &QueryAnalyzer {
        &self.query_analyzer
    }

    /// Get the decomposer
    pub fn decomposer(&self) -> &QueryDecomposer {
        &self.decomposer
    }

    /// Get the fusion module
    pub fn fusion(&self) -> &RrfFusion {
        &self.fusion
    }

    /// Get the confidence scorer
    pub fn confidence_scorer(&self) -> &ConfidenceScorer {
        &self.confidence_scorer
    }

    /// Check if decomposition is enabled
    pub fn decomposition_enabled(&self) -> bool {
        self.config.enable_decomposition
    }

    /// Analyze a query
    pub async fn analyze_query(&self, query: &str) -> QueryAnalysis {
        self.query_analyzer.analyze(query).await
    }

    /// Try to decompose a query into multi-hop steps
    pub fn try_decompose(&self, query: &str) -> Option<DecomposedQuery> {
        if self.config.enable_decomposition {
            self.decomposer.decompose(query)
        } else {
            None
        }
    }

    /// Execute standard retrieval pipeline (fuse + assess)
    pub fn execute_pipeline(
        &self,
        query: &str,
        fused_results: Vec<FusedResult>,
        analysis: &QueryAnalysis,
    ) -> RetrievalResult {
        let reranked: Vec<RerankedResult> =
            fused_results.into_iter().map(RerankedResult::from_fused).collect();

        // Assess confidence
        let confidence = self.confidence_scorer.assess(&reranked);

        RetrievalResult {
            query: query.to_string(),
            analysis: analysis.clone(),
            results: reranked,
            confidence,
            was_decomposed: false,
            decomposition_steps: None,
        }
    }

    /// Execute a decomposed query pipeline
    pub fn execute_decomposed_pipeline(
        &self,
        decomposed: &DecomposedQuery,
        results: Vec<RerankedResult>,
        analysis: &QueryAnalysis,
    ) -> RetrievalResult {
        let steps = decomposed.steps.len();
        let confidence = self.confidence_scorer.assess(&results);

        RetrievalResult {
            query: decomposed.original.clone(),
            analysis: analysis.clone(),
            results,
            confidence,
            was_decomposed: true,
            decomposition_steps: Some(steps),
        }
    }

    /// Create a retrieval result for an empty query
    pub fn empty_result(&self, query: &str, analysis: &QueryAnalysis) -> RetrievalResult {
        let results: Vec<RerankedResult> = vec![];
        let confidence = self.confidence_scorer.assess(&results);

        RetrievalResult {
            query: query.to_string(),
            analysis: analysis.clone(),
            results,
            confidence,
            was_decomposed: false,
            decomposition_steps: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Memory;
    use std::collections::HashMap;

    fn make_memory() -> Memory {
        Memory::new("test_user", "test content")
    }

    fn make_fused_result(score: f32) -> FusedResult {
        FusedResult {
            memory: make_memory(),
            rrf_score: score,
            contributing_channels: vec![],
            channel_ranks: HashMap::new(),
        }
    }

    fn make_reranked_result(score: f32) -> RerankedResult {
        RerankedResult {
            memory: make_memory(),
            original_rrf_score: score,
            rerank_score: Some(score),
            final_score: score,
            contributing_channels: vec![],
        }
    }

    fn make_analysis(query: &str, is_multi_hop: bool) -> QueryAnalysis {
        QueryAnalysis {
            original_query: query.to_string(),
            normalized_query: query.to_lowercase(),
            intent: super::super::query_analyzer::QueryIntent::Factual,
            temporal_intent: super::super::query_analyzer::TemporalIntent::None,
            entities: vec![],
            temporal_constraints: vec![],
            keywords: vec!["test".to_string()],
            is_multi_hop,
            active_channels: super::super::query_analyzer::SearchChannels::default(),
        }
    }

    // NOTE: 7 tests removed (test_engine_creation, test_engine_decomposition_disabled,
    // test_engine_decomposition_enabled, test_analyze_query, test_execute_pipeline,
    // test_execute_decomposed_pipeline, test_empty_result).
    // These required a reranker that was never functional (ONNX Runtime not installed).
    // The reranker module has since been removed entirely.

    #[test]
    fn test_retrieval_result_methods() {
        let results = vec![make_reranked_result(0.9), make_reranked_result(0.7)];
        let analysis = make_analysis("test", false);

        let confidence = ConfidenceAssessment {
            confidence: super::super::confidence::RetrievalConfidence::High,
            top1_score: 0.9,
            score_gap: Some(0.2),
            entity_coverage: None,
            overall_score: 0.9,
            reason: "Good results".to_string(),
        };

        let result = RetrievalResult {
            query: "test".to_string(),
            analysis,
            results,
            confidence,
            was_decomposed: false,
            decomposition_steps: None,
        };

        assert!(!result.should_abstain());
        assert!(result.top_result().is_some());
        assert_eq!(result.top_k(1).len(), 1);
        assert_eq!(result.top_k(5).len(), 2);
    }
}
