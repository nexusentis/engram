//! Entity type definitions for entity-aware extraction
//!
//! Provides typed entity representation for conversation-level entity tracking.

use serde::{Deserialize, Serialize};

/// Type of named entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    /// Person name (Alice, Bob, Dr. Smith)
    Person,

    /// Organization (Google, Target, Acme Corp)
    Organization,

    /// Location (New York, Paris, home)
    Location,

    /// Product or service (iPhone, Cartwheel app)
    Product,

    /// Date or time reference (January 2024, last Tuesday)
    DateTime,

    /// Other named entity
    Other,
}

impl EntityType {
    /// Check if this type could be a primary reference point
    pub fn is_reference_anchor(&self) -> bool {
        matches!(self, Self::Organization | Self::Location)
    }

    /// Convert from string (case-insensitive)
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "person" => Self::Person,
            "organization" | "org" | "company" | "store" => Self::Organization,
            "location" | "place" | "city" | "country" => Self::Location,
            "product" | "service" | "item" => Self::Product,
            "datetime" | "date" | "time" | "time_period" => Self::DateTime,
            _ => Self::Other,
        }
    }

    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Organization => "organization",
            Self::Location => "location",
            Self::Product => "product",
            Self::DateTime => "datetime",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A named entity extracted from conversation
///
/// This represents a conversation-level entity used during extraction,
/// distinct from `types::Entity` which is the persistent entity stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntity {
    /// Canonical name (normalized)
    pub name: String,

    /// Entity type
    pub entity_type: EntityType,

    /// Turn index where entity first appeared (0-indexed)
    pub first_mention_turn: usize,

    /// Alternative names/references for this entity
    pub aliases: Vec<String>,

    /// Number of times mentioned in conversation
    pub mention_count: usize,

    /// Confidence score (0-1) for entity extraction
    pub confidence: f32,
}

impl ConversationEntity {
    /// Create a new entity
    pub fn new(name: impl Into<String>, entity_type: EntityType, turn: usize) -> Self {
        Self {
            name: name.into(),
            entity_type,
            first_mention_turn: turn,
            aliases: vec![],
            mention_count: 1,
            confidence: 1.0,
        }
    }

    /// Add an alias for this entity
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Set confidence score
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Set mention count
    pub fn with_mention_count(mut self, count: usize) -> Self {
        self.mention_count = count;
        self
    }

    /// Increment mention count
    pub fn increment_mention(&mut self) {
        self.mention_count += 1;
    }

    /// Check if a string matches this entity (case-insensitive)
    pub fn matches(&self, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        let name_lower = self.name.to_lowercase();

        if text_lower == name_lower {
            return true;
        }

        self.aliases.iter().any(|a| a.to_lowercase() == text_lower)
    }

    /// Check if entity is salient (frequently mentioned)
    pub fn is_salient(&self, threshold: usize) -> bool {
        self.mention_count >= threshold
    }

    /// Get the normalized ID for this entity
    pub fn normalized_id(&self) -> String {
        self.name
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .trim()
            .replace(' ', "_")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_type_from_str_loose() {
        assert_eq!(EntityType::from_str_loose("person"), EntityType::Person);
        assert_eq!(EntityType::from_str_loose("PERSON"), EntityType::Person);
        assert_eq!(
            EntityType::from_str_loose("organization"),
            EntityType::Organization
        );
        assert_eq!(
            EntityType::from_str_loose("store"),
            EntityType::Organization
        );
        assert_eq!(
            EntityType::from_str_loose("company"),
            EntityType::Organization
        );
        assert_eq!(EntityType::from_str_loose("location"), EntityType::Location);
        assert_eq!(EntityType::from_str_loose("city"), EntityType::Location);
        assert_eq!(EntityType::from_str_loose("product"), EntityType::Product);
        assert_eq!(EntityType::from_str_loose("datetime"), EntityType::DateTime);
        assert_eq!(EntityType::from_str_loose("unknown"), EntityType::Other);
    }

    #[test]
    fn test_entity_type_is_reference_anchor() {
        assert!(EntityType::Organization.is_reference_anchor());
        assert!(EntityType::Location.is_reference_anchor());
        assert!(!EntityType::Person.is_reference_anchor());
        assert!(!EntityType::Product.is_reference_anchor());
    }

    #[test]
    fn test_entity_creation() {
        let entity = ConversationEntity::new("Alice", EntityType::Person, 0)
            .with_alias("Dr. Alice")
            .with_confidence(0.95);

        assert_eq!(entity.name, "Alice");
        assert_eq!(entity.entity_type, EntityType::Person);
        assert_eq!(entity.first_mention_turn, 0);
        assert_eq!(entity.aliases, vec!["Dr. Alice"]);
        assert_eq!(entity.mention_count, 1);
        assert!((entity.confidence - 0.95).abs() < 0.01);
    }

    #[test]
    fn test_entity_matches() {
        let entity =
            ConversationEntity::new("Alice", EntityType::Person, 0).with_alias("Dr. Alice");

        assert!(entity.matches("alice")); // Case insensitive
        assert!(entity.matches("Alice"));
        assert!(entity.matches("Dr. Alice"));
        assert!(!entity.matches("Bob"));
    }

    #[test]
    fn test_entity_is_salient() {
        let mut entity = ConversationEntity::new("Alice", EntityType::Person, 0);
        assert!(!entity.is_salient(2)); // Only 1 mention

        entity.increment_mention();
        assert!(entity.is_salient(2)); // Now 2 mentions

        entity.increment_mention();
        assert!(entity.is_salient(3)); // Now 3 mentions
    }

    #[test]
    fn test_entity_normalized_id() {
        let entity = ConversationEntity::new("Alice Smith", EntityType::Person, 0);
        assert_eq!(entity.normalized_id(), "alice_smith");

        let entity = ConversationEntity::new("Google Inc.", EntityType::Organization, 0);
        assert_eq!(entity.normalized_id(), "google_inc");
    }

    #[test]
    fn test_entity_serialization() {
        let entity = ConversationEntity::new("Google", EntityType::Organization, 0);
        let json = serde_json::to_string(&entity).unwrap();

        assert!(json.contains("\"name\":\"Google\""));
        assert!(json.contains("\"entity_type\":\"organization\""));

        let deserialized: ConversationEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "Google");
        assert_eq!(deserialized.entity_type, EntityType::Organization);
    }
}
