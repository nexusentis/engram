//! Temporal context builder for answer generation
//!
//! Provides timestamp formatting and context building for temporal queries.

use chrono::{DateTime, Utc};

use super::query_analyzer::TemporalIntent;
use crate::types::Memory;

/// Formats timestamps for LLM readability
pub struct TimestampFormatter;

impl TimestampFormatter {
    /// Format timestamp for human readability
    pub fn format(dt: DateTime<Utc>) -> String {
        // Format: "January 15, 2024 at 2:30 PM"
        dt.format("%B %d, %Y at %I:%M %p").to_string()
    }

    /// Format timestamp with relative description
    pub fn format_with_relative(dt: DateTime<Utc>) -> String {
        let now = Utc::now();
        let diff = now.signed_duration_since(dt);

        let relative = if diff.num_hours() < 0 {
            "in the future".to_string()
        } else if diff.num_hours() < 1 {
            "just now".to_string()
        } else if diff.num_hours() < 24 {
            let hours = diff.num_hours();
            if hours == 1 {
                "1 hour ago".to_string()
            } else {
                format!("{} hours ago", hours)
            }
        } else if diff.num_days() < 7 {
            let days = diff.num_days();
            if days == 1 {
                "1 day ago".to_string()
            } else {
                format!("{} days ago", days)
            }
        } else if diff.num_weeks() < 4 {
            let weeks = diff.num_weeks();
            if weeks == 1 {
                "1 week ago".to_string()
            } else {
                format!("{} weeks ago", weeks)
            }
        } else if diff.num_days() < 365 {
            let months = diff.num_days() / 30;
            if months == 1 {
                "1 month ago".to_string()
            } else {
                format!("{} months ago", months)
            }
        } else {
            let years = diff.num_days() / 365;
            if years == 1 {
                "1 year ago".to_string()
            } else {
                format!("{} years ago", years)
            }
        };

        format!("{} ({})", Self::format(dt), relative)
    }

    /// Format as compact date only (ISO format)
    pub fn format_date_only(dt: DateTime<Utc>) -> String {
        dt.format("%Y-%m-%d").to_string()
    }

    /// Format as compact date and time
    pub fn format_compact(dt: DateTime<Utc>) -> String {
        dt.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// Builds context with timestamps for temporal queries
pub struct TemporalContextBuilder;

impl TemporalContextBuilder {
    /// Build context string with timestamps when appropriate
    pub fn build(memories: &[Memory], intent: &TemporalIntent) -> String {
        let include_timestamps = Self::should_include_timestamps(intent);

        let mut context = String::new();

        for (i, memory) in memories.iter().enumerate() {
            if include_timestamps {
                let timestamp = TimestampFormatter::format_with_relative(memory.t_valid);
                context.push_str(&format!(
                    "[Memory {} - Valid as of: {}]\n{}\n\n",
                    i + 1,
                    timestamp,
                    memory.content
                ));
            } else {
                context.push_str(&format!("[Memory {}]\n{}\n\n", i + 1, memory.content));
            }
        }

        context
    }

    /// Determine if timestamps should be included based on temporal intent
    pub fn should_include_timestamps(intent: &TemporalIntent) -> bool {
        matches!(
            intent,
            TemporalIntent::Ordering | TemporalIntent::PointInTime | TemporalIntent::PastState
        )
    }

    /// Build context for ordering queries with sorted memories
    ///
    /// Sorts memories by timestamp (ascending for "first/earliest", descending for "last/latest")
    pub fn build_for_ordering(memories: &[Memory], ascending: bool) -> String {
        let mut sorted: Vec<&Memory> = memories.iter().collect();
        sorted.sort_by(|a, b| {
            if ascending {
                a.t_valid.cmp(&b.t_valid)
            } else {
                b.t_valid.cmp(&a.t_valid)
            }
        });

        let order_desc = if ascending {
            "oldest to newest"
        } else {
            "newest to oldest"
        };

        let mut context = format!(
            "Memories listed in chronological order ({}):\n\n",
            order_desc
        );

        for (i, memory) in sorted.iter().enumerate() {
            let timestamp = TimestampFormatter::format_with_relative(memory.t_valid);
            context.push_str(&format!(
                "[{}. {}]\n{}\n\n",
                i + 1,
                timestamp,
                memory.content
            ));
        }

        context
    }

    /// Determine if query wants ascending (oldest first) order
    pub fn wants_ascending_order(query: &str) -> bool {
        let q = query.to_lowercase();
        q.contains("first") || q.contains("earliest") || q.contains("oldest")
    }

    /// Get temporal instructions for LLM prompt
    pub fn temporal_instructions() -> &'static str {
        r#"TEMPORAL CONTEXT INSTRUCTIONS:
The memories below include timestamps showing when each piece of information was valid.
Use these timestamps to:
1. Determine which information is most recent (for "current" questions)
2. Identify the order of events (for "before/after" questions)
3. Find the first or last occurrence (for "first/last" questions)
4. Compare dates when asked about temporal relationships

If timestamps conflict with content, trust explicit statements in the content."#
    }

    /// Check if temporal instructions should be included
    pub fn needs_temporal_instructions(intent: &TemporalIntent) -> bool {
        !matches!(intent, TemporalIntent::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_timestamp_format() {
        let dt = Utc::now();
        let formatted = TimestampFormatter::format(dt);

        assert!(!formatted.is_empty());
        assert!(formatted.contains("at")); // "at X:XX AM/PM"
    }

    #[test]
    fn test_timestamp_format_with_relative_just_now() {
        let dt = Utc::now();
        let formatted = TimestampFormatter::format_with_relative(dt);

        assert!(formatted.contains("just now"));
    }

    #[test]
    fn test_timestamp_format_with_relative_hours() {
        let dt = Utc::now() - Duration::hours(5);
        let formatted = TimestampFormatter::format_with_relative(dt);

        assert!(formatted.contains("5 hours ago"));
    }

    #[test]
    fn test_timestamp_format_with_relative_days() {
        let dt = Utc::now() - Duration::days(3);
        let formatted = TimestampFormatter::format_with_relative(dt);

        assert!(formatted.contains("3 days ago"));
    }

    #[test]
    fn test_timestamp_format_with_relative_weeks() {
        let dt = Utc::now() - Duration::weeks(2);
        let formatted = TimestampFormatter::format_with_relative(dt);

        assert!(formatted.contains("2 weeks ago"));
    }

    #[test]
    fn test_timestamp_format_with_relative_months() {
        let dt = Utc::now() - Duration::days(60);
        let formatted = TimestampFormatter::format_with_relative(dt);

        assert!(formatted.contains("months ago"));
    }

    #[test]
    fn test_timestamp_format_date_only() {
        let dt = Utc::now();
        let formatted = TimestampFormatter::format_date_only(dt);

        // Should be YYYY-MM-DD format
        assert_eq!(formatted.len(), 10);
        assert!(formatted.contains('-'));
    }

    #[test]
    fn test_context_includes_timestamps_for_ordering() {
        let memories = vec![
            Memory::new("user", "Event A"),
            Memory::new("user", "Event B"),
        ];

        let context = TemporalContextBuilder::build(&memories, &TemporalIntent::Ordering);

        assert!(context.contains("Valid as of:"));
    }

    #[test]
    fn test_context_includes_timestamps_for_past_state() {
        let memories = vec![Memory::new("user", "Past event")];

        let context = TemporalContextBuilder::build(&memories, &TemporalIntent::PastState);

        assert!(context.contains("Valid as of:"));
    }

    #[test]
    fn test_context_includes_timestamps_for_point_in_time() {
        let memories = vec![Memory::new("user", "Specific event")];

        let context = TemporalContextBuilder::build(&memories, &TemporalIntent::PointInTime);

        assert!(context.contains("Valid as of:"));
    }

    #[test]
    fn test_context_excludes_timestamps_for_none() {
        let memories = vec![Memory::new("user", "Event A")];

        let context = TemporalContextBuilder::build(&memories, &TemporalIntent::None);

        assert!(!context.contains("Valid as of:"));
        assert!(context.contains("[Memory 1]"));
    }

    #[test]
    fn test_context_excludes_timestamps_for_current_state() {
        let memories = vec![Memory::new("user", "Current event")];

        let context = TemporalContextBuilder::build(&memories, &TemporalIntent::CurrentState);

        // CurrentState doesn't need timestamps (is_latest filter handles it)
        assert!(!context.contains("Valid as of:"));
    }

    #[test]
    fn test_ordering_context_sorted_ascending() {
        let now = Utc::now();

        let mut old = Memory::new("user", "Older event");
        old.t_valid = now - Duration::days(5);

        let mut new = Memory::new("user", "Newer event");
        new.t_valid = now - Duration::days(1);

        let memories = vec![new.clone(), old.clone()]; // Wrong order

        let context = TemporalContextBuilder::build_for_ordering(&memories, true);

        // Ascending order: older should come first
        let older_pos = context.find("Older event").unwrap();
        let newer_pos = context.find("Newer event").unwrap();
        assert!(older_pos < newer_pos, "Older should come before newer");
        assert!(context.contains("oldest to newest"));
    }

    #[test]
    fn test_ordering_context_sorted_descending() {
        let now = Utc::now();

        let mut old = Memory::new("user", "Older event");
        old.t_valid = now - Duration::days(5);

        let mut new = Memory::new("user", "Newer event");
        new.t_valid = now - Duration::days(1);

        let memories = vec![old.clone(), new.clone()]; // Wrong order for descending

        let context = TemporalContextBuilder::build_for_ordering(&memories, false);

        // Descending order: newer should come first
        let older_pos = context.find("Older event").unwrap();
        let newer_pos = context.find("Newer event").unwrap();
        assert!(newer_pos < older_pos, "Newer should come before older");
        assert!(context.contains("newest to oldest"));
    }

    #[test]
    fn test_wants_ascending_order() {
        assert!(TemporalContextBuilder::wants_ascending_order(
            "What was the first time?"
        ));
        assert!(TemporalContextBuilder::wants_ascending_order(
            "Show me the earliest event"
        ));
        assert!(TemporalContextBuilder::wants_ascending_order(
            "What's the oldest memory?"
        ));

        assert!(!TemporalContextBuilder::wants_ascending_order(
            "What was the last time?"
        ));
        assert!(!TemporalContextBuilder::wants_ascending_order(
            "Show me recent events"
        ));
    }

    #[test]
    fn test_should_include_timestamps() {
        assert!(TemporalContextBuilder::should_include_timestamps(
            &TemporalIntent::Ordering
        ));
        assert!(TemporalContextBuilder::should_include_timestamps(
            &TemporalIntent::PointInTime
        ));
        assert!(TemporalContextBuilder::should_include_timestamps(
            &TemporalIntent::PastState
        ));

        assert!(!TemporalContextBuilder::should_include_timestamps(
            &TemporalIntent::CurrentState
        ));
        assert!(!TemporalContextBuilder::should_include_timestamps(
            &TemporalIntent::None
        ));
    }

    #[test]
    fn test_needs_temporal_instructions() {
        assert!(TemporalContextBuilder::needs_temporal_instructions(
            &TemporalIntent::Ordering
        ));
        assert!(TemporalContextBuilder::needs_temporal_instructions(
            &TemporalIntent::CurrentState
        ));
        assert!(TemporalContextBuilder::needs_temporal_instructions(
            &TemporalIntent::PastState
        ));

        assert!(!TemporalContextBuilder::needs_temporal_instructions(
            &TemporalIntent::None
        ));
    }

    #[test]
    fn test_temporal_instructions_content() {
        let instructions = TemporalContextBuilder::temporal_instructions();

        assert!(instructions.contains("timestamps"));
        assert!(instructions.contains("order of events"));
    }
}
