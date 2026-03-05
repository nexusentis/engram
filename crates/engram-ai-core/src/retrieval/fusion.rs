use std::collections::HashMap;
use uuid::Uuid;

use super::channel::ScoredResult;
use crate::types::Memory;

/// Configuration for RRF fusion
#[derive(Debug, Clone)]
pub struct RrfConfig {
    /// The k parameter in RRF formula: score = sum(1 / (k + rank))
    /// Higher k gives more weight to lower-ranked results
    /// Default: 60 (standard value from research)
    pub k: f32,
    /// Maximum results to return after fusion
    pub top_k: usize,
    /// Whether to normalize final scores to [0, 1]
    pub normalize_scores: bool,
}

impl Default for RrfConfig {
    fn default() -> Self {
        Self {
            k: 60.0,
            top_k: 20,
            normalize_scores: true,
        }
    }
}

impl RrfConfig {
    /// Create config with custom k parameter
    pub fn with_k(mut self, k: f32) -> Self {
        self.k = k;
        self
    }

    /// Create config with custom top_k limit
    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k;
        self
    }

    /// Create config with normalization setting
    pub fn with_normalization(mut self, normalize: bool) -> Self {
        self.normalize_scores = normalize;
        self
    }
}

/// A fused result with combined score
#[derive(Debug, Clone)]
pub struct FusedResult {
    pub memory: Memory,
    pub rrf_score: f32,
    pub contributing_channels: Vec<String>,
    pub channel_ranks: HashMap<String, usize>,
}

impl FusedResult {
    /// Check if result appeared in multiple channels
    pub fn is_cross_channel(&self) -> bool {
        self.contributing_channels.len() > 1
    }

    /// Get the best (lowest) rank across all channels
    pub fn best_rank(&self) -> usize {
        self.channel_ranks
            .values()
            .min()
            .copied()
            .unwrap_or(usize::MAX)
    }

    /// Get average rank across contributing channels
    pub fn average_rank(&self) -> f32 {
        if self.channel_ranks.is_empty() {
            return f32::MAX;
        }
        let sum: usize = self.channel_ranks.values().sum();
        sum as f32 / self.channel_ranks.len() as f32
    }

    /// Check if result was found by a specific channel
    pub fn found_by(&self, channel: &str) -> bool {
        self.contributing_channels.contains(&channel.to_string())
    }
}

/// Statistics about fusion results
#[derive(Debug)]
pub struct FusionStats {
    pub total_results: usize,
    pub cross_channel_results: usize,
    pub single_channel_results: usize,
    pub channel_contributions: HashMap<String, usize>,
}

/// Internal accumulator for fusion
struct FusionAccumulator {
    memory: Memory,
    total_score: f32,
    channels: Vec<String>,
    ranks: HashMap<String, usize>,
}

/// Reciprocal Rank Fusion implementation
pub struct RrfFusion {
    config: RrfConfig,
}

impl RrfFusion {
    /// Create a new RRF fusion with the given config
    pub fn new(config: RrfConfig) -> Self {
        Self { config }
    }

    /// Get the config
    pub fn config(&self) -> &RrfConfig {
        &self.config
    }

    /// Fuse results from multiple channels using RRF
    ///
    /// # Arguments
    /// * `channel_results` - Vec of (channel_name, weight, results) tuples
    ///
    /// # Returns
    /// * Fused and ranked results
    pub fn fuse(&self, channel_results: Vec<(&str, f32, Vec<ScoredResult>)>) -> Vec<FusedResult> {
        // Map from memory ID to accumulated RRF score and metadata
        let mut score_map: HashMap<Uuid, FusionAccumulator> = HashMap::new();

        for (channel_name, weight, results) in channel_results {
            for (rank, result) in results.into_iter().enumerate() {
                let memory_id = result.memory.id;

                // RRF formula: 1 / (k + rank)
                // rank is 0-indexed, so rank 0 -> 1/(k+1)
                let rrf_contribution = weight / (self.config.k + (rank as f32) + 1.0);

                let accumulator = score_map
                    .entry(memory_id)
                    .or_insert_with(|| FusionAccumulator {
                        memory: result.memory.clone(),
                        total_score: 0.0,
                        channels: Vec::new(),
                        ranks: HashMap::new(),
                    });

                accumulator.total_score += rrf_contribution;
                accumulator.channels.push(channel_name.to_string());
                accumulator.ranks.insert(channel_name.to_string(), rank + 1);
            }
        }

        // Convert to results and sort by score
        let mut fused: Vec<FusedResult> = score_map
            .into_values()
            .map(|acc| FusedResult {
                memory: acc.memory,
                rrf_score: acc.total_score,
                contributing_channels: acc.channels,
                channel_ranks: acc.ranks,
            })
            .collect();

        // Sort descending by RRF score
        fused.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Truncate to top_k
        fused.truncate(self.config.top_k);

        // Normalize scores if configured
        if self.config.normalize_scores && !fused.is_empty() {
            let max_score = fused[0].rrf_score;
            if max_score > 0.0 {
                for result in &mut fused {
                    result.rrf_score /= max_score;
                }
            }
        }

        fused
    }

    /// Simple fusion without channel weights (all channels equal weight 1.0)
    pub fn fuse_equal_weight(&self, channel_results: Vec<Vec<ScoredResult>>) -> Vec<FusedResult> {
        // Extract channel names first, then create weighted tuples
        let channel_names: Vec<String> = channel_results
            .iter()
            .map(|results| {
                results
                    .first()
                    .map(|r| r.channel_name.clone())
                    .unwrap_or_else(|| "unknown".to_string())
            })
            .collect();

        let weighted: Vec<(&str, f32, Vec<ScoredResult>)> = channel_names
            .iter()
            .zip(channel_results)
            .map(|(name, results)| (name.as_str(), 1.0, results))
            .collect();

        self.fuse(weighted)
    }

    /// Analyze fusion statistics
    pub fn analyze_fusion(&self, results: &[FusedResult]) -> FusionStats {
        let total = results.len();
        let cross_channel = results.iter().filter(|r| r.is_cross_channel()).count();

        let mut channel_counts: HashMap<String, usize> = HashMap::new();
        for result in results {
            for channel in &result.contributing_channels {
                *channel_counts.entry(channel.clone()).or_insert(0) += 1;
            }
        }

        FusionStats {
            total_results: total,
            cross_channel_results: cross_channel,
            single_channel_results: total - cross_channel,
            channel_contributions: channel_counts,
        }
    }
}

impl Default for RrfFusion {
    fn default() -> Self {
        Self::new(RrfConfig::default())
    }
}

/// Helper to create weighted channel results
pub fn weighted_results(
    channel_name: &str,
    weight: f32,
    results: Vec<ScoredResult>,
) -> (&str, f32, Vec<ScoredResult>) {
    (channel_name, weight, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_memory(id: Uuid, content: &str) -> Memory {
        let mut memory = Memory::new("user", content);
        // Override the auto-generated ID
        memory.id = id;
        memory
    }

    fn make_result(memory: Memory, score: f32, channel: &str) -> ScoredResult {
        ScoredResult {
            memory,
            score,
            channel_name: channel.to_string(),
        }
    }

    #[test]
    fn test_rrf_config_default() {
        let config = RrfConfig::default();
        assert_eq!(config.k, 60.0);
        assert_eq!(config.top_k, 20);
        assert!(config.normalize_scores);
    }

    #[test]
    fn test_rrf_config_builder() {
        let config = RrfConfig::default()
            .with_k(30.0)
            .with_top_k(10)
            .with_normalization(false);

        assert_eq!(config.k, 30.0);
        assert_eq!(config.top_k, 10);
        assert!(!config.normalize_scores);
    }

    #[test]
    fn test_rrf_basic_fusion() {
        let fusion = RrfFusion::default();

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let id3 = Uuid::now_v7();

        let semantic_results = vec![
            make_result(make_memory(id1, "Result 1"), 0.9, "semantic"),
            make_result(make_memory(id2, "Result 2"), 0.8, "semantic"),
        ];

        let keyword_results = vec![
            make_result(make_memory(id2, "Result 2"), 0.95, "keyword"),
            make_result(make_memory(id3, "Result 3"), 0.7, "keyword"),
        ];

        let fused = fusion.fuse(vec![
            ("semantic", 1.0, semantic_results),
            ("keyword", 1.0, keyword_results),
        ]);

        // Result 2 should be top because it appears in both channels
        assert_eq!(fused[0].memory.id, id2);
        assert!(fused[0].is_cross_channel());
        assert_eq!(fused[0].contributing_channels.len(), 2);
    }

    #[test]
    fn test_rrf_formula_correctness() {
        let config = RrfConfig {
            k: 60.0,
            top_k: 10,
            normalize_scores: false, // Don't normalize for this test
        };
        let fusion = RrfFusion::new(config);

        let id1 = Uuid::now_v7();

        // Single result at rank 0 in one channel
        let results = vec![make_result(make_memory(id1, "Result"), 1.0, "test")];

        let fused = fusion.fuse(vec![("test", 1.0, results)]);

        // RRF score should be 1 / (60 + 0 + 1) = 1/61
        let expected = 1.0 / 61.0;
        assert!((fused[0].rrf_score - expected).abs() < 0.0001);
    }

    #[test]
    fn test_rrf_weighted_channels() {
        let config = RrfConfig {
            k: 60.0,
            top_k: 10,
            normalize_scores: false,
        };
        let fusion = RrfFusion::new(config);

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        // id1 at rank 0 in channel with weight 2.0
        let channel1 = vec![make_result(make_memory(id1, "R1"), 1.0, "high_weight")];
        // id2 at rank 0 in channel with weight 1.0
        let channel2 = vec![make_result(make_memory(id2, "R2"), 1.0, "low_weight")];

        let fused = fusion.fuse(vec![
            ("high_weight", 2.0, channel1),
            ("low_weight", 1.0, channel2),
        ]);

        // id1 should have higher score due to channel weight
        assert_eq!(fused[0].memory.id, id1);
    }

    #[test]
    fn test_rrf_deduplication() {
        let fusion = RrfFusion::default();

        let id1 = Uuid::now_v7();

        // Same memory in multiple channels
        let results1 = vec![make_result(make_memory(id1, "Same"), 0.9, "c1")];
        let results2 = vec![make_result(make_memory(id1, "Same"), 0.8, "c2")];
        let results3 = vec![make_result(make_memory(id1, "Same"), 0.7, "c3")];

        let fused = fusion.fuse(vec![
            ("c1", 1.0, results1),
            ("c2", 1.0, results2),
            ("c3", 1.0, results3),
        ]);

        // Should only appear once
        assert_eq!(fused.len(), 1);
        // But with combined score from all channels
        assert_eq!(fused[0].contributing_channels.len(), 3);
    }

    #[test]
    fn test_rrf_top_k_limit() {
        let config = RrfConfig {
            k: 60.0,
            top_k: 3,
            normalize_scores: true,
        };
        let fusion = RrfFusion::new(config);

        let results: Vec<ScoredResult> = (0..10)
            .map(|i| {
                make_result(
                    make_memory(Uuid::now_v7(), &format!("Result {}", i)),
                    1.0 - (i as f32 * 0.1),
                    "test",
                )
            })
            .collect();

        let fused = fusion.fuse(vec![("test", 1.0, results)]);

        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn test_rrf_normalization() {
        let config = RrfConfig {
            k: 60.0,
            top_k: 10,
            normalize_scores: true,
        };
        let fusion = RrfFusion::new(config);

        let results: Vec<ScoredResult> = (0..5)
            .map(|i| {
                make_result(
                    make_memory(Uuid::now_v7(), &format!("Result {}", i)),
                    1.0,
                    "test",
                )
            })
            .collect();

        let fused = fusion.fuse(vec![("test", 1.0, results)]);

        // First result should be normalized to 1.0
        assert!((fused[0].rrf_score - 1.0).abs() < 0.0001);

        // All scores should be in [0, 1]
        for result in &fused {
            assert!(result.rrf_score >= 0.0);
            assert!(result.rrf_score <= 1.0);
        }
    }

    #[test]
    fn test_fusion_stats() {
        let fusion = RrfFusion::default();

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        let r1 = vec![
            make_result(make_memory(id1, "R1"), 0.9, "semantic"),
            make_result(make_memory(id2, "R2"), 0.8, "semantic"),
        ];
        let r2 = vec![make_result(make_memory(id1, "R1"), 0.9, "keyword")];

        let fused = fusion.fuse(vec![("semantic", 1.0, r1), ("keyword", 1.0, r2)]);

        let stats = fusion.analyze_fusion(&fused);

        assert_eq!(stats.total_results, 2);
        assert_eq!(stats.cross_channel_results, 1);
        assert_eq!(stats.single_channel_results, 1);
    }

    #[test]
    fn test_fused_result_helpers() {
        let result = FusedResult {
            memory: make_memory(Uuid::now_v7(), "Test"),
            rrf_score: 0.5,
            contributing_channels: vec!["semantic".to_string(), "keyword".to_string()],
            channel_ranks: {
                let mut m = HashMap::new();
                m.insert("semantic".to_string(), 1);
                m.insert("keyword".to_string(), 3);
                m
            },
        };

        assert!(result.is_cross_channel());
        assert_eq!(result.best_rank(), 1);
        assert!((result.average_rank() - 2.0).abs() < 0.01);
        assert!(result.found_by("semantic"));
        assert!(!result.found_by("temporal"));
    }

    #[test]
    fn test_fused_result_single_channel() {
        let result = FusedResult {
            memory: make_memory(Uuid::now_v7(), "Test"),
            rrf_score: 0.5,
            contributing_channels: vec!["semantic".to_string()],
            channel_ranks: {
                let mut m = HashMap::new();
                m.insert("semantic".to_string(), 1);
                m
            },
        };

        assert!(!result.is_cross_channel());
        assert_eq!(result.best_rank(), 1);
        assert_eq!(result.average_rank(), 1.0);
    }

    #[test]
    fn test_fuse_equal_weight() {
        let fusion = RrfFusion::default();

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        let results1 = vec![make_result(make_memory(id1, "R1"), 0.9, "c1")];
        let results2 = vec![make_result(make_memory(id2, "R2"), 0.8, "c2")];

        let fused = fusion.fuse_equal_weight(vec![results1, results2]);

        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn test_weighted_results_helper() {
        let results = vec![make_result(
            make_memory(Uuid::now_v7(), "Test"),
            0.9,
            "test",
        )];

        let (name, weight, r) = weighted_results("test", 1.5, results);

        assert_eq!(name, "test");
        assert_eq!(weight, 1.5);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn test_empty_results() {
        let fusion = RrfFusion::default();
        let fused = fusion.fuse(vec![]);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_empty_channel_results() {
        let fusion = RrfFusion::default();
        let fused = fusion.fuse(vec![("empty1", 1.0, vec![]), ("empty2", 1.0, vec![])]);
        assert!(fused.is_empty());
    }
}
