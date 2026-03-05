use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{EpistemicType, FactType, SourceType};

/// A single turn in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub role: Role,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
}

impl ConversationTurn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            timestamp: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            timestamp: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            timestamp: None,
        }
    }

    pub fn with_timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.timestamp = Some(ts);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A conversation to be processed
#[derive(Debug, Clone)]
pub struct Conversation {
    pub user_id: String,
    pub session_id: Option<String>,
    pub turns: Vec<ConversationTurn>,
    pub timestamp: DateTime<Utc>,
}

impl Conversation {
    pub fn new(user_id: impl Into<String>, turns: Vec<ConversationTurn>) -> Self {
        Self {
            user_id: user_id.into(),
            session_id: None,
            turns,
            timestamp: Utc::now(),
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

/// Result of extracting a single memory
#[derive(Debug, Clone)]
pub struct ExtractedFact {
    pub content: String,
    pub confidence: f32,
    pub source_type: SourceType,
    pub fact_type: FactType,
    pub epistemic_type: EpistemicType,
    pub entities: Vec<ExtractedEntity>,
    pub temporal_markers: Vec<String>,
    pub t_valid: Option<DateTime<Utc>>,
    /// Observation level: "explicit", "deductive", or "contradiction"
    pub observation_level: String,
}

/// An extracted entity mention
#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: String,
    pub normalized_id: String,
}

/// Complete result from extraction pipeline
#[derive(Debug)]
pub struct ExtractionResult {
    pub facts: Vec<ExtractedFact>,
    pub model_used: String,
    pub processing_time_ms: u64,
    pub fallback_used: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_turn_builders() {
        let user_turn = ConversationTurn::user("Hello");
        assert_eq!(user_turn.role, Role::User);
        assert_eq!(user_turn.content, "Hello");

        let assistant_turn = ConversationTurn::assistant("Hi there");
        assert_eq!(assistant_turn.role, Role::Assistant);

        let system_turn = ConversationTurn::system("System message");
        assert_eq!(system_turn.role, Role::System);
    }

    #[test]
    fn test_conversation_with_session() {
        let conv = Conversation::new("user_123", vec![]).with_session("session_456");

        assert_eq!(conv.user_id, "user_123");
        assert_eq!(conv.session_id, Some("session_456".to_string()));
    }

    #[test]
    fn test_role_serialization() {
        let role = Role::User;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"user\"");

        let deserialized: Role = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(deserialized, Role::Assistant);
    }
}
