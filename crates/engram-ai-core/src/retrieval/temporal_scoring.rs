//! Temporal Semantic Memory (TSM) scoring
//!
//! Combines semantic similarity with temporal recency for relevance scoring.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::query_analyzer::TemporalIntent;
use crate::types::Memory;

/// Configuration for Temporal Semantic Memory scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsmConfig {
    /// Weight for semantic (embedding) similarity [0-1]
    pub semantic_weight: f32,

    /// Weight for temporal recency [0-1]
    pub temporal_weight: f32,

    /// Half-life for recency decay in days
    pub decay_half_life_days: f64,

    /// Whether to boost is_latest memories
    pub boost_latest: bool,

    /// Boost factor for is_latest memories
    pub latest_boost: f32,
}

impl Default for TsmConfig {
    fn default() -> Self {
        Self {
            semantic_weight: 0.7,       // Configurable, initial value
            temporal_weight: 0.3,       // Configurable, initial value
            decay_half_life_days: 30.0, // One month half-life
            boost_latest: true,
            latest_boost: 1.2,
        }
    }
}

impl TsmConfig {
    /// Create a new TSM config
    pub fn new() -> Self {
        Self::default()
    }

    /// Config optimized for current-state queries
    pub fn for_current_state() -> Self {
        Self {
            semantic_weight: 0.6,
            temporal_weight: 0.4,
            decay_half_life_days: 14.0, // Shorter half-life for current state
            boost_latest: true,
            latest_boost: 1.3,
        }
    }

    /// Config optimized for past-state queries
    pub fn for_past_state() -> Self {
        Self {
            semantic_weight: 0.8,
            temporal_weight: 0.2,
            decay_half_life_days: 90.0, // Longer half-life for historical
            boost_latest: false,
            latest_boost: 1.0,
        }
    }

    /// Config for temporal ordering queries
    pub fn for_ordering() -> Self {
        Self {
            semantic_weight: 0.5,
            temporal_weight: 0.5, // Equal weight for ordering
            decay_half_life_days: 365.0,
            boost_latest: false,
            latest_boost: 1.0,
        }
    }

    /// Config for point-in-time queries
    pub fn for_point_in_time() -> Self {
        Self {
            semantic_weight: 0.8,
            temporal_weight: 0.2,
            decay_half_life_days: 60.0,
            boost_latest: false,
            latest_boost: 1.0,
        }
    }

    /// Get config for a given temporal intent
    pub fn for_intent(intent: &TemporalIntent) -> Self {
        match intent {
            TemporalIntent::CurrentState => Self::for_current_state(),
            TemporalIntent::PastState => Self::for_past_state(),
            TemporalIntent::PointInTime => Self::for_point_in_time(),
            TemporalIntent::Ordering => Self::for_ordering(),
            TemporalIntent::None => Self::default(),
        }
    }

    /// Set semantic weight
    pub fn with_semantic_weight(mut self, weight: f32) -> Self {
        self.semantic_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set temporal weight
    pub fn with_temporal_weight(mut self, weight: f32) -> Self {
        self.temporal_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set decay half-life
    pub fn with_decay_half_life(mut self, days: f64) -> Self {
        self.decay_half_life_days = days.max(1.0);
        self
    }

    /// Enable/disable is_latest boost
    pub fn with_latest_boost(mut self, enable: bool, factor: f32) -> Self {
        self.boost_latest = enable;
        self.latest_boost = factor.max(1.0);
        self
    }
}

/// Calculates recency score using exponential decay
#[derive(Debug, Clone)]
pub struct RecencyScorer {
    half_life_days: f64,
    reference_time: DateTime<Utc>,
}

impl RecencyScorer {
    /// Create a new recency scorer with the given half-life
    pub fn new(half_life_days: f64) -> Self {
        Self {
            half_life_days: half_life_days.max(1.0),
            reference_time: Utc::now(),
        }
    }

    /// Create from TsmConfig
    pub fn from_config(config: &TsmConfig) -> Self {
        Self::new(config.decay_half_life_days)
    }

    /// Set a custom reference time (for testing)
    pub fn with_reference_time(mut self, time: DateTime<Utc>) -> Self {
        self.reference_time = time;
        self
    }

    /// Calculate recency score [0-1] for a given timestamp
    ///
    /// Uses exponential decay: score = 2^(-age/half_life)
    /// - At t=0 (current): score = 1.0
    /// - At t=half_life: score = 0.5
    /// - At t=2*half_life: score = 0.25
    pub fn score(&self, t_valid: DateTime<Utc>) -> f32 {
        let age_days = (self.reference_time - t_valid).num_days() as f64;

        if age_days <= 0.0 {
            return 1.0;
        }

        let decay = 2.0_f64.powf(-age_days / self.half_life_days);
        (decay as f32).clamp(0.0, 1.0)
    }

    /// Score with hours precision for finer-grained recency
    pub fn score_precise(&self, t_valid: DateTime<Utc>) -> f32 {
        let age_hours = (self.reference_time - t_valid).num_hours() as f64;
        let age_days = age_hours / 24.0;

        if age_days <= 0.0 {
            return 1.0;
        }

        let decay = 2.0_f64.powf(-age_days / self.half_life_days);
        (decay as f32).clamp(0.0, 1.0)
    }
}

impl Default for RecencyScorer {
    fn default() -> Self {
        Self::new(30.0) // 30-day half-life default
    }
}

/// Combines semantic and temporal scores using TSM
#[derive(Debug, Clone)]
pub struct TsmScorer {
    config: TsmConfig,
    recency_scorer: RecencyScorer,
}

impl TsmScorer {
    /// Create a new TSM scorer with the given config
    pub fn new(config: TsmConfig) -> Self {
        let recency_scorer = RecencyScorer::from_config(&config);
        Self {
            config,
            recency_scorer,
        }
    }

    /// Create scorer for a temporal intent
    pub fn for_intent(intent: &TemporalIntent) -> Self {
        Self::new(TsmConfig::for_intent(intent))
    }

    /// Set a custom reference time (for testing)
    pub fn with_reference_time(mut self, time: DateTime<Utc>) -> Self {
        self.recency_scorer = self.recency_scorer.with_reference_time(time);
        self
    }

    /// Calculate combined TSM score for a memory
    ///
    /// Returns a score combining semantic similarity and temporal recency.
    pub fn score(&self, semantic_score: f32, memory: &Memory) -> f32 {
        let recency_score = self.recency_scorer.score(memory.t_valid);

        // Weighted combination
        let mut combined = self.config.semantic_weight * semantic_score
            + self.config.temporal_weight * recency_score;

        // Apply is_latest boost if configured
        if self.config.boost_latest && memory.is_latest {
            combined *= self.config.latest_boost;
        }

        // Clamp to [0, 1] range
        combined.clamp(0.0, 1.0)
    }

    /// Score a list of memories and sort by TSM score
    ///
    /// Returns (memory, original_score, tsm_score) tuples sorted by TSM score descending.
    pub fn score_and_sort(&self, memories: Vec<(Memory, f32)>) -> Vec<(Memory, f32, f32)> {
        let mut scored: Vec<_> = memories
            .into_iter()
            .map(|(memory, semantic_score)| {
                let tsm_score = self.score(semantic_score, &memory);
                (memory, semantic_score, tsm_score)
            })
            .collect();

        // Sort by TSM score descending
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        scored
    }

    /// Get the config
    pub fn config(&self) -> &TsmConfig {
        &self.config
    }
}

impl Default for TsmScorer {
    fn default() -> Self {
        Self::new(TsmConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_tsm_config_default() {
        let config = TsmConfig::default();
        assert_eq!(config.semantic_weight, 0.7);
        assert_eq!(config.temporal_weight, 0.3);
        assert!(config.boost_latest);
    }

    #[test]
    fn test_tsm_config_for_intent() {
        let current = TsmConfig::for_current_state();
        assert!(current.boost_latest);
        assert!(current.temporal_weight > TsmConfig::default().temporal_weight);

        let past = TsmConfig::for_past_state();
        assert!(!past.boost_latest);

        let ordering = TsmConfig::for_ordering();
        assert_eq!(ordering.semantic_weight, 0.5);
    }

    #[test]
    fn test_recency_score_now() {
        let scorer = RecencyScorer::default();
        let score = scorer.score(Utc::now());
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_recency_score_half_life() {
        let now = Utc::now();
        let scorer = RecencyScorer::new(30.0).with_reference_time(now);

        let thirty_days_ago = now - Duration::days(30);
        let score = scorer.score(thirty_days_ago);

        assert!(
            (score - 0.5).abs() < 0.01,
            "Score at half-life should be ~0.5, got {}",
            score
        );
    }

    #[test]
    fn test_recency_score_double_half_life() {
        let now = Utc::now();
        let scorer = RecencyScorer::new(30.0).with_reference_time(now);

        let sixty_days_ago = now - Duration::days(60);
        let score = scorer.score(sixty_days_ago);

        assert!(
            (score - 0.25).abs() < 0.01,
            "Score at 2x half-life should be ~0.25, got {}",
            score
        );
    }

    #[test]
    fn test_recency_score_decay_ordering() {
        let now = Utc::now();
        let scorer = RecencyScorer::new(30.0).with_reference_time(now);

        let score_7d = scorer.score(now - Duration::days(7));
        let score_30d = scorer.score(now - Duration::days(30));
        let score_90d = scorer.score(now - Duration::days(90));

        assert!(score_7d > score_30d);
        assert!(score_30d > score_90d);
    }

    #[test]
    fn test_recency_score_future() {
        let scorer = RecencyScorer::default();
        let future = Utc::now() + Duration::days(1);
        let score = scorer.score(future);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_tsm_combined_score() {
        let config = TsmConfig::default()
            .with_semantic_weight(0.7)
            .with_temporal_weight(0.3)
            .with_latest_boost(false, 1.0);

        let now = Utc::now();
        let scorer = TsmScorer::new(config).with_reference_time(now);

        let memory = Memory::new("user", "test content");
        // Memory is current (t_valid = now), so recency = 1.0

        let combined = scorer.score(0.8, &memory);

        // Expected: 0.7 * 0.8 + 0.3 * 1.0 = 0.56 + 0.3 = 0.86
        assert!(
            (combined - 0.86).abs() < 0.05,
            "Combined score should be ~0.86, got {}",
            combined
        );
    }

    #[test]
    fn test_tsm_is_latest_boost() {
        let config = TsmConfig::default().with_latest_boost(true, 1.2);

        let scorer = TsmScorer::new(config);

        let mut memory_latest = Memory::new("user", "test");
        memory_latest.is_latest = true;

        let mut memory_not_latest = Memory::new("user", "test");
        memory_not_latest.is_latest = false;

        let score_latest = scorer.score(0.8, &memory_latest);
        let score_not_latest = scorer.score(0.8, &memory_not_latest);

        assert!(
            score_latest > score_not_latest,
            "is_latest should receive boost: {} vs {}",
            score_latest,
            score_not_latest
        );
    }

    #[test]
    fn test_tsm_score_and_sort() {
        let scorer = TsmScorer::default();
        let now = Utc::now();

        let mut mem1 = Memory::new("user", "recent high score");
        mem1.is_latest = true;

        let mut mem2 = Memory::new("user", "older content");
        mem2.t_valid = now - Duration::days(60);
        mem2.is_latest = false;

        let memories = vec![(mem1, 0.9), (mem2, 0.95)];

        let sorted = scorer.score_and_sort(memories);

        // First memory should win due to recency + is_latest boost
        assert_eq!(sorted[0].0.content, "recent high score");
    }

    #[test]
    fn test_tsm_for_intent() {
        let scorer = TsmScorer::for_intent(&TemporalIntent::CurrentState);
        assert!(scorer.config().boost_latest);

        let scorer = TsmScorer::for_intent(&TemporalIntent::PastState);
        assert!(!scorer.config().boost_latest);
    }

    #[test]
    fn test_tsm_config_builder() {
        let config = TsmConfig::new()
            .with_semantic_weight(0.8)
            .with_temporal_weight(0.2)
            .with_decay_half_life(60.0)
            .with_latest_boost(true, 1.5);

        assert_eq!(config.semantic_weight, 0.8);
        assert_eq!(config.temporal_weight, 0.2);
        assert_eq!(config.decay_half_life_days, 60.0);
        assert!(config.boost_latest);
        assert_eq!(config.latest_boost, 1.5);
    }

    #[test]
    fn test_tsm_config_clamping() {
        let config = TsmConfig::new()
            .with_semantic_weight(1.5) // Over 1.0
            .with_temporal_weight(-0.5); // Below 0.0

        assert_eq!(config.semantic_weight, 1.0);
        assert_eq!(config.temporal_weight, 0.0);
    }
}
