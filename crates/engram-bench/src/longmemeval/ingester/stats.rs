//! Ingestion statistics types.

use qdrant_client::qdrant::{value::Kind, Value};

/// Parse a datetime string from a Qdrant payload field
pub(super) fn parse_payload_datetime(
    payload: &std::collections::HashMap<String, Value>,
    key: &str,
) -> Option<chrono::DateTime<chrono::Utc>> {
    payload
        .get(key)
        .and_then(|v| v.kind.as_ref())
        .and_then(|k| match k {
            Kind::StringValue(s) => chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            _ => None,
        })
}

/// Statistics from ingesting sessions
#[derive(Debug, Default, Clone)]
pub struct IngestionStats {
    /// Number of sessions processed
    pub sessions_processed: usize,
    /// Number of memories created
    pub memories_created: usize,
    /// Number of entities extracted
    pub entities_extracted: usize,
    /// Total facts extracted
    pub facts_extracted: usize,
    /// Total raw message turns stored
    pub messages_stored: usize,
    /// Errors encountered
    pub errors: Vec<String>,
}

impl IngestionStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Add stats from processing a session
    pub fn add_session(&mut self, stats: SessionStats) {
        self.sessions_processed += 1;
        self.memories_created += stats.memories_created;
        self.entities_extracted += stats.entities_extracted;
        self.facts_extracted += stats.facts_extracted;
        self.messages_stored += stats.messages_stored;
    }

    /// Record an error
    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    /// Check if any errors occurred
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get success rate
    pub fn success_rate(&self) -> f32 {
        if self.sessions_processed == 0 {
            0.0
        } else {
            let successful = self.sessions_processed - self.errors.len();
            successful as f32 / self.sessions_processed as f32
        }
    }
}

/// Statistics from processing a single session
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    /// Number of memories created
    pub memories_created: usize,
    /// Number of entities extracted
    pub entities_extracted: usize,
    /// Number of facts extracted
    pub facts_extracted: usize,
    /// Number of raw message turns stored
    pub messages_stored: usize,
}
