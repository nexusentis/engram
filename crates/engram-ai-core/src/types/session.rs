use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Conversation session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u32,
    pub memory_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_summary: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_mentions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topic_tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Session {
    pub fn new(user_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            user_id: user_id.into(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            memory_count: 0,
            timeline_summary: None,
            entity_mentions: Vec::new(),
            topic_tags: Vec::new(),
            metadata: None,
        }
    }

    /// Record a new message in this session
    pub fn record_message(&mut self) {
        self.message_count += 1;
        self.updated_at = Utc::now();
    }

    /// Record a memory extracted from this session
    pub fn record_memory(&mut self, entity_ids: &[String], topics: &[String]) {
        self.memory_count += 1;
        self.updated_at = Utc::now();

        for entity in entity_ids {
            if !self.entity_mentions.contains(entity) {
                self.entity_mentions.push(entity.clone());
            }
        }

        for topic in topics {
            if !self.topic_tags.contains(topic) {
                self.topic_tags.push(topic.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session::new("user_123");

        assert_eq!(session.user_id, "user_123");
        assert_eq!(session.message_count, 0);
        assert_eq!(session.memory_count, 0);
    }

    #[test]
    fn test_session_record_message() {
        let mut session = Session::new("user");

        session.record_message();
        session.record_message();

        assert_eq!(session.message_count, 2);
    }

    #[test]
    fn test_session_record_memory() {
        let mut session = Session::new("user");

        session.record_memory(
            &["alice".to_string(), "google".to_string()],
            &["career".to_string()],
        );

        assert_eq!(session.memory_count, 1);
        assert_eq!(session.entity_mentions.len(), 2);
        assert_eq!(session.topic_tags.len(), 1);

        // Record another with overlap
        session.record_memory(
            &["alice".to_string()], // Already present
            &["tech".to_string()],
        );

        assert_eq!(session.memory_count, 2);
        assert_eq!(session.entity_mentions.len(), 2); // No duplicate
        assert_eq!(session.topic_tags.len(), 2);
    }

    #[test]
    fn test_session_serialization() {
        let session = Session::new("user");

        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(session.id, deserialized.id);
        assert_eq!(session.user_id, deserialized.user_id);
    }
}
