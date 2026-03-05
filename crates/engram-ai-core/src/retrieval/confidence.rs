//! Confidence scoring and abstention detection
//!
//! Implements abstention gate to prevent hallucination when the memory system
//! cannot confidently answer a query.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;

use super::engine::RerankedResult;

/// Configuration for abstention detection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AbstentionConfig {
    /// Minimum top-1 score to not abstain
    pub min_top1_score: f32,
    /// Minimum gap between top-1 and top-2 scores
    pub min_score_gap: f32,
    /// Minimum number of results required
    pub min_results: usize,
    /// Minimum entity coverage (0-1)
    pub min_entity_coverage: f32,
    /// Weight for entity coverage in overall confidence
    pub entity_coverage_weight: f32,
    /// Whether to enable abstention
    pub enabled: bool,
    /// Minimum content similarity to consider results as same answer cluster (0-1)
    /// When top results have similarity above this threshold, they reinforce each other
    pub cluster_similarity_threshold: f32,
    /// Number of top results to check for answer clustering
    pub cluster_check_depth: usize,
}

impl Default for AbstentionConfig {
    fn default() -> Self {
        Self {
            // Calibrated on validation set (50 questions, seed=42):
            // - Abstention questions: 0 results (no relevant data in DB)
            // - Answerable questions with results: top1 range [0.42, 0.75]
            // - Threshold 0.35 safely separates (below min answerable 0.42)
            min_top1_score: 0.35,
            // Reduced from 0.1 to avoid over-abstention on clustered results
            min_score_gap: 0.05,
            min_results: 1,
            min_entity_coverage: 0.0, // Don't require entity coverage by default
            entity_coverage_weight: 0.3,
            enabled: true,
            cluster_similarity_threshold: 0.35, // 35% word overlap = same answer cluster
            cluster_check_depth: 3,             // Check top 3 results for clustering
        }
    }
}

impl AbstentionConfig {
    /// Load from environment variables
    pub fn from_env() -> Self {
        Self {
            min_top1_score: env::var("ABSTENTION_MIN_SCORE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.35),
            min_score_gap: env::var("ABSTENTION_MIN_GAP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.05),
            min_results: env::var("ABSTENTION_MIN_RESULTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            min_entity_coverage: env::var("ABSTENTION_MIN_COVERAGE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            entity_coverage_weight: env::var("ABSTENTION_COVERAGE_WEIGHT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.3),
            enabled: env::var("ABSTENTION_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            cluster_similarity_threshold: env::var("ABSTENTION_CLUSTER_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.6),
            cluster_check_depth: env::var("ABSTENTION_CLUSTER_DEPTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
        }
    }

    /// Disabled configuration (always proceed)
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

/// Confidence level of retrieval results
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalConfidence {
    /// High confidence in results
    High,
    /// Medium confidence, results may be partial
    Medium,
    /// Low confidence, should consider abstaining
    Low,
    /// No relevant results found, abstain from answering
    Abstain,
}

impl RetrievalConfidence {
    pub fn should_abstain(&self) -> bool {
        matches!(self, RetrievalConfidence::Abstain)
    }

    pub fn is_confident(&self) -> bool {
        matches!(
            self,
            RetrievalConfidence::High | RetrievalConfidence::Medium
        )
    }
}

/// Reason for abstention
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AbstentionReason {
    /// Top retrieval score below minimum threshold
    NoRelevantMemories,
    /// Score is low and gap to next result is small
    LowConfidence,
    /// Query entities not found in any retrieved memory
    EntityNotFound,
    /// Insufficient number of results returned
    InsufficientResults,
}

impl AbstentionReason {
    /// Get human-readable message for this reason
    pub fn message(&self) -> &'static str {
        match self {
            Self::NoRelevantMemories => "I don't have any relevant information about that.",
            Self::LowConfidence => "I'm not confident enough to answer that question.",
            Self::EntityNotFound => "I don't have information about the entities you mentioned.",
            Self::InsufficientResults => "I couldn't find enough information to answer.",
        }
    }
}

/// Result of abstention check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbstentionResult {
    /// Proceed with answer generation
    Proceed,
    /// Abstain from answering
    Abstain(AbstentionReason),
}

impl AbstentionResult {
    /// Check if result is abstain
    pub fn is_abstain(&self) -> bool {
        matches!(self, AbstentionResult::Abstain(_))
    }

    /// Get the abstention reason if abstaining
    pub fn reason(&self) -> Option<AbstentionReason> {
        match self {
            AbstentionResult::Abstain(reason) => Some(*reason),
            AbstentionResult::Proceed => None,
        }
    }

    /// Get the message if abstaining
    pub fn message(&self) -> Option<&'static str> {
        self.reason().map(|r| r.message())
    }
}

/// Scoring and abstention detection
pub struct ConfidenceScorer {
    config: AbstentionConfig,
}

impl ConfidenceScorer {
    pub fn new(config: AbstentionConfig) -> Self {
        Self { config }
    }

    /// Assess confidence level for retrieval results
    pub fn assess(&self, results: &[RerankedResult]) -> ConfidenceAssessment {
        self.assess_with_entities(results, &[])
    }

    /// Assess confidence level with entity coverage
    pub fn assess_with_entities(
        &self,
        results: &[RerankedResult],
        query_entities: &[String],
    ) -> ConfidenceAssessment {
        if !self.config.enabled {
            return ConfidenceAssessment {
                confidence: RetrievalConfidence::High,
                top1_score: results.first().map(|r| r.final_score).unwrap_or(0.0),
                score_gap: None,
                entity_coverage: None,
                overall_score: 1.0,
                reason: "Abstention disabled".to_string(),
            };
        }

        // Check minimum results
        if results.len() < self.config.min_results {
            return ConfidenceAssessment {
                confidence: RetrievalConfidence::Abstain,
                top1_score: 0.0,
                score_gap: None,
                entity_coverage: None,
                overall_score: 0.0,
                reason: format!(
                    "Insufficient results: {} < {}",
                    results.len(),
                    self.config.min_results
                ),
            };
        }

        let top1_score = results.first().map(|r| r.final_score).unwrap_or(0.0);
        let top2_score = results.get(1).map(|r| r.final_score).unwrap_or(0.0);
        let score_gap = top1_score - top2_score;

        // Calculate entity coverage if entities provided
        let entity_coverage = if query_entities.is_empty() {
            None
        } else {
            Some(Self::compute_entity_coverage(results, query_entities))
        };

        // Compute overall score
        let overall_score =
            self.compute_overall_score(top1_score, score_gap, entity_coverage.unwrap_or(1.0));

        // Check top-1 threshold
        if top1_score < self.config.min_top1_score {
            return ConfidenceAssessment {
                confidence: RetrievalConfidence::Abstain,
                top1_score,
                score_gap: Some(score_gap),
                entity_coverage,
                overall_score,
                reason: format!(
                    "Top-1 score too low: {:.3} < {:.3}",
                    top1_score, self.config.min_top1_score
                ),
            };
        }

        // Check entity coverage threshold
        if let Some(coverage) = entity_coverage {
            if coverage < self.config.min_entity_coverage {
                return ConfidenceAssessment {
                    confidence: RetrievalConfidence::Abstain,
                    top1_score,
                    score_gap: Some(score_gap),
                    entity_coverage: Some(coverage),
                    overall_score,
                    reason: format!(
                        "Entity coverage too low: {:.3} < {:.3}",
                        coverage, self.config.min_entity_coverage
                    ),
                };
            }
        }

        // Assess confidence level based on overall score
        let confidence = if overall_score > 0.7 && score_gap > self.config.min_score_gap {
            RetrievalConfidence::High
        } else if overall_score > 0.5 {
            RetrievalConfidence::Medium
        } else if overall_score > self.config.min_top1_score {
            RetrievalConfidence::Low
        } else {
            RetrievalConfidence::Abstain
        };

        ConfidenceAssessment {
            confidence,
            top1_score,
            score_gap: Some(score_gap),
            entity_coverage,
            overall_score,
            reason: format!(
                "Top-1: {:.3}, Gap: {:.3}, Coverage: {:?}",
                top1_score, score_gap, entity_coverage
            ),
        }
    }

    /// Compute entity coverage (0-1)
    fn compute_entity_coverage(results: &[RerankedResult], query_entities: &[String]) -> f32 {
        if query_entities.is_empty() {
            return 1.0;
        }

        // Collect all entity names from retrieved memories
        let result_entities: std::collections::HashSet<String> = results
            .iter()
            .flat_map(|r| r.memory.entity_names.iter())
            .map(|e| e.to_lowercase())
            .collect();

        // Count how many query entities appear in results
        let matched = query_entities
            .iter()
            .filter(|e| result_entities.contains(&e.to_lowercase()))
            .count();

        matched as f32 / query_entities.len() as f32
    }

    /// Compute overall confidence score
    fn compute_overall_score(&self, top1: f32, gap: f32, coverage: f32) -> f32 {
        // Weighted combination
        // Base: 50% top1, 20% gap, 30% coverage (configurable)
        let score_weight = 1.0 - self.config.entity_coverage_weight - 0.2;
        let gap_weight = 0.2;
        let coverage_weight = self.config.entity_coverage_weight;

        let score = top1 * score_weight + gap * gap_weight + coverage * coverage_weight;
        score.max(0.0).min(1.0)
    }

    /// Compute content similarity between two texts using Jaccard similarity on word sets
    fn content_similarity(text1: &str, text2: &str) -> f32 {
        use std::collections::HashSet;

        // Normalize and tokenize
        let text1_lower = text1.to_lowercase();
        let text2_lower = text2.to_lowercase();

        let words1: HashSet<&str> = text1_lower
            .split_whitespace()
            .filter(|w| w.len() > 2) // Skip short words
            .collect();
        let words2: HashSet<&str> = text2_lower
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();

        if words1.is_empty() || words2.is_empty() {
            return 0.0;
        }

        // Jaccard similarity: |intersection| / |union|
        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    /// Check if top results form a reinforcing cluster (similar content = same answer)
    ///
    /// Returns true if the top N results are semantically similar, meaning they're
    /// all pointing to the same answer. In this case, small score gaps are actually
    /// reinforcing evidence, not uncertainty.
    fn top_results_form_cluster(&self, results: &[RerankedResult]) -> bool {
        let check_count = self.config.cluster_check_depth.min(results.len());
        if check_count < 2 {
            return false; // Need at least 2 results to form a cluster
        }

        let top_results: Vec<&str> = results
            .iter()
            .take(check_count)
            .map(|r| r.memory.content.as_str())
            .collect();

        // Check pairwise similarity between top-1 and other top results
        let first = top_results[0];
        for other in &top_results[1..] {
            let similarity = Self::content_similarity(first, other);
            if similarity < self.config.cluster_similarity_threshold {
                return false; // Found a dissimilar result, not a cluster
            }
        }

        true // All top results are similar = reinforcing cluster
    }

    /// Check if retrieval should abstain
    pub fn should_abstain(&self, results: &[RerankedResult]) -> bool {
        self.assess(results).confidence.should_abstain()
    }

    /// Check if retrieval should abstain with entity coverage
    pub fn should_abstain_with_entities(
        &self,
        results: &[RerankedResult],
        query_entities: &[String],
    ) -> bool {
        self.assess_with_entities(results, query_entities)
            .confidence
            .should_abstain()
    }

    /// Check abstention with detailed reason
    pub fn check_abstention(&self, results: &[RerankedResult]) -> AbstentionResult {
        self.check_abstention_with_entities(results, &[])
    }

    /// Check abstention with entity coverage and detailed reason
    pub fn check_abstention_with_entities(
        &self,
        results: &[RerankedResult],
        query_entities: &[String],
    ) -> AbstentionResult {
        // If disabled, always proceed
        if !self.config.enabled {
            return AbstentionResult::Proceed;
        }

        // Check minimum results
        if results.len() < self.config.min_results {
            return AbstentionResult::Abstain(AbstentionReason::InsufficientResults);
        }

        let top1_score = results.first().map(|r| r.final_score).unwrap_or(0.0);
        let top2_score = results.get(1).map(|r| r.final_score).unwrap_or(0.0);
        let score_gap = top1_score - top2_score;

        // Check minimum score threshold
        if top1_score < self.config.min_top1_score {
            return AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories);
        }

        // Check entity coverage if entities provided
        if !query_entities.is_empty() {
            let coverage = Self::compute_entity_coverage(results, query_entities);
            if coverage < self.config.min_entity_coverage {
                return AbstentionResult::Abstain(AbstentionReason::EntityNotFound);
            }
        }

        // Check score gap in low confidence zone
        // BUT: if top results form a reinforcing cluster (similar content), proceed anyway
        if top1_score < 0.5 && score_gap < self.config.min_score_gap {
            // Before abstaining, check if top results are all pointing to the same answer
            if self.top_results_form_cluster(results) {
                // Top results have similar content = reinforcing evidence, not uncertainty
                tracing::debug!(
                    top1_score = top1_score,
                    score_gap = score_gap,
                    "Score gap small but top results form reinforcing cluster - proceeding"
                );
                return AbstentionResult::Proceed;
            }
            return AbstentionResult::Abstain(AbstentionReason::LowConfidence);
        }

        AbstentionResult::Proceed
    }
}

/// Metrics for abstention calibration analysis
#[derive(Debug, Default)]
pub struct AbstentionMetrics {
    pub total_queries: u64,
    pub abstentions: u64,
    pub by_reason: HashMap<AbstentionReason, u64>,
    // Score distributions for calibration
    pub abstained_top1_scores: Vec<f32>,
    pub proceeded_top1_scores: Vec<f32>,
}

impl AbstentionMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, result: &AbstentionResult, top1_score: f32) {
        self.total_queries += 1;

        match result {
            AbstentionResult::Abstain(reason) => {
                self.abstentions += 1;
                *self.by_reason.entry(*reason).or_insert(0) += 1;
                self.abstained_top1_scores.push(top1_score);
            }
            AbstentionResult::Proceed => {
                self.proceeded_top1_scores.push(top1_score);
            }
        }
    }

    pub fn abstention_rate(&self) -> f64 {
        if self.total_queries == 0 {
            return 0.0;
        }
        self.abstentions as f64 / self.total_queries as f64 * 100.0
    }

    /// Get breakdown of abstention reasons
    pub fn reason_breakdown(&self) -> Vec<(AbstentionReason, u64, f64)> {
        let total = self.abstentions as f64;
        if total == 0.0 {
            return vec![];
        }

        self.by_reason
            .iter()
            .map(|(reason, count)| (*reason, *count, *count as f64 / total * 100.0))
            .collect()
    }

    /// Get average top1 score for abstained queries
    pub fn avg_abstained_score(&self) -> f32 {
        if self.abstained_top1_scores.is_empty() {
            return 0.0;
        }
        self.abstained_top1_scores.iter().sum::<f32>() / self.abstained_top1_scores.len() as f32
    }

    /// Get average top1 score for proceeded queries
    pub fn avg_proceeded_score(&self) -> f32 {
        if self.proceeded_top1_scores.is_empty() {
            return 0.0;
        }
        self.proceeded_top1_scores.iter().sum::<f32>() / self.proceeded_top1_scores.len() as f32
    }
}

/// Detailed confidence assessment
#[derive(Debug, Clone)]
pub struct ConfidenceAssessment {
    pub confidence: RetrievalConfidence,
    pub top1_score: f32,
    pub score_gap: Option<f32>,
    /// Entity coverage (0-1): fraction of query entities found in results
    pub entity_coverage: Option<f32>,
    /// Combined overall score (0-1)
    pub overall_score: f32,
    pub reason: String,
}

impl Default for ConfidenceScorer {
    fn default() -> Self {
        Self::new(AbstentionConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Memory;

    fn make_memory() -> Memory {
        Memory::new("test_user", "test content")
    }

    fn make_memory_with_entities(entities: Vec<&str>) -> Memory {
        let mut memory = Memory::new("test_user", "test content");
        memory.entity_names = entities.into_iter().map(String::from).collect();
        memory
    }

    #[test]
    fn test_confidence_high() {
        let scorer = ConfidenceScorer::default();

        let results = vec![make_reranked_result(0.9), make_reranked_result(0.5)];

        let assessment = scorer.assess(&results);
        assert_eq!(assessment.confidence, RetrievalConfidence::High);
        assert!(!assessment.confidence.should_abstain());
    }

    #[test]
    fn test_confidence_medium() {
        let scorer = ConfidenceScorer::default();

        let results = vec![make_reranked_result(0.6), make_reranked_result(0.55)];

        let assessment = scorer.assess(&results);
        assert_eq!(assessment.confidence, RetrievalConfidence::Medium);
        assert!(assessment.confidence.is_confident());
    }

    #[test]
    fn test_confidence_low() {
        let scorer = ConfidenceScorer::default();

        let results = vec![make_reranked_result(0.35), make_reranked_result(0.3)];

        let assessment = scorer.assess(&results);
        assert_eq!(assessment.confidence, RetrievalConfidence::Low);
        assert!(!assessment.confidence.is_confident());
    }

    #[test]
    fn test_confidence_abstain_low_score() {
        let scorer = ConfidenceScorer::default();

        let results = vec![make_reranked_result(0.1)];

        let assessment = scorer.assess(&results);
        assert_eq!(assessment.confidence, RetrievalConfidence::Abstain);
        assert!(assessment.confidence.should_abstain());
    }

    #[test]
    fn test_confidence_abstain_no_results() {
        let scorer = ConfidenceScorer::default();
        let results: Vec<RerankedResult> = vec![];

        let assessment = scorer.assess(&results);
        assert!(assessment.confidence.should_abstain());
    }

    #[test]
    fn test_abstention_disabled() {
        let config = AbstentionConfig {
            enabled: false,
            ..Default::default()
        };
        let scorer = ConfidenceScorer::new(config);

        let results = vec![make_reranked_result(0.1)];
        let assessment = scorer.assess(&results);

        // Should not abstain when disabled
        assert_eq!(assessment.confidence, RetrievalConfidence::High);
    }

    #[test]
    fn test_should_abstain_helper() {
        let scorer = ConfidenceScorer::default();

        let good_results = vec![make_reranked_result(0.9)];
        let bad_results = vec![make_reranked_result(0.1)];

        assert!(!scorer.should_abstain(&good_results));
        assert!(scorer.should_abstain(&bad_results));
    }

    #[test]
    fn test_custom_thresholds() {
        let config = AbstentionConfig {
            min_top1_score: 0.5,
            min_score_gap: 0.2,
            min_results: 2,
            ..Default::default()
        };
        let scorer = ConfidenceScorer::new(config);

        // Single result should abstain (min_results = 2)
        let single = vec![make_reranked_result(0.9)];
        assert!(scorer.should_abstain(&single));

        // Two results with low score should abstain
        let low_score = vec![make_reranked_result(0.4), make_reranked_result(0.3)];
        assert!(scorer.should_abstain(&low_score));

        // Two results with good score should not abstain
        let good = vec![make_reranked_result(0.8), make_reranked_result(0.5)];
        assert!(!scorer.should_abstain(&good));
    }

    #[test]
    fn test_entity_coverage_full() {
        let scorer = ConfidenceScorer::default();
        let results = vec![
            make_reranked_result_with_entities(0.9, vec!["Paris", "France"]),
            make_reranked_result_with_entities(0.6, vec!["London"]),
        ];
        let query_entities = vec!["Paris".to_string(), "France".to_string()];

        let assessment = scorer.assess_with_entities(&results, &query_entities);
        assert_eq!(assessment.entity_coverage, Some(1.0)); // All entities found
    }

    #[test]
    fn test_entity_coverage_partial() {
        let scorer = ConfidenceScorer::default();
        let results = vec![make_reranked_result_with_entities(0.9, vec!["Paris"])];
        let query_entities = vec!["Paris".to_string(), "London".to_string()];

        let assessment = scorer.assess_with_entities(&results, &query_entities);
        assert_eq!(assessment.entity_coverage, Some(0.5)); // 1 of 2 found
    }

    #[test]
    fn test_entity_coverage_none() {
        let scorer = ConfidenceScorer::default();
        let results = vec![make_reranked_result_with_entities(0.9, vec!["Berlin"])];
        let query_entities = vec!["Paris".to_string()];

        let assessment = scorer.assess_with_entities(&results, &query_entities);
        assert_eq!(assessment.entity_coverage, Some(0.0)); // No match
    }

    #[test]
    fn test_entity_coverage_empty_query() {
        let scorer = ConfidenceScorer::default();
        let results = vec![make_reranked_result(0.9)];
        let query_entities: Vec<String> = vec![];

        let assessment = scorer.assess_with_entities(&results, &query_entities);
        assert!(assessment.entity_coverage.is_none()); // No entities to check
    }

    #[test]
    fn test_entity_coverage_threshold() {
        let config = AbstentionConfig {
            min_entity_coverage: 0.5, // Require at least 50% coverage
            ..Default::default()
        };
        let scorer = ConfidenceScorer::new(config);

        // Low coverage should abstain
        let results = vec![make_reranked_result_with_entities(0.9, vec!["Other"])];
        let query_entities = vec!["Paris".to_string(), "London".to_string()];

        let assessment = scorer.assess_with_entities(&results, &query_entities);
        assert_eq!(assessment.confidence, RetrievalConfidence::Abstain);
    }

    #[test]
    fn test_overall_score_calculation() {
        let scorer = ConfidenceScorer::default();
        let results = vec![
            make_reranked_result_with_entities(0.8, vec!["Paris"]),
            make_reranked_result_with_entities(0.4, vec![]),
        ];
        let query_entities = vec!["Paris".to_string()];

        let assessment = scorer.assess_with_entities(&results, &query_entities);
        assert!(assessment.overall_score > 0.5); // Good overall score
        assert!(assessment.overall_score <= 1.0);
    }

    // Abstention Result and Reason tests

    #[test]
    fn test_abstention_reason_messages() {
        assert!(!AbstentionReason::NoRelevantMemories.message().is_empty());
        assert!(!AbstentionReason::LowConfidence.message().is_empty());
        assert!(!AbstentionReason::EntityNotFound.message().is_empty());
        assert!(!AbstentionReason::InsufficientResults.message().is_empty());
    }

    #[test]
    fn test_abstention_result_methods() {
        let proceed = AbstentionResult::Proceed;
        assert!(!proceed.is_abstain());
        assert!(proceed.reason().is_none());
        assert!(proceed.message().is_none());

        let abstain = AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories);
        assert!(abstain.is_abstain());
        assert_eq!(abstain.reason(), Some(AbstentionReason::NoRelevantMemories));
        assert!(abstain.message().is_some());
    }

    #[test]
    fn test_check_abstention_low_score() {
        let scorer = ConfidenceScorer::default();
        let results = vec![make_reranked_result(0.1)]; // Very low score

        let result = scorer.check_abstention(&results);
        assert!(result.is_abstain());
        assert_eq!(result.reason(), Some(AbstentionReason::NoRelevantMemories));
    }

    #[test]
    fn test_check_abstention_low_confidence_small_gap() {
        let scorer = ConfidenceScorer::default();
        // Score between min_top1_score (0.3) and 0.5, with small gap
        // Use different content so they don't form a reinforcing cluster
        let results = vec![
            make_reranked_result_with_content(0.4, "The user graduated with a degree in Business"),
            make_reranked_result_with_content(0.38, "The weather is sunny and warm today"),
        ];

        let result = scorer.check_abstention(&results);
        assert!(result.is_abstain());
        assert_eq!(result.reason(), Some(AbstentionReason::LowConfidence));
    }

    #[test]
    fn test_check_abstention_entity_not_found() {
        let config = AbstentionConfig {
            min_entity_coverage: 0.5,
            ..Default::default()
        };
        let scorer = ConfidenceScorer::new(config);
        let results = vec![make_reranked_result_with_entities(0.8, vec!["Other"])];
        let query_entities = vec!["Paris".to_string()];

        let result = scorer.check_abstention_with_entities(&results, &query_entities);
        assert!(result.is_abstain());
        assert_eq!(result.reason(), Some(AbstentionReason::EntityNotFound));
    }

    #[test]
    fn test_check_abstention_proceeds_high_confidence() {
        let scorer = ConfidenceScorer::default();
        let results = vec![make_reranked_result(0.9), make_reranked_result(0.5)];

        let result = scorer.check_abstention(&results);
        assert_eq!(result, AbstentionResult::Proceed);
    }

    #[test]
    fn test_check_abstention_disabled() {
        let scorer = ConfidenceScorer::new(AbstentionConfig::disabled());
        let results = vec![make_reranked_result(0.1)]; // Very low score

        let result = scorer.check_abstention(&results);
        assert_eq!(result, AbstentionResult::Proceed); // Should proceed even with low score
    }

    #[test]
    fn test_abstention_metrics_tracking() {
        let mut metrics = AbstentionMetrics::new();

        // Record some abstentions
        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories),
            0.2,
        );
        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::LowConfidence),
            0.4,
        );
        metrics.record(&AbstentionResult::Proceed, 0.8);

        assert_eq!(metrics.total_queries, 3);
        assert_eq!(metrics.abstentions, 2);
        assert!((metrics.abstention_rate() - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_abstention_metrics_reason_breakdown() {
        let mut metrics = AbstentionMetrics::new();

        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories),
            0.2,
        );
        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories),
            0.1,
        );
        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::LowConfidence),
            0.4,
        );

        let breakdown = metrics.reason_breakdown();
        assert_eq!(breakdown.len(), 2);

        let no_relevant = breakdown
            .iter()
            .find(|(r, _, _)| *r == AbstentionReason::NoRelevantMemories);
        assert!(no_relevant.is_some());
        assert_eq!(no_relevant.unwrap().1, 2);
    }

    #[test]
    fn test_abstention_metrics_avg_scores() {
        let mut metrics = AbstentionMetrics::new();

        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories),
            0.2,
        );
        metrics.record(
            &AbstentionResult::Abstain(AbstentionReason::NoRelevantMemories),
            0.4,
        );
        metrics.record(&AbstentionResult::Proceed, 0.8);
        metrics.record(&AbstentionResult::Proceed, 0.9);

        assert!((metrics.avg_abstained_score() - 0.3).abs() < 0.01);
        assert!((metrics.avg_proceeded_score() - 0.85).abs() < 0.01);
    }

    #[test]
    fn test_abstention_config_from_env() {
        // Test defaults when no env vars set
        let config = AbstentionConfig::from_env();
        assert!(config.enabled);
        assert!((config.min_top1_score - 0.35).abs() < 0.01);
    }

    #[test]
    fn test_abstention_config_disabled() {
        let config = AbstentionConfig::disabled();
        assert!(!config.enabled);
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

    fn make_reranked_result_with_entities(score: f32, entities: Vec<&str>) -> RerankedResult {
        RerankedResult {
            memory: make_memory_with_entities(entities),
            original_rrf_score: score,
            rerank_score: Some(score),
            final_score: score,
            contributing_channels: vec![],
        }
    }

    fn make_memory_with_content(content: &str) -> Memory {
        Memory::new("test_user", content)
    }

    fn make_reranked_result_with_content(score: f32, content: &str) -> RerankedResult {
        RerankedResult {
            memory: make_memory_with_content(content),
            original_rrf_score: score,
            rerank_score: Some(score),
            final_score: score,
            contributing_channels: vec![],
        }
    }

    // Answer clustering tests

    #[test]
    fn test_content_similarity_identical() {
        let similarity = ConfidenceScorer::content_similarity(
            "The user graduated with a Business Administration degree",
            "The user graduated with a Business Administration degree",
        );
        assert!((similarity - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_similar() {
        let similarity = ConfidenceScorer::content_similarity(
            "The user graduated with a degree in Business Administration",
            "User graduated with a Business Administration degree which helped them",
        );
        // Should be fairly similar (share many words)
        assert!(similarity > 0.4);
    }

    #[test]
    fn test_content_similarity_different() {
        let similarity = ConfidenceScorer::content_similarity(
            "The user graduated with a degree in Business Administration",
            "The weather today is sunny and warm",
        );
        // Should be very different
        assert!(similarity < 0.2);
    }

    #[test]
    fn test_cluster_detection_similar_results() {
        let scorer = ConfidenceScorer::default();

        // Three similar results about Business Administration
        let results = vec![
            make_reranked_result_with_content(
                0.45,
                "The user graduated with a degree in Business Administration which has helped them",
            ),
            make_reranked_result_with_content(
                0.43,
                "User graduated with a Business Administration degree from their university",
            ),
            make_reranked_result_with_content(
                0.42,
                "The user has a Business Administration degree and graduated last year",
            ),
        ];

        assert!(scorer.top_results_form_cluster(&results));
    }

    #[test]
    fn test_cluster_detection_different_results() {
        let scorer = ConfidenceScorer::default();

        // Three different results about different topics
        let results = vec![
            make_reranked_result_with_content(
                0.45,
                "The user graduated with a degree in Business Administration",
            ),
            make_reranked_result_with_content(
                0.43,
                "The weather today is sunny and the temperature is 75 degrees",
            ),
            make_reranked_result_with_content(0.42, "User ordered pizza from the local restaurant"),
        ];

        assert!(!scorer.top_results_form_cluster(&results));
    }

    #[test]
    fn test_abstention_with_reinforcing_cluster() {
        let scorer = ConfidenceScorer::default();

        // Low score gap but similar content = should NOT abstain
        let results = vec![
            make_reranked_result_with_content(
                0.45,
                "The user graduated with a degree in Business Administration which has helped them",
            ),
            make_reranked_result_with_content(
                0.44, // Very small gap (0.01)
                "User graduated with a Business Administration degree from their university",
            ),
            make_reranked_result_with_content(
                0.43,
                "The user has a Business Administration degree and graduated last year",
            ),
        ];

        let result = scorer.check_abstention(&results);
        assert!(
            !result.is_abstain(),
            "Should NOT abstain when top results form a reinforcing cluster"
        );
    }

    #[test]
    fn test_abstention_with_divergent_results() {
        let scorer = ConfidenceScorer::default();

        // Low score gap with different content = should abstain
        let results = vec![
            make_reranked_result_with_content(
                0.45,
                "The user graduated with a degree in Business Administration",
            ),
            make_reranked_result_with_content(
                0.44, // Very small gap
                "The weather forecast shows rain tomorrow afternoon",
            ),
        ];

        let result = scorer.check_abstention(&results);
        assert!(
            result.is_abstain(),
            "Should abstain when top results are divergent"
        );
        assert_eq!(result.reason(), Some(AbstentionReason::LowConfidence));
    }
}
