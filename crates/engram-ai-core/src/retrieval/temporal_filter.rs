//! Temporal filter builder for Qdrant queries
//!
//! Builds filters based on temporal intent to control which memories are retrieved.

use qdrant_client::qdrant::{Condition, DatetimeRange, Filter, Timestamp};

use super::query_analyzer::{TemporalConstraint, TemporalIntent};

/// Builds Qdrant filters based on temporal intent
pub struct TemporalFilterBuilder;

impl TemporalFilterBuilder {
    /// Build filter conditions for temporal intent
    ///
    /// Returns conditions that should be added to the Qdrant filter.
    pub fn build(intent: &TemporalIntent, constraints: &[TemporalConstraint]) -> Vec<Condition> {
        match intent {
            TemporalIntent::CurrentState => Self::current_state_conditions(),
            TemporalIntent::PastState => Self::past_state_conditions(),
            TemporalIntent::PointInTime => Self::point_in_time_conditions(constraints),
            TemporalIntent::Ordering => vec![], // Handled by scoring, not filtering
            TemporalIntent::None => vec![],     // Default behavior
        }
    }

    /// Build filter for current state (is_latest = true)
    fn current_state_conditions() -> Vec<Condition> {
        vec![Condition::matches("is_latest", true)]
    }

    /// Build filter for past state (is_latest = false)
    fn past_state_conditions() -> Vec<Condition> {
        vec![Condition::matches("is_latest", false)]
    }

    /// Build filter for point in time (time range)
    fn point_in_time_conditions(constraints: &[TemporalConstraint]) -> Vec<Condition> {
        let mut conditions = Vec::new();

        // Use the first (most confident) constraint
        if let Some(constraint) = constraints.first() {
            let gte = constraint.start.map(|s| Timestamp {
                seconds: s.timestamp(),
                nanos: 0,
            });
            let lte = constraint.end.map(|e| Timestamp {
                seconds: e.timestamp(),
                nanos: 0,
            });

            if gte.is_some() || lte.is_some() {
                conditions.push(Condition::datetime_range(
                    "t_valid",
                    DatetimeRange {
                        gte,
                        lte,
                        ..Default::default()
                    },
                ));
            }
        }

        conditions
    }

    /// Build a complete filter combining user_id, temporal, and optional base conditions
    pub fn build_filter(
        user_id: &str,
        intent: &TemporalIntent,
        constraints: &[TemporalConstraint],
    ) -> Filter {
        let mut conditions = vec![Condition::matches("user_id", user_id.to_string())];

        // Add temporal conditions (CurrentState adds is_latest=true, PastState adds is_latest=false, etc.)
        // For None and Ordering, no additional filters — search all user memories
        conditions.extend(Self::build(intent, constraints));

        Filter::must(conditions)
    }

    /// Check if intent requires skipping is_latest filter
    ///
    /// Returns true if the intent explicitly handles is_latest (CurrentState, PastState)
    /// or needs historical data (PointInTime).
    pub fn overrides_is_latest(intent: &TemporalIntent) -> bool {
        matches!(
            intent,
            TemporalIntent::CurrentState | TemporalIntent::PastState | TemporalIntent::PointInTime
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_current_state_conditions() {
        let conditions = TemporalFilterBuilder::build(&TemporalIntent::CurrentState, &[]);
        assert_eq!(conditions.len(), 1);
    }

    #[test]
    fn test_past_state_conditions() {
        let conditions = TemporalFilterBuilder::build(&TemporalIntent::PastState, &[]);
        assert_eq!(conditions.len(), 1);
    }

    #[test]
    fn test_point_in_time_conditions() {
        let now = Utc::now();
        let constraint = TemporalConstraint {
            start: Some(now - Duration::days(7)),
            end: Some(now),
            expression: "last week".to_string(),
            confidence: 0.9,
        };

        let conditions = TemporalFilterBuilder::build(&TemporalIntent::PointInTime, &[constraint]);
        assert_eq!(conditions.len(), 1); // single datetime_range with gte + lte
    }

    #[test]
    fn test_point_in_time_start_only() {
        let now = Utc::now();
        let constraint = TemporalConstraint {
            start: Some(now - Duration::days(7)),
            end: None,
            expression: "since last week".to_string(),
            confidence: 0.9,
        };

        let conditions = TemporalFilterBuilder::build(&TemporalIntent::PointInTime, &[constraint]);
        assert_eq!(conditions.len(), 1);
    }

    #[test]
    fn test_ordering_no_conditions() {
        let conditions = TemporalFilterBuilder::build(&TemporalIntent::Ordering, &[]);
        assert!(conditions.is_empty());
    }

    #[test]
    fn test_none_no_conditions() {
        let conditions = TemporalFilterBuilder::build(&TemporalIntent::None, &[]);
        assert!(conditions.is_empty());
    }

    #[test]
    fn test_build_filter_with_user_id() {
        let filter =
            TemporalFilterBuilder::build_filter("test-user", &TemporalIntent::CurrentState, &[]);

        // Should have user_id and is_latest conditions
        assert!(!filter.must.is_empty());
    }

    #[test]
    fn test_build_filter_none_no_is_latest() {
        let filter = TemporalFilterBuilder::build_filter("test-user", &TemporalIntent::None, &[]);

        // Should have only user_id condition — no is_latest filter for non-temporal queries
        assert_eq!(filter.must.len(), 1);
    }

    #[test]
    fn test_overrides_is_latest() {
        assert!(TemporalFilterBuilder::overrides_is_latest(
            &TemporalIntent::CurrentState
        ));
        assert!(TemporalFilterBuilder::overrides_is_latest(
            &TemporalIntent::PastState
        ));
        assert!(TemporalFilterBuilder::overrides_is_latest(
            &TemporalIntent::PointInTime
        ));
        assert!(!TemporalFilterBuilder::overrides_is_latest(
            &TemporalIntent::Ordering
        ));
        assert!(!TemporalFilterBuilder::overrides_is_latest(
            &TemporalIntent::None
        ));
    }
}
