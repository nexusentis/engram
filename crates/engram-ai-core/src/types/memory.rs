use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::{EpistemicType, FactType, SourceType};

fn default_observation_level() -> String {
    "explicit".to_string()
}

/// Entity context from the conversation session
///
/// Captures conversation-level entity information for context-aware retrieval.
/// This helps resolve implicit references during both extraction and retrieval.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionEntityContext {
    /// Primary location/store mentioned in the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_location: Option<String>,
    /// Primary organization mentioned in the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_organization: Option<String>,
    /// Primary person (other than user) mentioned in the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_person: Option<String>,
    /// All entities extracted from the session
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_entities: Vec<String>,
}

impl SessionEntityContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: set primary location
    pub fn with_primary_location(mut self, location: impl Into<String>) -> Self {
        self.primary_location = Some(location.into());
        self
    }

    /// Builder: set primary organization
    pub fn with_primary_organization(mut self, org: impl Into<String>) -> Self {
        self.primary_organization = Some(org.into());
        self
    }

    /// Builder: set all entities
    pub fn with_entities(mut self, entities: Vec<String>) -> Self {
        self.all_entities = entities;
        self
    }

    /// Check if context has any meaningful data
    pub fn is_empty(&self) -> bool {
        self.primary_location.is_none()
            && self.primary_organization.is_none()
            && self.primary_person.is_none()
            && self.all_entities.is_empty()
    }
}

/// Core unit of stored knowledge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    // === Identity ===
    pub id: Uuid,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    // === Content ===
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,

    // === Bi-Temporal Timestamps ===
    pub t_created: DateTime<Utc>,
    pub t_valid: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t_expired: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t_invalid: Option<DateTime<Utc>>,

    // === Relational Versioning ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from_ids: Vec<Uuid>,
    pub is_latest: bool,

    // === Entity Tags ===
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_names: Vec<String>,

    // === Extraction Metadata ===
    pub confidence: f32,
    pub source_type: SourceType,
    pub fact_type: FactType,
    pub epistemic_type: EpistemicType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_mode: Option<String>,

    // === Observation Level ===
    /// Observation level: "explicit", "deductive", or "contradiction"
    #[serde(default = "default_observation_level")]
    pub observation_level: String,

    // === Causal Links ===
    /// UUIDs of facts causally linked to this one (cause → effect)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_links: Vec<Uuid>,

    // === Topic Tags ===
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topic_tags: Vec<String>,

    // === Retrieval Metadata ===
    #[serde(default)]
    pub retrieval_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_retrieved: Option<DateTime<Utc>>,

    // === Session Entity Context ===
    /// Entity context from the conversation session for implicit reference resolution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_entity_context: Option<SessionEntityContext>,
}

impl Memory {
    /// Create a new memory with generated UUIDv7
    pub fn new(user_id: impl Into<String>, content: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            user_id: user_id.into(),
            session_id: None,
            content: content.into(),
            content_hash: None,
            t_created: now,
            t_valid: now,
            t_expired: None,
            t_invalid: None,
            supersedes_id: None,
            extends_id: None,
            derived_from_ids: Vec::new(),
            is_latest: true,
            entity_ids: Vec::new(),
            entity_types: Vec::new(),
            entity_names: Vec::new(),
            observation_level: "explicit".to_string(),
            causal_links: Vec::new(),
            confidence: 1.0,
            source_type: SourceType::UserExplicit,
            fact_type: FactType::State,
            epistemic_type: EpistemicType::World,
            extraction_model: None,
            extraction_mode: None,
            topic_tags: Vec::new(),
            retrieval_count: 0,
            last_retrieved: None,
            session_entity_context: None,
        }
    }

    /// Builder: Set session ID
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Builder: Set fact validity time
    pub fn with_valid_time(mut self, t_valid: DateTime<Utc>) -> Self {
        self.t_valid = t_valid;
        self
    }

    /// Builder: Set source type and adjust confidence
    pub fn with_source(mut self, source_type: SourceType) -> Self {
        self.source_type = source_type;
        self.confidence *= source_type.confidence_multiplier();
        self
    }

    /// Builder: Set fact type
    pub fn with_fact_type(mut self, fact_type: FactType) -> Self {
        self.fact_type = fact_type;
        self
    }

    /// Builder: Set epistemic type
    pub fn with_epistemic_type(mut self, epistemic_type: EpistemicType) -> Self {
        self.epistemic_type = epistemic_type;
        self
    }

    /// Builder: Add entities
    pub fn with_entities(mut self, entities: Vec<(String, String, String)>) -> Self {
        for (id, typ, name) in entities {
            self.entity_ids.push(id);
            self.entity_types.push(typ);
            self.entity_names.push(name);
        }
        self
    }

    /// Builder: Add topic tags
    pub fn with_topics(mut self, topics: Vec<String>) -> Self {
        self.topic_tags = topics;
        self
    }

    /// Builder: Set observation level
    pub fn with_observation_level(mut self, level: impl Into<String>) -> Self {
        self.observation_level = level.into();
        self
    }

    /// Builder: Set causal links
    pub fn with_causal_links(mut self, links: Vec<Uuid>) -> Self {
        self.causal_links = links;
        self
    }

    /// Builder: Set session entity context
    pub fn with_session_entity_context(mut self, context: SessionEntityContext) -> Self {
        self.session_entity_context = Some(context);
        self
    }

    /// Compute content hash for deduplication
    pub fn compute_hash(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.content.hash(&mut hasher);
        self.content_hash = Some(format!("{:x}", hasher.finish()));
    }

    /// Mark this memory as superseded by another
    pub fn supersede(&mut self, _new_memory_id: Uuid) {
        self.t_expired = Some(Utc::now());
        self.is_latest = false;
        // Note: The new memory should set supersedes_id = self.id
    }

    /// Get the Qdrant collection name for this memory
    pub fn collection(&self) -> &'static str {
        self.epistemic_type.collection_name()
    }

    /// Get primary location from session entity context
    pub fn primary_location(&self) -> Option<&str> {
        self.session_entity_context
            .as_ref()
            .and_then(|ctx| ctx.primary_location.as_deref())
    }

    /// Get primary organization from session entity context
    pub fn primary_organization(&self) -> Option<&str> {
        self.session_entity_context
            .as_ref()
            .and_then(|ctx| ctx.primary_organization.as_deref())
    }

    /// Get primary person from session entity context
    pub fn primary_person(&self) -> Option<&str> {
        self.session_entity_context
            .as_ref()
            .and_then(|ctx| ctx.primary_person.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_creation() {
        let memory = Memory::new("user_123", "Alice works at Google");

        assert_eq!(memory.user_id, "user_123");
        assert_eq!(memory.content, "Alice works at Google");
        assert!(memory.is_latest);
        assert_eq!(memory.confidence, 1.0);
    }

    #[test]
    fn test_memory_with_source_adjusts_confidence() {
        let memory = Memory::new("user_123", "Test").with_source(SourceType::AssistantStated);

        assert_eq!(memory.confidence, 0.3);
    }

    #[test]
    fn test_memory_serialization_roundtrip() {
        let memory = Memory::new("user_123", "Test content")
            .with_session("session_456")
            .with_entities(vec![(
                "google".into(),
                "organization".into(),
                "Google".into(),
            )]);

        let json = serde_json::to_string(&memory).unwrap();
        let deserialized: Memory = serde_json::from_str(&json).unwrap();

        assert_eq!(memory.id, deserialized.id);
        assert_eq!(memory.content, deserialized.content);
        assert_eq!(memory.entity_ids, deserialized.entity_ids);
    }

    #[test]
    fn test_memory_collection_routing() {
        let mut memory = Memory::new("user", "test");

        memory.epistemic_type = EpistemicType::World;
        assert_eq!(memory.collection(), "world");

        memory.epistemic_type = EpistemicType::Opinion;
        assert_eq!(memory.collection(), "opinion");
    }

    #[test]
    fn test_memory_preserves_epistemic_type() {
        // Test that with_epistemic_type correctly sets the epistemic type
        let memory = Memory::new("user", "I think Python is better than Rust")
            .with_epistemic_type(EpistemicType::Opinion);

        assert_eq!(memory.epistemic_type, EpistemicType::Opinion);
        assert_eq!(memory.collection(), "opinion");
    }

    #[test]
    fn test_memory_default_epistemic_type() {
        // Test that default epistemic type is World
        let memory = Memory::new("user", "The Earth orbits the Sun");

        assert_eq!(memory.epistemic_type, EpistemicType::World);
        assert_eq!(memory.collection(), "world");
    }

    #[test]
    fn test_memory_all_epistemic_types() {
        // Test all epistemic types route correctly
        let world = Memory::new("user", "fact").with_epistemic_type(EpistemicType::World);
        let experience =
            Memory::new("user", "experience").with_epistemic_type(EpistemicType::Experience);
        let opinion = Memory::new("user", "opinion").with_epistemic_type(EpistemicType::Opinion);
        let observation =
            Memory::new("user", "observation").with_epistemic_type(EpistemicType::Observation);

        assert_eq!(world.collection(), "world");
        assert_eq!(experience.collection(), "experience");
        assert_eq!(opinion.collection(), "opinion");
        assert_eq!(observation.collection(), "observation");
    }

    #[test]
    fn test_uuid_v7_is_time_sortable() {
        let m1 = Memory::new("user", "first");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let m2 = Memory::new("user", "second");

        // UUIDv7 encodes timestamp, so m2.id > m1.id
        assert!(m2.id > m1.id);
    }

    #[test]
    fn test_optional_fields_omitted_in_json() {
        let memory = Memory::new("user", "test");
        let json = serde_json::to_string(&memory).unwrap();

        // Optional None fields should not appear
        assert!(!json.contains("session_id"));
        assert!(!json.contains("content_hash"));
        assert!(!json.contains("t_expired"));
    }

    #[test]
    fn test_compute_hash() {
        let mut memory = Memory::new("user", "test content");
        assert!(memory.content_hash.is_none());

        memory.compute_hash();
        assert!(memory.content_hash.is_some());

        // Same content should produce same hash
        let mut memory2 = Memory::new("user", "test content");
        memory2.compute_hash();
        assert_eq!(memory.content_hash, memory2.content_hash);
    }

    #[test]
    fn test_session_entity_context() {
        let context = SessionEntityContext::new()
            .with_primary_location("Target")
            .with_primary_organization("Google");

        let memory = Memory::new("user", "test content").with_session_entity_context(context);

        assert_eq!(memory.primary_location(), Some("Target"));
        assert_eq!(memory.primary_organization(), Some("Google"));
        assert_eq!(memory.primary_person(), None);
    }

    #[test]
    fn test_session_entity_context_none() {
        let memory = Memory::new("user", "test content");

        assert!(memory.primary_location().is_none());
        assert!(memory.primary_organization().is_none());
        assert!(memory.primary_person().is_none());
    }

    #[test]
    fn test_session_entity_context_is_empty() {
        let context = SessionEntityContext::new();
        assert!(context.is_empty());

        let context = SessionEntityContext::new().with_primary_location("Target");
        assert!(!context.is_empty());
    }

    #[test]
    fn test_session_entity_context_with_entities() {
        let context =
            SessionEntityContext::new().with_entities(vec!["Alice".to_string(), "Bob".to_string()]);

        assert!(!context.is_empty());
        assert_eq!(context.all_entities.len(), 2);
    }
}
